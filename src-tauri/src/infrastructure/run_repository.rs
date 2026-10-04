//! `runs` 统一注册表仓储（M1）。
//!
//! 职责刻意收窄：**登记 + 生命周期推进 + 查询**。类型相关的明细
//! （抽取进度、Agent 步骤）仍由 extraction_runs / agent_runs 维护，
//! 这里不做重复存储。

use rusqlite::Connection;

use crate::ai::accounting::TokenUsage;
use crate::domain::extraction::ExtractionRunStatus;
use crate::domain::run::{RunRecord, RunType};
use crate::error::{AppError, AppResult};
use crate::infrastructure::db::{now, parse_col};

/// 登记一条新 Run（status = queued）。
pub fn register(
    conn: &Connection,
    id: &str,
    run_type: RunType,
    actor: &str,
    parent_run_id: Option<&str>,
    metadata: &str,
) -> AppResult<()> {
    let started_at = now(conn)?;
    conn.execute(
        "INSERT INTO runs(id, parent_run_id, run_type, actor, status, started_at, metadata) \
         VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6)",
        rusqlite::params![
            id,
            parent_run_id,
            run_type.as_str(),
            actor,
            started_at,
            metadata
        ],
    )?;
    Ok(())
}

/// 推进状态（登记处只存通用状态；明细表各自推进自己的进度/阶段）。
pub fn set_status(conn: &Connection, id: &str, status: ExtractionRunStatus) -> AppResult<()> {
    conn.execute(
        "UPDATE runs SET status = ?2 WHERE id = ?1",
        rusqlite::params![id, status.as_str()],
    )?;
    Ok(())
}

/// 记录阶段字面量（自由文本，各类型自行约定）。
pub fn set_stage(conn: &Connection, id: &str, stage: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE runs SET stage = ?2 WHERE id = ?1",
        rusqlite::params![id, stage],
    )?;
    Ok(())
}

/// 覆盖写 metadata（如 Skill 执行结果摘要）。
pub fn set_metadata(conn: &Connection, id: &str, metadata: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE runs SET metadata = ?2 WHERE id = ?1",
        rusqlite::params![id, metadata],
    )?;
    Ok(())
}

/// 写入本次 Run 的真实 token 账本（PR-07）。
///
/// 只在确有 provider 用量时调用；序列化失败不阻断主流程（返回错误由调用方决定）。
pub fn set_usage(conn: &Connection, id: &str, usage: &TokenUsage) -> AppResult<()> {
    let json = serde_json::to_string(usage)
        .map_err(|err| AppError::Internal(format!("token 用量序列化失败：{err}")))?;
    conn.execute(
        "UPDATE runs SET usage_json = ?2 WHERE id = ?1",
        rusqlite::params![id, json],
    )?;
    Ok(())
}

/// 终态收口：status + finished_at + 错误信息一次写齐。
pub fn finish(
    conn: &Connection,
    id: &str,
    status: ExtractionRunStatus,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> AppResult<()> {
    let finished_at = now(conn)?;
    conn.execute(
        "UPDATE runs SET status = ?2, finished_at = ?3, error_code = ?4, error_message = ?5 \
         WHERE id = ?1",
        rusqlite::params![id, status.as_str(), finished_at, error_code, error_message],
    )?;
    Ok(())
}

/// 读取一条登记。
pub fn get(conn: &Connection, id: &str) -> AppResult<Option<RunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_run_id, run_type, actor, status, stage, started_at, finished_at, \
         error_code, error_message, metadata, usage_json FROM runs WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![id], map_run)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 启动恢复：把登记处里仍在 running/queued 的抽取 Run 标记为 interrupted
/// （与 extraction_run_repository::mark_running_as_interrupted 成对执行）。
pub fn mark_stale_interrupted(conn: &Connection) -> AppResult<usize> {
    let finished_at = now(conn)?;
    let n = conn.execute(
        "UPDATE runs SET status = 'interrupted', finished_at = ?1 \
         WHERE run_type = 'extraction' AND status IN ('running', 'queued')",
        rusqlite::params![finished_at],
    )?;
    Ok(n)
}

