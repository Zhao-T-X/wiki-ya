//! 密钥存储（SEC-001 / SEC-002）。
//!
//! API Key 属于 **Secret**：不写明文进 SQLite，而是用 **AES-256-GCM** 加密后
//! 存进 `settings` 表（键名 `ai.api_key.enc`）。这取代了早期的 macOS Keychain 方案。
//!
//! ```text
//! settings 表                       应用数据目录
//! ├── ai.base_url                    └── secret.key （32 字节主密钥，权限 0600）
//! ├── ai.model
//! ├── ai.embedding_model
//! ├── ai.token_budget
//! └── ai.api_key.enc  ← AES-256-GCM 密文
//! ```
//!
//! 主密钥首次运行时随机生成并落盘；之后每次启动读取。密文格式版本化
//! （`v1:<nonce hex>:<ciphertext+tag hex>`），便于将来更换算法。
//!
//! ## ⚠️ 安全边界（诚实说明，不假装更强）
//!
//! 主密钥与数据库在**同一台机器、同一个用户可读的目录**下。因此本方案：
//! - ✅ 能防：数据库文件被单独复制、误传、误提交到仓库时 Key 泄露；
//! - ❌ 不能防：能够读取你 home 目录的本地攻击者（他能连主密钥一起拿走）。
//!
//! 想要真正的「静态加密」，需要**用户口令派生密钥**（每次启动输入）或系统级
//! 密钥库。这是刻意的取舍：用一次「重新输入 Key」的代价，换掉对 Keychain 的依赖。
//!
//! 另外：主密钥文件**丢失或更换**会导致已存密文无法解密，此时系统会如实报告
//! 「解密失败」并视为未配置，不会静默伪造一个空 Key。

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use aes_gcm::aead::{Aead, Nonce};
use aes_gcm::{Aes256Gcm, KeyInit};
use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::infrastructure::settings_repository;

/// 加密后的 API Key 在 `settings` 表中的键名。
pub const KEY_API_KEY_ENC: &str = "ai.api_key.enc";
/// 旧版明文键名。仅用于迁移与清理，**不再作为读取来源**。
pub const KEY_API_KEY_LEGACY: &str = "ai.api_key";
/// 主密钥文件名（位于应用数据目录，权限 0600）。
pub const KEY_FILE_NAME: &str = "secret.key";

/// 主密钥长度（AES-256）。
const KEY_LEN: usize = 32;
/// GCM nonce 长度（标准 96 bit）。
const NONCE_LEN: usize = 12;
/// 密文格式版本前缀。
const VERSION: &str = "v1";

// ---------------------------------------------------------------------------
// 加解密
// ---------------------------------------------------------------------------

/// AES-256-GCM 加解密器（持有主密钥）。
#[derive(Clone)]
pub struct SecretCipher {
    key: [u8; KEY_LEN],
}

impl SecretCipher {
    /// 用给定的 32 字节主密钥构造（测试 / 已有密钥场景）。
    pub fn from_key(key: [u8; KEY_LEN]) -> Self {
        Self { key }
    }

    /// 从密钥文件加载；不存在或长度异常时重新生成（权限 0600）。
    pub fn load_or_create(path: &Path) -> AppResult<Self> {
        match fs::read(path) {
            Ok(bytes) if bytes.len() == KEY_LEN => {
                let mut key = [0u8; KEY_LEN];
                key.copy_from_slice(&bytes);
                Ok(Self::from_key(key))
            }
            Ok(bytes) => {
                // 长度不对说明文件被破坏：重新生成。旧密文将无法解密（会如实报告）。
                crate::log_warn!(
                    "主密钥文件长度异常（{} 字节，期望 {KEY_LEN}），将重新生成：{}",
                    bytes.len(),
                    path.display()
                );
                Self::generate(path)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::generate(path),
            Err(err) => Err(AppError::Internal(format!(
                "读取主密钥文件失败（{}）：{err}",
                path.display()
            ))),
        }
    }

    fn generate(path: &Path) -> AppResult<Self> {
        let mut key = [0u8; KEY_LEN];
        getrandom::getrandom(&mut key)
            .map_err(|err| AppError::Internal(format!("生成主密钥失败：{err}")))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        write_private(path, &key)?;
        crate::log_info!("已生成 API Key 主密钥：{}", path.display());
        Ok(Self::from_key(key))
    }

    fn cipher(&self) -> AppResult<Aes256Gcm> {
        Aes256Gcm::new_from_slice(&self.key)
            .map_err(|_| AppError::Internal("初始化 AES-256-GCM 失败".into()))
    }

    /// 加密明文，返回版本化密文（每次调用使用全新随机 nonce）。
    pub fn encrypt(&self, plaintext: &str) -> AppResult<String> {
        let cipher = self.cipher()?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        getrandom::getrandom(&mut nonce_bytes)
            .map_err(|err| AppError::Internal(format!("生成随机数失败：{err}")))?;

        let nonce = Nonce::<Aes256Gcm>::try_from(nonce_bytes.as_slice())
            .map_err(|_| AppError::Internal("生成 nonce 失败".into()))?;
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| AppError::Internal("加密 API Key 失败".into()))?;

        Ok(format!(
            "{VERSION}:{}:{}",
            to_hex(&nonce_bytes),
            to_hex(&ciphertext)
        ))
    }

