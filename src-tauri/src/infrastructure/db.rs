//! SQLite 连接、迁移与行映射工具。
//!
//! 这里是唯一知道"数据库长什么样"的地方（TDD §86）。

use std::path::Path;

use rusqlite::Connection;

use crate::domain::ontology::registry;
use crate::error::{AppError, AppResult};

/// 当前 schema 版本。新增迁移文件时 +1。
pub const SCHEMA_VERSION: i64 = 13;

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

/// 0006 统一 Run 注册表（M1：Run → Skill → Tool → Result → Trace）。
const MIGRATION_0006: &str = include_str!("../../migrations/0006_unified_runs.sql");

/// 0007 Skill 标准化（M3：skills / skill_versions）。
const MIGRATION_0007: &str = include_str!("../../migrations/0007_skills.sql");

/// 0008 Agent Profile（M4：Agent 配置化）。
const MIGRATION_0008: &str = include_str!("../../migrations/0008_agent_profiles.sql");

/// 0009 候选知识持久化（M6：Candidate 一产生就持久化）。
const MIGRATION_0009: &str = include_str!("../../migrations/0009_candidates.sql");

/// 0010 Skill 自定义（M11：is_builtin 标记）。
const MIGRATION_0010: &str = include_str!("../../migrations/0010_custom_skills.sql");

/// 0011 候选 grounding 支持度（PR-03：support_level）。
const MIGRATION_0011: &str = include_str!("../../migrations/0011_candidate_support.sql");

/// 0012 Run 级 token 账本（PR-07：usage_json）。
const MIGRATION_0012: &str = include_str!("../../migrations/0012_run_usage.sql");

/// 0013 按真实查询补索引（PERF-04）。
const MIGRATION_0013: &str = include_str!("../../migrations/0013_perf_indexes.sql");

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
    (6, MIGRATION_0006),
    (7, MIGRATION_0007),
    (8, MIGRATION_0008),
    (9, MIGRATION_0009),
    (10, MIGRATION_0010),
    (11, MIGRATION_0011),
    (12, MIGRATION_0012),
    (13, MIGRATION_0013),
];

/// 打开连接并设置**连接级** PRAGMA。
///
/// 每次操作都开一条新连接（本地单用户 + WAL 下比共享 mutex 更快）。
/// 这里只设置**连接级**参数；`journal_mode` 是**数据库级**持久设置，
/// 由 [`initialize`] 一次性写入（见该函数与 PERF-05 说明）。
pub fn open(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(conn)
}

/// 把数据库切到 WAL 模式（**一次性**，仅在 [`initialize`] 里调用）。
///
/// WAL 是**数据库级**属性：设置后会写进库文件头，之后打开的每条连接
/// 自动继承。原先放在 [`open`] 里意味着**每个 IPC** 都要重发一次
/// `PRAGMA journal_mode = WAL`——而这条 PRAGMA 并非纯读：它要取库锁、
/// 判断是否需恢复/checkpoint，是实打实的开销。改为启动时设一次即可。
fn enable_wal(conn: &Connection) -> AppResult<()> {
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        // 诚实失败：拿不到 WAL 就说明并发读会被写阻塞，不能静默继续。
        return Err(crate::error::AppError::Internal(format!(
            "无法启用 WAL（当前 journal_mode = {mode}）"
        )));
    }
    Ok(())
}

/// 建库、应用迁移、一次性设置 WAL（每次启动调用；迁移按版本增量执行）。
pub fn initialize(path: &Path) -> AppResult<()> {
    // 先自检注册表：如果词表与代码不一致，产出的知识会带着非法谓语落库，
    // 事后无法自动修复。宁可此时启动失败。
    registry::self_check()?;

    let mut conn = open(path)?;
    // 先切 WAL 再迁移：迁移本身也受益于 WAL 的读写不互斥。
    enable_wal(&conn)?;
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
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
    })
}

/// 读取一个 `[String]` JSON 列。
pub fn string_list_col(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
    let raw: String = row.get(index)?;
    serde_json::from_str(&raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
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
        rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, Box::new(err))
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
        let result = conn.query_row("SELECT types_json FROM entities WHERE id='e1'", [], |row| {
            string_list_col(row, 0)
        });
        assert!(result.is_err(), "损坏的 JSON 必须报错而不是返回空集");
    }
}

#[cfg(test)]
mod wal_tests {
    use super::*;
    use std::path::PathBuf;

    /// 自包含的临时库路径（`mod tests` 里的同类 helper 是兄弟模块私有项）。
    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("wiki-ya-wal-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{tag}-{}.db", uuid::Uuid::new_v4()))
    }

    fn cleanup(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.to_path_buf();
            if !suffix.is_empty() {
                p.set_extension(format!("db{suffix}"));
            }
            std::fs::remove_file(p).ok();
        }
    }

    fn journal_mode(path: &Path) -> String {
        let conn = open(path).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        mode
    }

    /// PERF-03：WAL 只需在 initialize 设一次——它持久化在库文件头里，
    /// 之后任何连接都自动继承。这正是把它从 `open()` 移出的依据。
    #[test]
    fn wal_is_persisted_and_inherited_by_later_connections() {
        let path = temp_path("init");
        initialize(&path).expect("初始化应成功");

        assert_eq!(
            journal_mode(&path).to_lowercase(),
            "wal",
            "initialize 之后应为 WAL"
        );

        // 关键：这条连接**没有**再发 journal_mode PRAGMA（open() 已不再设置），
        // 但它依然运行在 WAL 模式——证明该设置是数据库级持久的。
        assert_eq!(
            journal_mode(&path).to_lowercase(),
            "wal",
            "后续连接应自动继承 WAL，无需重发 PRAGMA"
        );

        // 重开一次初始化也不应有任何问题（幂等）。
        initialize(&path).expect("重复初始化应幂等");
        assert_eq!(journal_mode(&path).to_lowercase(), "wal");

        cleanup(&path);
    }

    /// 连接级 PRAGMA 仍然每次生效（它们确实是连接级的）。
    #[test]
    fn connection_level_pragmas_still_apply() {
        let path = temp_path("conn");
        initialize(&path).unwrap();
        let conn = open(&path).unwrap();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        let busy: i64 = conn
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1, "外键约束必须在每条连接上开启");
        assert_eq!(busy, 5000, "busy_timeout 必须是连接级设置");
        cleanup(&path);
    }
}
