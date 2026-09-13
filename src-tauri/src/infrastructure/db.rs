//! SQLite 连接、迁移与行映射工具。
//!
//! 这里是唯一知道"数据库长什么样"的地方（TDD §86）。

use std::path::Path;

use rusqlite::Connection;

use crate::domain::ontology::registry;
use crate::error::{AppError, AppResult};

/// 当前 schema 版本。新增迁移文件时 +1。
pub const SCHEMA_VERSION: i64 = 5;

/// 0001 初始 Schema。
///
/// 用 `include_str!` 编译期内嵌：打包后的应用不依赖运行期文件路径，
/// 也不会因为用户移动了目录而迁移失败。
const MIGRATION_0001: &str = include_str!("../../migrations/0001_init.sql");

/// 0002 嵌入表（语义检索向量）。
const MIGRATION_0002: &str = include_str!("../../migrations/0002_embeddings.sql");

/// 0003 简单键值设置（AI 配置化）。
const MIGRATION_0003: &str = include_str!("../../migrations/0003_settings.sql");

/// 0004 Temporal 观察时间 + 演化事件日志（CORE-001 / CORE-005）。
const MIGRATION_0004: &str = include_str!("../../migrations/0004_evolution_events.sql");

/// 0005 Extraction Run（EXTRACTION-001：异步抽取后台任务）。
const MIGRATION_0005: &str = include_str!("../../migrations/0005_extraction_runs.sql");

/// 迁移清单 `(版本号, SQL)`：**必须按版本递增**。
///
/// DB-001：启动时只执行 `version > 当前版本` 的迁移，并逐条记录版本号，
/// 因此后续新增非幂等语句（ALTER / 种子 INSERT）时不会每次启动都重跑。
const MIGRATIONS: &[(i64, &str)] = &[
    (1, MIGRATION_0001),
    (2, MIGRATION_0002),
    (3, MIGRATION_0003),
    (4, MIGRATION_0004),
    (5, MIGRATION_0005),
];

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

/// 建表并应用迁移（每次启动都调用；内部按版本增量执行）。
pub fn initialize(path: &Path) -> AppResult<()> {
    // 先自检注册表：如果词表与代码不一致，产出的知识会带着非法谓语落库，
    // 事后无法自动修复。宁可此时启动失败。
    registry::self_check()?;

    let mut conn = open(path)?;
    apply_migrations(&mut conn)?;
    Ok(())
}

/// 按版本增量应用迁移（DB-001）。
///
/// 只执行 `version > current` 的迁移；每执行一条**立即**在同一事务内记录该
/// 版本号，保证「SQL 执行成功」与「版本已记录」二者一致（要么都成功，要么
/// 整体回滚）。返回应用后的最高版本。
pub fn apply_migrations(conn: &mut Connection) -> AppResult<i64> {
    let current = schema_version(conn)?;
    let tx = conn.transaction()?;
    let mut applied = current;
    for (version, sql) in MIGRATIONS {
        if *version > current {
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)",
                [version],
            )?;
            applied = *version;
        }
    }
    tx.commit()?;
    Ok(applied)
}

/// 当前实际应用的 schema 版本；尚未初始化时为 0。
pub fn schema_version(conn: &Connection) -> AppResult<i64> {
    let result: rusqlite::Result<Option<i64>> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        });
    match result {
        // 空表时 MAX 返回 NULL → None → 0（语义上「没有版本」）。
        Ok(version) => Ok(version.unwrap_or(0)),
        // 迁移表本身还不存在 = 尚未应用任何迁移。
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            Ok(0)
        }
        Err(other) => Err(other.into()),
    }
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
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = MEMORY;",
        )
        .expect("pragmas");
        apply_migrations(&mut conn).expect("apply migrations");
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
    fn fresh_db_reports_version_zero_before_migration() {
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), 0);
    }

    /// DB-001：文件库上完整初始化 → 每个版本都被记录；重复启动幂等。
    #[test]
    fn initialization_records_every_version_and_is_idempotent() {
        let path = temp_db_path();
        initialize(&path).unwrap();
        {
            let conn = open(&path).unwrap();
            assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
            for version in 1..=SCHEMA_VERSION {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
                        [version],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(count, 1, "版本 {version} 必须且只记录一次");
            }
        }
        // 二次启动：不应重复执行，版本不变。
        initialize(&path).unwrap();
        {
            let conn = open(&path).unwrap();
            assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        }
        cleanup_db_files(&path);
    }

    /// DB-001：v1 旧库升级到最新，只补执行缺失的迁移。
    #[test]
    fn partial_upgrade_only_applies_newer_migrations() {
        let path = temp_db_path();
        {
            let conn = open(&path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO schema_migrations(version) VALUES (1)",
                [],
            )
            .unwrap();
            assert_eq!(schema_version(&conn).unwrap(), 1);
        }
        {
            let mut conn = open(&path).unwrap();
            let applied = apply_migrations(&mut conn).unwrap();
            assert_eq!(applied, SCHEMA_VERSION);
            assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
            // 再跑一次：没有更高版本可应用，返回值即当前版本。
            assert_eq!(apply_migrations(&mut conn).unwrap(), SCHEMA_VERSION);
        }
        cleanup_db_files(&path);
    }

    fn temp_db_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("wikiya-mig-{}.db", uuid::Uuid::new_v4()))
    }

    fn cleanup_db_files(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_file_name(format!(
            "{}-wal",
            path.file_name().unwrap().to_string_lossy()
        )));
        let _ = std::fs::remove_file(path.with_file_name(format!(
            "{}-shm",
            path.file_name().unwrap().to_string_lossy()
        )));
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
