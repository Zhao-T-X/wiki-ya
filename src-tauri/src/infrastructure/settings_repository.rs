//! 应用设置的键值存储（Phase 6，AI 配置化）。
//!
//! 极简 KV：`settings(key PK, value, updated_at)`。只承载「用户可改、需持久化」
//! 的少量配置（当前为 AI 运行时）。复杂领域状态不在这里。
//! 读取端负责决定默认值与优先级（见 `ai::config::AiConfig::from_settings`）。

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;

/// 读取单个设置；不存在时返回 `None`（不报错）。
pub fn get_setting(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value)
}

/// 写入（或覆盖）单个设置。空值也会被如实写入（表示「显式清除」）。
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        params![key, value],
    )?;
    Ok(())
}

/// 删除单个设置（不存在时为空操作）。
///
/// 用于 SEC-002：把明文 API Key 迁移到安全存储后，必须**物理删除**这一行，
/// 而不是仅写空串。
pub fn delete_setting(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    #[test]
    fn setting_round_trips_and_overwrites() {
        let conn = memory_db();
        assert!(get_setting(&conn, "ai.model").unwrap().is_none());

        set_setting(&conn, "ai.model", "gpt-4o").unwrap();
        assert_eq!(get_setting(&conn, "ai.model").unwrap().unwrap(), "gpt-4o");

        // 覆盖语义：后写胜出。
        set_setting(&conn, "ai.model", "gpt-4o-mini").unwrap();
        assert_eq!(
            get_setting(&conn, "ai.model").unwrap().unwrap(),
            "gpt-4o-mini"
        );
    }
}
