//! RDP 服务端证书信任库（TOFU）：按 `host:port` 记一条 leaf 证书 SHA-256 指纹。
//! 与 hosts 同库；RDP 自签证书是常态，故不做 CA 链校验，只做每主机指纹固定。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};

pub const RDP_CERT_TRUST_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS rdp_cert_trust (
    endpoint    TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
"#;

/// TOFU 比对结论。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CertTrustVerdict {
    /// 与已记录指纹一致。
    Trusted,
    /// 首次连接，已记录。
    Pinned,
    /// 与已记录指纹不符，必须拒绝。
    Mismatch { expected: String },
}

// 由 host db 初始化时调用（幂等）。
pub fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(RDP_CERT_TRUST_SCHEMA)
        .map_err(|e| format!("initialize rdp_cert_trust table: {e}"))
}

/// 信任库主键：`host:port`，host 统一小写。
pub fn endpoint_key(host: &str, port: u16) -> String {
    format!("{}:{port}", host.trim().to_ascii_lowercase())
}

/// leaf 证书 DER 的 SHA-256 指纹，大写十六进制冒号分隔（与 openssl / mstsc 展示一致）。
pub fn sha256_fingerprint(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn open_rw(db_path: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|e| format!("open NexShell db {}: {e}", db_path.display()))?;
    ensure_schema(&conn)?;
    Ok(conn)
}

/// 比对指纹：一致放行，未记录则记录，不一致返回 Mismatch（调用方负责断开）。
pub fn verify_or_pin_at(
    db_path: &Path,
    endpoint: &str,
    fingerprint: &str,
) -> Result<CertTrustVerdict, String> {
    let conn = open_rw(db_path)?;
    let recorded: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM rdp_cert_trust WHERE endpoint = ?1",
            params![endpoint],
            |row| row.get(0),
        )
        .ok();

    match recorded {
        Some(expected) if expected == fingerprint => Ok(CertTrustVerdict::Trusted),
        Some(expected) => Ok(CertTrustVerdict::Mismatch { expected }),
        None => {
            conn.execute(
                "INSERT OR REPLACE INTO rdp_cert_trust (endpoint, fingerprint, updated_at)
                 VALUES (?1, ?2, ?3)",
                params![endpoint, fingerprint, now_secs()],
            )
            .map_err(|e| format!("pin rdp cert for {endpoint}: {e}"))?;
            Ok(CertTrustVerdict::Pinned)
        }
    }
}

/// 用默认库路径做一次 TOFU 比对。
pub fn verify_or_pin(endpoint: &str, cert_der: &[u8]) -> Result<CertTrustVerdict, String> {
    let db_path = crate::host_management::default_database_path()
        .ok_or_else(|| "cannot resolve NexShell db path".to_string())?;
    verify_or_pin_at(&db_path, endpoint, &sha256_fingerprint(cert_der))
}

/// 清除某 endpoint 的指纹记录：下次连接重新按首次信任固定。
pub fn forget_at(db_path: &Path, endpoint: &str) -> Result<(), String> {
    let conn = open_rw(db_path)?;
    conn.execute(
        "DELETE FROM rdp_cert_trust WHERE endpoint = ?1",
        params![endpoint],
    )
    .map(|_| ())
    .map_err(|e| format!("forget rdp cert for {endpoint}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_uppercase_hex_of_sha256() {
        // SHA-256("") 的前两个字节是 e3 b0。
        let fingerprint = sha256_fingerprint(b"");
        assert!(fingerprint.starts_with("E3:B0:"));
        assert_eq!(fingerprint.split(':').count(), 32);
        assert_ne!(fingerprint, sha256_fingerprint(b"cert"));
    }

    #[test]
    fn endpoint_key_normalizes_host_case() {
        assert_eq!(
            endpoint_key(" RDP.Example.COM ", 3389),
            "rdp.example.com:3389"
        );
    }

    #[test]
    fn first_cert_is_pinned_then_matched_and_mismatch_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("nexshell.db");
        let endpoint = endpoint_key("rdp.example.com", 3389);
        let first = sha256_fingerprint(b"cert-a");
        let second = sha256_fingerprint(b"cert-b");

        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint, &first).unwrap(),
            CertTrustVerdict::Pinned
        );
        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint, &first).unwrap(),
            CertTrustVerdict::Trusted
        );
        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint, &second).unwrap(),
            CertTrustVerdict::Mismatch {
                expected: first.clone()
            }
        );
        // 拒绝后记录不变，另一个 endpoint 互不影响。
        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint, &first).unwrap(),
            CertTrustVerdict::Trusted
        );
        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint_key("other", 3389), &second).unwrap(),
            CertTrustVerdict::Pinned
        );
    }

    #[test]
    fn forget_allows_repinning() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("nexshell.db");
        let endpoint = endpoint_key("rdp.example.com", 3389);
        let first = sha256_fingerprint(b"cert-a");
        let second = sha256_fingerprint(b"cert-b");

        verify_or_pin_at(&db_path, &endpoint, &first).unwrap();
        forget_at(&db_path, &endpoint).unwrap();
        assert_eq!(
            verify_or_pin_at(&db_path, &endpoint, &second).unwrap(),
            CertTrustVerdict::Pinned
        );
    }
}
