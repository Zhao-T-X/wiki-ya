//! SQLite 连接、迁移与行映射工具。
//!
//! 这里是唯一知道"数据库长什么样"的地方（TDD §86）。

use std::path::Path;

use rusqlite::Connection;

use crate::domain::ontology::registry;
use crate::error::{AppError, AppResult};

/// 当前 schema 版本。新增迁移文件时 +1。
pub const SCHEMA_VERSION: i64 = 3;

/// 0001 初始 Schema。
///
/// 用 `include_str!` 编译期内嵌：打包后的应用不依赖运行期文件路径，
/// 也不会因为用户移动了目录而迁移失败。
const MIGRATION_0001: &str = include_str!("../../migrations/0001_init.sql");

/// 0002 嵌入表（语义检索向量）。
const MIGRATION_0002: &str = include_str!("../../migrations/0002_embeddings.sql");

/// 0003 简单键值设置（AI 配置化）。
const MIGRATION_0003: &str = include_str!("../../migrations/0003_settings.sql");

/// 打开连接并设置全部 PRAGMA。
///
/// 每次操作都开一条新连接（本地单用户 + WAL 下比共享 mutex 更快）。
/// 因此**所有** PRAGMA 必须在这里设置——它是唯一的连接入口。
pub fn open(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(conn)
}

/// 建表并应用迁移（幂等，每次启动都执行）。
pub fn initialize(path: &Path) -> AppResult<()> {
    // 先自检注册表：如果词表与代码不一致，产出的知识会带着非法谓语落库，
    // 事后无法自动修复。宁可此时启动失败。
    registry::self_check()?;

    let mut conn = open(path)?;
    let transaction = conn.transaction()?;
    transaction.execute_batch(MIGRATION_0001)?;
    transaction.execute_batch(MIGRATION_0002)?;
    transaction.execute_batch(MIGRATION_0003)?;
    transaction.execute(
        "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)",
        [SCHEMA_VERSION],
    )?;
    transaction.commit()?;
    Ok(())
}

/// 当前实际应用的 schema 版本。
pub fn schema_version(conn: &Connection) -> AppResult<i64> {
    let version: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .or_else(|err| match err {
            // 空表时 MAX 返回 NULL，get::<i64> 会失败——这在语义上是"没有版本"。
            rusqlite::Error::InvalidColumnType(..) => Ok(None),
            other => Err(other),
        })?;
    Ok(version.unwrap_or(0))
}

/// 数据库侧的当前时间（UTC，`YYYY-MM-DD HH:MM:SS`）。
///
/// 刻意用数据库时间而不是 Rust 的 `Utc::now()`：写入与比较都来自同一个时钟，
/// 不会因为进程与数据库时区解释差异而产生"刚刚写入的记录看起来在未来"。
pub fn now(conn: &Connection) -> AppResult<String> {
    conn.query_row("SELECT datetime('now')", [], |row| row.get(0))
        .map_err(AppError::from)
}