    /// 解密版本化密文；主密钥不匹配或数据被篡改都会失败。
    pub fn decrypt(&self, token: &str) -> AppResult<String> {
        let mut parts = token.splitn(3, ':');
        let (Some(version), Some(nonce_hex), Some(cipher_hex)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(AppError::Internal("密文格式不可识别".into()));
        };
        if version != VERSION {
            return Err(AppError::Internal(format!("不支持的密文版本：{version}")));
        }

        let nonce_bytes = from_hex(nonce_hex)?;
        if nonce_bytes.len() != NONCE_LEN {
            return Err(AppError::Internal("密文 nonce 长度非法".into()));
        }
        let ciphertext = from_hex(cipher_hex)?;

        let cipher = self.cipher()?;
        let nonce = Nonce::<Aes256Gcm>::try_from(nonce_bytes.as_slice())
            .map_err(|_| AppError::Internal("密文 nonce 长度非法".into()))?;
        let plaintext = cipher
            .decrypt(&nonce, ciphertext.as_slice())
            .map_err(|_| AppError::Internal("解密 API Key 失败（主密钥不匹配或数据被篡改）".into()))?;

        String::from_utf8(plaintext)
            .map_err(|err| AppError::Internal(format!("解密结果不是有效 UTF-8：{err}")))
    }
}

/// 主密钥文件落到磁盘时收紧权限（仅所有者可读写）。
fn write_private(path: &Path, bytes: &[u8]) -> AppResult<()> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn from_hex(text: &str) -> AppResult<Vec<u8>> {
    if text.len() % 2 != 0 {
        return Err(AppError::Internal("密文十六进制长度非法".into()));
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        out.push((hex_value(pair[0])? << 4) | hex_value(pair[1])?);
    }
    Ok(out)
}

fn hex_value(byte: u8) -> AppResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(AppError::Internal("密文含非十六进制字符".into())),
    }
}

// ---------------------------------------------------------------------------
// 进程级加密器
// ---------------------------------------------------------------------------

static CIPHER: OnceLock<SecretCipher> = OnceLock::new();

/// 注入进程级加密器（启动时调用一次；测试也可注入固定密钥）。
pub fn install_cipher(cipher: SecretCipher) {
    let _ = CIPHER.set(cipher);
}

/// 是否已就绪。未就绪时读操作按「未配置」处理，写操作会明确报错。
pub fn is_ready() -> bool {
    CIPHER.get().is_some()
}

// ---------------------------------------------------------------------------
// 读写 API Key
// ---------------------------------------------------------------------------

/// 加密并保存 API Key；同时清掉历史明文行。
pub fn save_api_key(conn: &Connection, plaintext: &str) -> AppResult<()> {
    save_api_key_with(conn, require_cipher()?, plaintext)
}

/// 清除 API Key（幂等）：密文行与历史明文行都删掉。
pub fn clear_api_key(conn: &Connection) -> AppResult<()> {
    settings_repository::delete_setting(conn, KEY_API_KEY_ENC)?;
    settings_repository::delete_setting(conn, KEY_API_KEY_LEGACY)?;
    Ok(())
}

/// 读取并解密 API Key。未配置 / 加密器未就绪都返回 `Ok(None)`（不视为错误）；
/// 解密失败会记日志并返回 `Ok(None)`，绝不让 AI 因存储异常而崩溃。
pub fn load_api_key(conn: &Connection) -> AppResult<Option<String>> {
    match CIPHER.get() {
        Some(cipher) => load_api_key_with(conn, cipher),
        None => Ok(None),
    }
}