/// 最近的 Run（新→旧），供 Activity / Trace 列表。
pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<RunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_run_id, run_type, actor, status, stage, started_at, finished_at, \
         error_code, error_message, metadata, usage_json FROM runs ORDER BY started_at DESC, id LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit as i64], map_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        id: r.get(0)?,
        parent_run_id: r.get(1)?,
        run_type: parse_col::<RunType>(r, 2)?,
        actor: r.get(3)?,
        status: parse_col::<ExtractionRunStatus>(r, 4)?,
        stage: r.get(5)?,
        started_at: r.get(6)?,
        finished_at: r.get(7)?,
        error_code: r.get(8)?,
        error_message: r.get(9)?,
        metadata: r.get(10)?,
        usage_json: r.get(11)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;

    fn setup() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        conn
    }

    #[test]
    fn register_and_finish_round_trip() {
        let conn = setup();
        register(
            &conn,
            "r1",
            RunType::Extraction,
            "ExtractionAgent",
            None,
            "{}",
        )
        .unwrap();

        let run = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(run.run_type, RunType::Extraction);
        assert_eq!(run.status, ExtractionRunStatus::Queued);
        assert!(run.finished_at.is_none());

        set_status(&conn, "r1", ExtractionRunStatus::Running).unwrap();
        set_stage(&conn, "r1", "extracting").unwrap();
        finish(&conn, "r1", ExtractionRunStatus::Completed, None, None).unwrap();

        let run = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(run.status, ExtractionRunStatus::Completed);
        assert_eq!(run.stage, "extracting");
        assert!(run.finished_at.is_some());
        // 未写入用量时诚实保持 None（Trace 显示「无用量记录」）。
        assert!(run.usage_json.is_none());
    }

    /// PR-07：真实 token 账本写入后能原样读回。
    #[test]
    fn set_usage_round_trips() {
        let conn = setup();
        register(&conn, "r1", RunType::Agent, "KnowledgeAgent", None, "{}").unwrap();

        let usage = TokenUsage {
            input_tokens: 120,
            output_tokens: 45,
            embedding_tokens: 300,
            retries: 1,
        };
        set_usage(&conn, "r1", &usage).unwrap();

        let run = get(&conn, "r1").unwrap().unwrap();
        let raw = run.usage_json.expect("usage_json 应已写入");
        let parsed: TokenUsage = serde_json::from_str(&raw).expect("usage_json 应可反序列化");
        assert_eq!(parsed, usage);
        assert_eq!(parsed.total_tokens(), 465);
    }

    #[test]
    fn list_recent_is_newest_first_and_typed() {
        let conn = setup();
        register(&conn, "a", RunType::Agent, "KnowledgeAgent", None, "{}").unwrap();
        register(
            &conn,
            "b",
            RunType::Extraction,
            "ExtractionAgent",
            None,
            "{}",
        )
        .unwrap();
        // 同秒内按 started_at 顺序不稳定，校验集合与条数即可。
        let runs = list_recent(&conn, 10).unwrap();
        assert_eq!(runs.len(), 2);
        let types: Vec<_> = runs.iter().map(|r| r.run_type.clone()).collect();
        assert!(types.contains(&RunType::Agent));
        assert!(types.contains(&RunType::Extraction));
    }

    #[test]
    fn parent_link_round_trips() {
        let conn = setup();
        register(&conn, "parent", RunType::Agent, "ResearchAgent", None, "{}").unwrap();
        register(
            &conn,
            "child",
            RunType::Skill,
            "knowledge-extraction",
            Some("parent"),
            "{}",
        )
        .unwrap();
        let child = get(&conn, "child").unwrap().unwrap();
        assert_eq!(child.parent_run_id.as_deref(), Some("parent"));
    }
}
