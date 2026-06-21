//! SSH 私钥库：导入一次、多主机引用。ssh_keys 表与 hosts 同库。

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};

pub const SSH_KEYS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ssh_keys (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    content     TEXT NOT NULL,
    passphrase  TEXT,
    key_type    TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);
"#;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SshKeyRecord {
    pub id: String,
    pub name: String,
    pub content: String,
    pub passphrase: Option<String>,
    pub key_type: String,
    pub created_at: i64,
    pub updated_at: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

pub fn new_ssh_key_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("sshkey-{}-{nanos}", std::process::id())
}

// 由 host db 初始化时调用：建 ssh_keys 表 + 给 hosts 补 key_id 列（幂等）。
pub fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SSH_KEYS_SCHEMA)
        .map_err(|e| format!("initialize ssh_keys table: {e}"))?;
    let _ = conn.execute("ALTER TABLE hosts ADD COLUMN key_id TEXT", []);
    Ok(())
}

fn open_rw(db_path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("open NexShell db {}: {e}", db_path.display()))
}

fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<SshKeyRecord> {
    Ok(SshKeyRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        content: row.get(2)?,
        passphrase: row.get(3)?,
        key_type: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

const SELECT_COLS: &str = "id, name, content, passphrase, key_type, created_at, updated_at";

// 列表 + 各密钥被多少主机引用（一钥多机价值的可视化）。
pub fn list_ssh_keys_with_usage(db_path: &Path) -> Result<Vec<(SshKeyRecord, usize)>, String> {
    let conn = open_rw(db_path)?;
    let sql = format!(
        "SELECT {SELECT_COLS},
         (SELECT COUNT(*) FROM hosts h WHERE h.key_id = ssh_keys.id) AS usage
         FROM ssh_keys ORDER BY created_at DESC"
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare keys: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            let record = row_to_record(row)?;
            let usage: i64 = row.get(7)?;
            Ok((record, usage.max(0) as usize))
        })
        .map_err(|e| format!("query keys: {e}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("map keys: {e}"))
}

pub fn get_ssh_key(db_path: &Path, id: &str) -> Result<Option<SshKeyRecord>, String> {
    let conn = open_rw(db_path)?;
    let sql = format!("SELECT {SELECT_COLS} FROM ssh_keys WHERE id = ?1");
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare key: {e}"))?;
    let mut rows = stmt
        .query_map(params![id], row_to_record)
        .map_err(|e| format!("query key: {e}"))?;
    match rows.next() {
        Some(record) => Ok(Some(record.map_err(|e| format!("map key: {e}"))?)),
        None => Ok(None),
    }
}

pub fn upsert_ssh_key(db_path: &Path, key: &SshKeyRecord) -> Result<(), String> {
    let conn = open_rw(db_path)?;
    conn.execute(
        "INSERT INTO ssh_keys (id, name, content, passphrase, key_type, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            content = excluded.content,
            passphrase = excluded.passphrase,
            key_type = excluded.key_type,
            updated_at = excluded.updated_at",
        params![
            key.id,
            key.name.trim(),
            key.content.trim(),
            key.passphrase.as_deref().map(str::trim).filter(|p| !p.is_empty()),
            key.key_type,
            key.created_at,
            key.updated_at,
        ],
    )
    .map_err(|e| format!("upsert ssh key: {e}"))?;
    Ok(())
}

// 删除密钥：先把引用它的主机 key_id 置空，避免悬空引用。
pub fn delete_ssh_key(db_path: &Path, id: &str) -> Result<(), String> {
    let conn = open_rw(db_path)?;
    let _ = conn.execute("UPDATE hosts SET key_id = NULL WHERE key_id = ?1", params![id]);
    conn.execute("DELETE FROM ssh_keys WHERE id = ?1", params![id])
        .map_err(|e| format!("delete ssh key: {e}"))?;
    Ok(())
}

// 编辑已有密钥的名称/口令（内容不变）：名称空则保留原名，重测类型、更新 updated_at。
pub fn update_ssh_key_meta(
    db_path: &Path,
    id: &str,
    name: &str,
    passphrase: Option<String>,
) -> Result<(), String> {
    let mut record = match get_ssh_key(db_path, id)? {
        Some(record) => record,
        None => return Err("密钥不存在".to_string()),
    };
    let passphrase = passphrase
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let name = name.trim();
    if !name.is_empty() {
        record.name = name.to_string();
    }
    // 新口令解不开私钥但原口令能解 → 视为误清空/填错，保留原口令与类型，避免把可用密钥改坏。
    let new_works = decode(&record.content, passphrase.as_deref()).is_some();
    let old_works = decode(&record.content, record.passphrase.as_deref()).is_some();
    if new_works || !old_works {
        record.key_type = detect_key_type(&record.content, passphrase.as_deref());
        record.passphrase = passphrase;
    }
    record.updated_at = now_secs();
    upsert_ssh_key(db_path, &record)
}

// 从私钥文本构建记录（解析类型）。passphrase 空串归一为 None。
pub fn build_record(name: &str, content: &str, passphrase: Option<String>) -> SshKeyRecord {
    let now = now_secs();
    let passphrase = passphrase
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let key_type = detect_key_type(content, passphrase.as_deref());
    SshKeyRecord {
        id: new_ssh_key_id(),
        name: name.trim().to_string(),
        content: content.trim().to_string(),
        passphrase,
        key_type,
        created_at: now,
        updated_at: now,
    }
}

fn decode(content: &str, passphrase: Option<&str>) -> Option<russh::keys::key::KeyPair> {
    let pass = passphrase.map(str::trim).filter(|p| !p.is_empty());
    russh::keys::decode_secret_key(content.trim(), pass).ok()
}

// 短类型标签：rsa / ed25519 / ecdsa / dsa；解不开(加密无口令)时按 PEM 头猜。
pub fn detect_key_type(content: &str, passphrase: Option<&str>) -> String {
    if let Some(key) = decode(content, passphrase) {
        return short_algo(key.name());
    }
    guess_type_from_text(content)
}

fn short_algo(algo: &str) -> String {
    let a = algo.to_ascii_lowercase();
    if a.contains("ed25519") {
        "ed25519".to_string()
    } else if a.contains("ecdsa") {
        "ecdsa".to_string()
    } else if a.contains("rsa") {
        "rsa".to_string()
    } else if a.contains("dss") || a.contains("dsa") {
        "dsa".to_string()
    } else {
        algo.to_string()
    }
}

fn guess_type_from_text(content: &str) -> String {
    let up = content.to_ascii_uppercase();
    if up.contains("BEGIN RSA") {
        "rsa".to_string()
    } else if up.contains("BEGIN EC") {
        "ecdsa".to_string()
    } else if up.contains("BEGIN DSA") {
        "dsa".to_string()
    } else {
        String::new()
    }
}

// 推导 openssh 公钥（详情页展示 / 后续 copy-to-server 用）；加密无口令则 None。
pub fn derive_public_key(content: &str, passphrase: Option<&str>) -> Option<String> {
    use russh::keys::PublicKeyBase64;
    let key = decode(content, passphrase)?;
    let public = key.clone_public_key().ok()?;
    Some(format!("{} {}", public.name(), public.public_key_base64()))
}