/// 便于测试注入加密器的读写实现。
pub fn save_api_key_with(
    conn: &Connection,
    cipher: &SecretCipher,
    plaintext: &str,
) -> AppResult<()> {
    let token = cipher.encrypt(plaintext)?;
    settings_repository::set_setting(conn, KEY_API_KEY_ENC, &token)?;
    // 同一把 Key 只保留一份，避免遗留明文。
    settings_repository::delete_setting(conn, KEY_API_KEY_LEGACY)?;
    Ok(())
}

/// 便于测试注入加密器的读取实现。
pub fn load_api_key_with(conn: &Connection, cipher: &SecretCipher) -> AppResult<Option<String>> {
    let Some(token) = settings_repository::get_setting(conn, KEY_API_KEY_ENC)? else {
        return Ok(None);
    };
    let token = token.trim();
    if token.is_empty() {
        return Ok(None);
    }

    match cipher.decrypt(token) {
        Ok(key) if !key.trim().is_empty() => Ok(Some(key)),
        Ok(_) => Ok(None),
        Err(err) => {
            crate::log_error!("API Key 解密失败，视为未配置：{err}");
            Ok(None)
        }
    }
}

fn require_cipher() -> AppResult<&'static SecretCipher> {
    CIPHER
        .get()
        .ok_or_else(|| AppError::Internal("密钥加密器尚未初始化，无法保存 API Key".into()))
}

// ---------------------------------------------------------------------------
// SEC-002：历史明文 API Key 迁移
// ---------------------------------------------------------------------------

/// 把历史版本遗留在 `settings` 表里的明文 `ai.api_key` 加密后迁移。
///
/// 顺序（严格）：加密写入 → **读回校验** → 才删除明文。
/// 任一步失败都**不删除**明文，只记警告；因此本函数永不因迁移失败而中断启动。
pub fn migrate_legacy_api_key(conn: &Connection) -> AppResult<()> {
    let Some(cipher) = CIPHER.get() else {
        return Ok(());
    };
    migrate_with(conn, cipher)
}

