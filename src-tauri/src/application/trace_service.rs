//! Run Trace 查询（M1：Run → Skill → Tool → Result → Trace）。
//!
//! 把一次 Run 的「统一登记 + 类型相关明细」聚合为一个视图：
//! - `agent` → `agent_runs` 的工具调用步骤（agent_events）；
//! - `extraction` → `extraction_runs` 的抽取明细快照；
//! - review / skill 的明细随对应阶段落地后接入（DTO 已预留扩展位）。
//!
//! 这是 M8 Trace UI 的数据源；先以命令形式暴露，前端随时可画时间线。

use rusqlite::Connection;

use crate::application::dto::{AgentEventDto, RunTraceDto};
use crate::domain::run::RunType;
use crate::error::{AppError, AppResult};
use crate::application::extraction_service;
use crate::infrastructure::{extraction_run_repository, run_repository};

/// 读取一条 Run 的完整 Trace。不存在时返回 NotFound。
pub fn get_trace(conn: &Connection, run_id: &str) -> AppResult<RunTraceDto> {
    let run = run_repository::get(conn, run_id)?
        .ok_or_else(|| AppError::NotFound(format!("Run {run_id} 不存在")))?;

    let agent_steps = if run.run_type == RunType::Agent {
        agent_steps(conn, run_id)?
    } else {
        Vec::new()
    };

    let extraction_run = if run.run_type == RunType::Extraction {
        // 该 Run 一定有明细（同一 id 两处登记）。
        Some(extraction_service::to_dto(&extraction_run_repository::get(
            conn, run_id,
        )?))
    } else {
        None
    };

    Ok(RunTraceDto {
        id: run.id,
        parent_run_id: run.parent_run_id,
        run_type: run.run_type.as_str().to_string(),
        actor: run.actor,
        status: run.status.as_str().to_string(),
        stage: run.stage,
        started_at: run.started_at,
        finished_at: run.finished_at,
        error_code: run.error_code,
        error_message: run.error_message,
        metadata: serde_json::from_str(&run.metadata).unwrap_or(serde_json::Value::Null),
        agent_steps,
        extraction_run,
    })
}

/// 读取一次 Agent Run 的步骤明细（按 step_index 升序）。
fn agent_steps(conn: &Connection, run_id: &str) -> AppResult<Vec<AgentEventDto>> {
    let mut stmt = conn.prepare(
        "SELECT step_index, name, status, input_summary, output_text, error_message, created_at \
         FROM agent_events WHERE run_id = ?1 ORDER BY step_index",
    )?;
    let rows = stmt.query_map(rusqlite::params![run_id], |r| {
        Ok(AgentEventDto {
            step_index: r.get(0)?,
            name: r.get(1)?,
            status: r.get(2)?,
            input_summary: r.get(3)?,
            output_text: r.get(4)?,
            error_message: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
