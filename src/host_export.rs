//! 主机库加密导出/导入。
//! AES-256-GCM + PBKDF2-HMAC-SHA256(600_000)。
//! 文件是 JSON envelope，便于排查；payload 是 base64 密文。

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use nexshell::host_management::HostCardSnapshot;

const ENVELOPE_VERSION: u32 = 1;
const PBKDF2_ITERS: u32 = 600_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    kdf: String,
    kdf_iters: u32,
    cipher: String,
    salt_b64: String,
    nonce_b64: String,
    ciphertext_b64: String,
}

/// 导出文件里的分组元数据，使带分组的主机能跨档案 round-trip。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportGroup {
    pub id: String,
    pub name: String,
}

/// 解密结果：分组先于主机重建，避免 host.group_id 悬空。
#[derive(Debug)]
pub struct ImportedLibrary {
    pub groups: Vec<ExportGroup>,
    pub hosts: Vec<HostCardSnapshot>,
}

#[derive(Serialize)]
struct ExportPayload<'a> {
    schema: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    groups: Vec<ExportGroup>,
    hosts: &'a [HostCardSnapshot],
}

#[derive(Deserialize)]
struct ImportPayload {
    schema: String,
    #[serde(default)] // 旧版导出文件无 groups 字段，按空处理
    groups: Vec<ExportGroup>,
    hosts: Vec<HostCardSnapshot>,
}

/// 用 `password` 加密 `hosts`，返回写入磁盘的 JSON envelope 字节。
pub fn encrypt_export(
    hosts: &[HostCardSnapshot],
    groups: &[ExportGroup],
    password: &str,
) -> Result<Vec<u8>, String> {
    let payload = ExportPayload {
        schema: "nexshell.host_library.v1",
        groups: groups.to_vec(),
        hosts,
    };
    let plaintext = serde_json::to_vec(&payload).map_err(|e| format!("序列化失败：{}", e))?;

    let mut rng = rand::thread_rng();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut nonce);

    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ITERS, &mut key);

    let cipher = Aes256Gcm::new((&key).into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|e| format!("加密失败：{}", e))?;

    let envelope = Envelope {
        v: ENVELOPE_VERSION,
        kdf: "pbkdf2-hmac-sha256".to_string(),
        kdf_iters: PBKDF2_ITERS,
        cipher: "aes-256-gcm".to_string(),
        salt_b64: B64.encode(salt),
        nonce_b64: B64.encode(nonce),
        ciphertext_b64: B64.encode(&ciphertext),
    };
    serde_json::to_vec_pretty(&envelope).map_err(|e| format!("封装失败：{}", e))
}

/// 用 `password` 解密导出的 envelope 字节，返回主机列表。
pub fn decrypt_export(bytes: &[u8], password: &str) -> Result<ImportedLibrary, String> {
    let envelope: Envelope =
        serde_json::from_slice(bytes).map_err(|e| format!("不是合法的导出文件：{}", e))?;
    if envelope.v != ENVELOPE_VERSION {
        return Err(format!("不支持的版本：v{}", envelope.v));
    }
    if envelope.kdf != "pbkdf2-hmac-sha256" {
        return Err(format!("不支持的 KDF：{}", envelope.kdf));
    }
    if envelope.cipher != "aes-256-gcm" {
        return Err(format!("不支持的加密算法：{}", envelope.cipher));
    }
    let salt = B64
        .decode(&envelope.salt_b64)
        .map_err(|e| format!("salt 解码失败：{}", e))?;
    let nonce = B64
        .decode(&envelope.nonce_b64)
        .map_err(|e| format!("nonce 解码失败：{}", e))?;
    let ciphertext = B64
        .decode(&envelope.ciphertext_b64)
        .map_err(|e| format!("ciphertext 解码失败：{}", e))?;
    if nonce.len() != NONCE_LEN {
        return Err(format!("nonce 长度异常：{}", nonce.len()));
    }

    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, envelope.kdf_iters, &mut key);
    let cipher = Aes256Gcm::new((&key).into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "密码错误或文件已损坏".to_string())?;

    let payload: ImportPayload =
        serde_json::from_slice(&plaintext).map_err(|e| format!("payload 解析失败：{}", e))?;
    if !payload.schema.starts_with("nexshell.host_library.") {
        return Err(format!("未知 schema：{}", payload.schema));
    }
    Ok(ImportedLibrary {
        groups: payload.groups,
        hosts: payload.hosts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexshell::host_management::{HostConnectionConfig, HostSystemIcon};

    fn sample_host() -> HostCardSnapshot {
        let mut conn = HostConnectionConfig::ssh("10.0.0.1", 22, "matt");
        conn.password = Some("hunter2".to_string());
        HostCardSnapshot {
            id: "h-1".to_string(),
            name: "edge-1".to_string(),
            protocol: "SSH".to_string(),
            endpoint: conn.endpoint("SSH"),
            description: "edge node".to_string(),
            connection: conn,
            group_id: Some("g-prod".to_string()),
            tags: vec!["prod".to_string()],
            system: HostSystemIcon::Linux,
            sort_order: 0,
        }
    }

    #[test]
    fn encrypt_then_decrypt_roundtrip() {
        let hosts = vec![sample_host()];
        let groups = vec![ExportGroup {
            id: "g-prod".to_string(),
            name: "Production".to_string(),
        }];
        let bytes = encrypt_export(&hosts, &groups, "correct horse").unwrap();
        let restored = decrypt_export(&bytes, "correct horse").unwrap();
        assert_eq!(restored.hosts.len(), 1);
        assert_eq!(restored.hosts[0].name, "edge-1");
        assert_eq!(
            restored.hosts[0].connection.password.as_deref(),
            Some("hunter2")
        );
        assert_eq!(restored.groups.len(), 1);
        assert_eq!(restored.groups[0].id, "g-prod");
        assert_eq!(restored.groups[0].name, "Production");
    }

    #[test]
    fn decrypt_with_wrong_password_fails() {
        let hosts = vec![sample_host()];
        let bytes = encrypt_export(&hosts, &[], "right").unwrap();
        let err = decrypt_export(&bytes, "wrong").unwrap_err();
        assert!(err.contains("密码"));
    }

    #[test]
    fn legacy_file_without_groups_still_imports() {
        // 旧版 envelope（payload 无 groups 字段）必须仍可导入，groups 视为空
        let hosts = vec![sample_host()];
        let bytes = encrypt_export(&hosts, &[], "pw").unwrap();
        let restored = decrypt_export(&bytes, "pw").unwrap();
        assert_eq!(restored.hosts.len(), 1);
        assert!(restored.groups.is_empty());
    }
}