/// 读取一个 JSON 列。
///
/// 解析失败会返回错误而不是悄悄用 `{}` 顶替：损坏的 JSON 列意味着
/// 数据出了问题，静默降级会让问题在很久以后才浮现。
pub fn json_col(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<serde_json::Value> {
    let raw: String = row.get(index)?;
    serde_json::from_str(&raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

/// 读取一个 `[String]` JSON 列。
pub fn string_list_col(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    let raw: String = row.get(index)?;
    serde_json::from_str(&raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

/// 把 SQLite 的 REAL 读成 `Option<f32>`。
pub fn opt_f32_col(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<f32>> {
    let value: Option<f64> = row.get(index)?;
    Ok(value.map(|v| v as f32))
}

/// 读取一个"受控词表"列（枚举或 ID）。
///
/// 枚举的 `FromStr` 拒绝越界取值，因此数据库里出现非法值时**读取就会失败**，
/// 而不是被静默变成某个默认枚举（INV-12）。
pub fn parse_col<T>(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<T>
where
    T: std::str::FromStr<Err = AppError>,
{
    let raw: String = row.get(index)?;
    raw.parse::<T>().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

/// 统计一张表的行数。
pub fn count_rows(conn: &Connection, table: &str) -> AppResult<i64> {
    // 表名不是用户输入（全部来自调用点的字面量），因此这里不存在注入面；
    // 用白名单再确认一次，免得未来有人把变量传进来。
    const ALLOWED: &[&str] = &[
        "documents",
        "chunks",
        "entities",
        "claims",
        "evidence",
        "relations",
    ];
    if !ALLOWED.contains(&table) {
        return Err(AppError::Internal(format!(
            "count_rows 不接受表名 {table:?}（不在白名单内）"
        )));
    }
    let sql = format!("SELECT COUNT(*) FROM {table}");
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(AppError::from)
}

/// 测试支撑：各 Repository 的单元测试共用一个内存库，
/// 保证它们面对的是与生产完全相同的 DDL（含触发器与 CHECK）。
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 建一个内存库并应用迁移，供各 Repository 的测试复用。
    ///
    /// 注意 `include_str!` 的 SQL 在内存库上同样有效：FTS5 虚拟表与触发器
    /// 都不依赖文件系统。
    pub fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = MEMORY;",
        )
        .expect("pragmas");
        conn.execute_batch(MIGRATION_0001).expect("apply migration 0001");
        conn.execute_batch(MIGRATION_0002).expect("apply migration 0002");
        conn.execute_batch(MIGRATION_0003).expect("apply migration 0003");
        conn
    }

    #[test]
    fn migration_applies_and_is_idempotent() {
        let conn = memory_db();
        // 再执行一次必须无错（应用每次启动都会重复执行）
        conn.execute_batch(MIGRATION_0001).expect("re-apply");
    }

    #[test]
    fn schema_version_is_recorded() {
        let conn = memory_db();
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)",
            [SCHEMA_VERSION],
        )
        .unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn check_constraints_reject_unknown_enum_values() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES('d1','t','c','h1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entities(id,name,primary_type) VALUES('e1','Rust','Software')",
            [],
        )
        .unwrap();

        let bad_status = conn.execute(
            "INSERT INTO claims(id,subject_id,predicate,status) VALUES('c1','e1','uses','nonsense')",
            [],
        );
        assert!(bad_status.is_err(), "非法 status 必须被 CHECK 拒绝");
    }

    #[test]
    fn content_hash_uniqueness_enforces_idempotent_import() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES('d1','t','c','same')",
            [],
        )
        .unwrap();
        let duplicate = conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES('d2','t2','c2','same')",
            [],
        );
        assert!(duplicate.is_err(), "重复 content_hash 必须冲突");
    }

    #[test]
    fn trigram_tokenizer_makes_chinese_searchable() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES('d1','苹果的SEO是乔布斯','正文','h')",
            [],
        )
        .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM documents_fts WHERE documents_fts MATCH '乔布斯'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1, "trigram 分词下中文子串必须可搜（需 ≥3 字查询）");
    }

    #[test]
    fn entity_name_is_unique_case_insensitively() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO entities(id,name,primary_type) VALUES('e1','OpenAI','Organization')",
            [],
        )
        .unwrap();
        let duplicate = conn.execute(
            "INSERT INTO entities(id,name,primary_type) VALUES('e2','openai','Organization')",
            [],
        );
        assert!(duplicate.is_err(), "大小写不同的同名实体必须冲突（INV-03）");
    }

    #[test]
    fn json_helpers_report_corruption_instead_of_defaulting() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO entities(id,name,primary_type,types_json) VALUES('e1','X','Concept','{broken')",
            [],
        )
        .unwrap();
        let result = conn.query_row(
            "SELECT types_json FROM entities WHERE id='e1'",
            [],
            |row| string_list_col(row, 0),
        );
        assert!(result.is_err(), "损坏的 JSON 必须报错而不是返回空集");
    }
}
