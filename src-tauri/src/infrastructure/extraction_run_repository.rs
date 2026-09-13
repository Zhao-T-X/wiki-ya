//! Extraction Run 仓储（EXTRACTION-001）。
//!
//! 唯一知道 `extraction_runs` 表长什么样的模块。所有更新都是"修补式"的
//! （按 run_id 改某一列），便于后台任务在阶段推进时逐步写入，也便于
//! 前端轮询 / 事件重放时拿到一致快照。

use rusqlite::{params, Connection};

use crate::domain::extraction::{ExtractionRun, ExtractionRunStatus, ExtractionStage};
use crate::error::AppResult;
use crate::infrastructure::db::{now, parse_col};

/// 创建一条 Run（初始 `queued` / `preparing`）。
pub fn create(conn: &Connection, run: &ExtractionRun) -> AppResult<()> {
    conn.execute(
        "INSERT INTO extraction_runs(
            id, document_id, status, stage, total_chunks, processed_chunks,
            candidates_found, changes_found, result_json, started_at,
            finished_at, error_code, error_message
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            run.id,
            run.document_id,
            run.status,
            run.stage,
            run.total_chunks,
            run.processed_chunks,
            run.candidates_found,
            run.changes_found,
            run.result_json,
            run.started_at,
            run.finished_at,
            run.error_code,
            run.error_message,
        ],
    )?;
    Ok(())
}

/// 读取一条 Run。
pub fn get(conn: &Connection, id: &str) -> AppResult<ExtractionRun> {
    let row = conn.query_row(
        "SELECT id, document_id, status, stage, total_chunks, processed_chunks,
                candidates_found, changes_found, result_json, started_at,
                finished_at, error_code, error_message
         FROM extraction_runs WHERE id = ?1",
        params![id],
        |r| map_run(r),
    )?;
    Ok(row)
}

/// 最近的 Run（新→旧）。
pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<ExtractionRun>> {
    let mut statement = conn.prepare(
        "SELECT id, document_id, status, stage, total_chunks, processed_chunks,
                candidates_found, changes_found, result_json, started_at,
                finished_at, error_code, error_message
         FROM extraction_runs ORDER BY started_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = statement.query_map(params![limit as i64], |r| map_run(r))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// 设置状态（阶段不变）。
pub fn set_status(conn: &Connection, id: &str, status: ExtractionRunStatus) -> AppResult<()> {
    conn.execute(
        "UPDATE extraction_runs SET status = ?2 WHERE id = ?1",
        params![id, status],
    )?;
    Ok(())
}

/// 设置阶段（状态不变）。
pub fn set_stage(conn: &Connection, id: &str, stage: ExtractionStage) -> AppResult<()> {
    conn.execute(
        "UPDATE extraction_runs SET stage = ?2 WHERE id = ?1",
        params![id, stage],
    )?;
    Ok(())
}

/// 更新已处理 / 总块数（Extracting 阶段逐批推进）。
pub fn set_progress(
    conn: &Connection,
    id: &str,
    processed: i64,
    total: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE extraction_runs SET processed_chunks = ?2, total_chunks = ?3 WHERE id = ?1",
        params![id, processed, total],
    )?;
    Ok(())
}

/// 写入候选数 / 变更数（Validating / Comparing 后）。
pub fn set_counts(
    conn: &Connection,
    id: &str,
    candidates: i64,
    changes: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE extraction_runs SET candidates_found = ?2, changes_found = ?3 WHERE id = ?1",
        params![id, candidates, changes],
    )?;
    Ok(())
}

/// 终态收尾：写状态、结果、错误、finished_at（一次完成）。
pub fn finish(
    conn: &Connection,
    id: &str,
    status: ExtractionRunStatus,
    result_json: Option<&str>,
    error_code: Option<&str>,
    error_message: Option<&str>,
) -> AppResult<()> {
    let finished = now(conn)?;
    conn.execute(
        "UPDATE extraction_runs
         SET status = ?2, result_json = ?3, error_code = ?4, error_message = ?5, finished_at = ?6
         WHERE id = ?1",
        params![id, status, result_json, error_code, error_message, finished],
    )?;
    Ok(())
}

/// 启动时把"上次还在跑"的 Run 标记为 interrupted（不假装还在运行）。
pub fn mark_running_as_interrupted(conn: &Connection) -> AppResult<usize> {
    let affected = conn.execute(
        "UPDATE extraction_runs SET status = 'interrupted', finished_at = datetime('now')
         WHERE status IN ('queued','running')",
        [],
    )?;
    Ok(affected)
}

fn map_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<ExtractionRun> {
    Ok(ExtractionRun {
        id: r.get(0)?,
        document_id: r.get(1)?,
        status: parse_col::<ExtractionRunStatus>(r, 2)?,
        stage: parse_col::<ExtractionStage>(r, 3)?,
        total_chunks: r.get(4)?,
        processed_chunks: r.get(5)?,
        candidates_found: r.get(6)?,
        changes_found: r.get(7)?,
        result_json: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
        error_code: r.get(11)?,
        error_message: r.get(12)?,
    })
}