/// 便于测试注入加密器的迁移实现。
pub fn migrate_with(conn: &Connection, cipher: &SecretCipher) -> AppResult<()> {
    let Some(legacy) = settings_repository::get_setting(conn, KEY_API_KEY_LEGACY)? else {
        return Ok(());
    };
    let legacy = legacy.trim().to_string();

    // 旧值只是「显式清除」留下的空串 → 直接清掉这一行。
    if legacy.is_empty() {
        settings_repository::delete_setting(conn, KEY_API_KEY_LEGACY)?;
        return Ok(());
    }

    let token = match cipher.encrypt(&legacy) {
        Ok(token) => token,
        Err(err) => {
            crate::log_warn!("无法加密历史明文 API Key（保留原值）：{err}");
            return Ok(());
        }
    };
    if let Err(err) = settings_repository::set_setting(conn, KEY_API_KEY_ENC, &token) {
        crate::log_warn!("无法写入加密后的 API Key（保留明文原值）：{err}");
        return Ok(());
    }

    match load_api_key_with(conn, cipher) {
        Ok(Some(read_back)) if read_back == legacy => {
            settings_repository::delete_setting(conn, KEY_API_KEY_LEGACY)?;
            crate::log_info!("已把明文 API Key 加密存入 SQLite（明文已删除）");
        }
        Ok(_) => crate::log_warn!("API Key 迁移读回校验失败，保留明文原值不动"),
        Err(err) => crate::log_warn!("API Key 迁移读回失败（保留明文原值）：{err}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    fn test_cipher() -> SecretCipher {
        SecretCipher::from_key([7u8; KEY_LEN])
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let cipher = test_cipher();
        let token = cipher.encrypt("sk-round-trip").unwrap();
        assert_eq!(cipher.decrypt(&token).unwrap(), "sk-round-trip");
    }

    #[test]
    fn ciphertext_hides_plaintext_and_uses_a_fresh_nonce() {
        let cipher = test_cipher();
        let first = cipher.encrypt("sk-same").unwrap();
        let second = cipher.encrypt("sk-same").unwrap();

        assert!(!first.contains("sk-same"), "密文不得包含明文");
        assert_ne!(first, second, "每次加密必须使用新的随机 nonce");
        assert_eq!(cipher.decrypt(&first).unwrap(), "sk-same");
        assert_eq!(cipher.decrypt(&second).unwrap(), "sk-same");
    }

    #[test]
    fn tampered_ciphertext_is_rejected() {
        let cipher = test_cipher();
        let mut token = cipher.encrypt("sk-abc").unwrap();
        let last = token.pop().unwrap();
        token.push(if last == '0' { '1' } else { '0' });
        assert!(cipher.decrypt(&token).is_err(), "篡改必须被 GCM tag 拒绝");
    }

    #[test]
    fn a_different_master_key_cannot_decrypt() {
        let token = test_cipher().encrypt("sk-abc").unwrap();
        let other = SecretCipher::from_key([9u8; KEY_LEN]);
        assert!(other.decrypt(&token).is_err());
    }

    #[test]
    fn malformed_tokens_are_rejected_without_panicking() {
        let cipher = test_cipher();
        assert!(cipher.decrypt("").is_err());
        assert!(cipher.decrypt("v1:zz:zz").is_err());
        assert!(cipher.decrypt("v9:00:00").is_err(), "未知版本必须被拒绝");
        assert!(cipher.decrypt("v1:00").is_err(), "缺字段必须被拒绝");
    }

    #[test]
    fn save_and_load_round_trip_stores_ciphertext_only() {
        let conn = memory_db();
        let cipher = test_cipher();

        save_api_key_with(&conn, &cipher, "sk-live-1").unwrap();

        assert_eq!(
            load_api_key_with(&conn, &cipher).unwrap().as_deref(),
            Some("sk-live-1")
        );

        // 落库的是版本化密文，不是明文。
        let stored = settings_repository::get_setting(&conn, KEY_API_KEY_ENC)
            .unwrap()
            .unwrap();
        assert!(stored.starts_with("v1:"));
        assert!(!stored.contains("sk-live-1"));
    }

    #[test]
    fn saving_clears_the_legacy_plaintext_row() {
        let conn = memory_db();
        let cipher = test_cipher();
        settings_repository::set_setting(&conn, KEY_API_KEY_LEGACY, "sk-old").unwrap();

        save_api_key_with(&conn, &cipher, "sk-new").unwrap();

        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_LEGACY)
            .unwrap()
            .is_none());
        assert_eq!(
            load_api_key_with(&conn, &cipher).unwrap().as_deref(),
            Some("sk-new")
        );
    }

    #[test]
    fn clear_removes_both_rows_and_is_idempotent() {
        let conn = memory_db();
        let cipher = test_cipher();
        save_api_key_with(&conn, &cipher, "sk-x").unwrap();

        clear_api_key(&conn).unwrap();

        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_ENC)
            .unwrap()
            .is_none());
        assert!(load_api_key_with(&conn, &cipher).unwrap().is_none());
        // 再清一次不应报错
        clear_api_key(&conn).unwrap();
    }

    #[test]
    fn load_returns_none_when_nothing_is_stored() {
        let conn = memory_db();
        assert!(load_api_key_with(&conn, &test_cipher()).unwrap().is_none());
    }

    #[test]
    fn migration_encrypts_plaintext_and_deletes_the_row() {
        let conn = memory_db();
        let cipher = test_cipher();
        settings_repository::set_setting(&conn, KEY_API_KEY_LEGACY, "sk-legacy-123").unwrap();

        migrate_with(&conn, &cipher).unwrap();

        assert_eq!(
            load_api_key_with(&conn, &cipher).unwrap().as_deref(),
            Some("sk-legacy-123")
        );
        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_LEGACY)
            .unwrap()
            .is_none());
    }

    #[test]
    fn migration_is_a_noop_when_there_is_no_legacy_key() {
        let conn = memory_db();
        migrate_with(&conn, &test_cipher()).unwrap();
        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_ENC)
            .unwrap()
            .is_none());
    }

    #[test]
    fn blank_legacy_key_is_cleared_without_writing_a_secret() {
        let conn = memory_db();
        settings_repository::set_setting(&conn, KEY_API_KEY_LEGACY, "   ").unwrap();

        migrate_with(&conn, &test_cipher()).unwrap();

        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_ENC)
            .unwrap()
            .is_none());
        assert!(settings_repository::get_setting(&conn, KEY_API_KEY_LEGACY)
            .unwrap()
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn keyfile_is_created_owner_only_and_reused() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("wikiya-secret-{}", uuid::Uuid::new_v4()));
        let path = dir.join(KEY_FILE_NAME);

        let created = SecretCipher::load_or_create(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "主密钥文件必须仅所有者可读写");

        // 第二次加载必须复用同一把密钥（否则旧密文将无法解密）。
        let reloaded = SecretCipher::load_or_create(&path).unwrap();
        let token = created.encrypt("sk-persist").unwrap();
        assert_eq!(reloaded.decrypt(&token).unwrap(), "sk-persist");

        fs::remove_dir_all(&dir).ok();
    }
}
