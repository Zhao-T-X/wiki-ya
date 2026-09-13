//! 运行遥测与上下文缓存（Phase 5/6 收尾，TDD §79/§85、INV-20/21）。
//!
//! 这些都是**派生数据**：只为审计、回放与缓存，清空重建不损失知识本身。
//! 任何 Service 都不直接拼这些 SQL。

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::error::AppResult;

// ---------------------------------------------------------------------------
// Context Runs（TDD §79：预算 / 实际 / 裁剪可审计）
// ---------------------------------------------------------------------------

/// 记录一次上下文编译。返回内部记录 id（供 `context_sections` 关联）。
#[allow(clippy::too_many_arguments)]
pub fn insert_context_run(
    conn: &Connection,
    run_id: &str,
    agent_name: &str,
    budget_tokens: i64,
    actual_tokens: i64,
    trimmed_tokens: i64,
    over_budget: bool,
) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let efficiency = if budget_tokens > 0 {
        actual_tokens as f64 / budget_tokens as f64
    } else {
        0.0
    };
    conn.execute(
        "INSERT INTO context_runs(id, run_id, agent_name, budget_tokens, actual_tokens, \
         trimmed_tokens, efficiency, over_budget, optimizations_json, withheld_json) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'[]','[]')",
        params![
            id,
            run_id,
            agent_name,
            budget_tokens,
            actual_tokens,
            trimmed_tokens,
            efficiency,
            over_budget as i64
        ],
    )?;
    Ok(id)
}

/// 记录编译结果的一个片段（policy 四值与 §6.3 逐字一致，大写）。
#[allow(clippy::too_many_arguments)]
pub fn insert_context_section(
    conn: &Connection,
    context_run_id: &str,
    section_index: i64,
    name: &str,
    policy: &str,
    source: Option<&str>,
    tokens: i64,
    chars: i64,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO context_sections(id, context_run_id, section_index, name, policy, source, \
         tokens, chars, trimmed) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0)",
        params![
            uuid::Uuid::new_v4().to_string(),
            context_run_id,
            section_index,
            name,
            policy,
            source,
            tokens,
            chars
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Agent Runs / Events（Run Trace，可回放）
// ---------------------------------------------------------------------------

/// 开启一次 Agent 运行记录（status = started）。
pub fn start_agent_run(
    conn: &Connection,
    task_type: &str,
    agent_role: Option<&str>,
    model: Option<&str>,
) -> AppResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO agent_runs(id, task_type, agent_role, model, status) \
         VALUES (?1,?2,?3,?4,'started')",
        params![id, task_type, agent_role, model],
    )?;
    Ok(id)
}

/// 结束一次 Agent 运行（success / failed，含耗时与摘要）。
pub fn finish_agent_run(
    conn: &Connection,
    id: &str,
    status: &str,
    step_count: i64,
    summary: &Value,
    error_message: Option<&str>,
    duration_ms: i64,
) -> AppResult<()> {
    conn.execute(
        "UPDATE agent_runs SET status=?2, step_count=?3, summary_json=?4, error_message=?5, \
         duration_ms=?6, finished_at=datetime('now') WHERE id=?1",
        params![
            id,
            status,
            step_count,
            summary.to_string(),
            error_message,
            duration_ms
        ],
    )?;
    Ok(())
}

/// 记录 Agent 运行中的一步（工具调用）。
#[allow(clippy::too_many_arguments)]
pub fn insert_agent_event(
    conn: &Connection,
    run_id: &str,
    step_index: i64,
    name: &str,
    status: &str,
    input_summary: Option<&str>,
    output_text: Option<&str>,
    error_message: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO agent_events(id, run_id, step_index, name, status, input_summary, \
         output_text, error_message) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            uuid::Uuid::new_v4().to_string(),
            run_id,
            step_index,
            name,
            status,
            input_summary,
            output_text,
            error_message
        ],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Context Cache（INV-20：组合哈希 key，版本变化即失效；INV-21：无需手工清理）
// ---------------------------------------------------------------------------

/// 命中则返回缓存值并把 hits/last_hit_at 一并更新；未命中返回 `None`。
pub fn cache_get(conn: &Connection, cache_key: &str) -> AppResult<Option<Value>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT prompt_json FROM context_cache WHERE cache_key = ?1",
            params![cache_key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if raw.is_some() {
        conn.execute(
            "UPDATE context_cache SET hits = hits + 1, last_hit_at = datetime('now') \
             WHERE cache_key = ?1",
            params![cache_key],
        )?;
    }
    Ok(raw.and_then(|text| serde_json::from_str(&text).ok()))
}

/// 写入（或覆盖）缓存条目；hits 计数保留（`ON CONFLICT DO UPDATE`）。
pub fn cache_put(
    conn: &Connection,
    cache_key: &str,
    agent: &str,
    prompt: &Value,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO context_cache(cache_key, agent, prompt_json) VALUES (?1,?2,?3) \
         ON CONFLICT(cache_key) DO UPDATE SET prompt_json = excluded.prompt_json",
        params![cache_key, agent, prompt.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    #[test]
    fn agent_run_round_trips_through_start_and_finish() {
        let conn = memory_db();
        let id = start_agent_run(&conn, "agent", Some("ResearchAgent"), Some("gpt-4o-mini")).unwrap();
        finish_agent_run(
            &conn,
            &id,
            "success",
            3,
            &serde_json::json!({ "answer_chars": 42 }),
            None,
            1200,
        )
        .unwrap();

        let (status, steps): (String, i64) = conn
            .query_row(
                "SELECT status, step_count FROM agent_runs WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "success");
        assert_eq!(steps, 3);
    }

    #[test]
    fn cache_round_trips_and_counts_hits() {
        let conn = memory_db();
        assert!(cache_get(&conn, "k1").unwrap().is_none());

        cache_put(&conn, "k1", "knowledge", &serde_json::json!({ "a": 1 })).unwrap();
        assert_eq!(cache_get(&conn, "k1").unwrap().unwrap()["a"], 1);

        // 覆盖写保留行；hits 累加。
        cache_put(&conn, "k1", "knowledge", &serde_json::json!({ "a": 2 })).unwrap();
        let _: Option<Value> = cache_get(&conn, "k1").unwrap();
        let hits: i64 = conn
            .query_row("SELECT hits FROM context_cache WHERE cache_key = ?1", ["k1"], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(hits >= 1);
    }

    #[test]
    fn context_run_records_budget_and_efficiency() {
        let conn = memory_db();
        let id = insert_context_run(&conn, "run-1", "knowledge", 1000, 400, 0, false).unwrap();
        insert_context_section(&conn, &id, 0, "t", "LOAD", Some("c1"), 400, 1000).unwrap();
        let (eff, over): (f64, i64) = conn
            .query_row(
                "SELECT efficiency, over_budget FROM context_runs WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!((eff - 0.4).abs() < 1e-9);
        assert_eq!(over, 0);
    }
}
