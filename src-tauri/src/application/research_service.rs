//! Research 多步研究（Phase 6，TDD §66）。
//!
//! 诚实边界：研究结果（Findings）**只进 Review 队列**，绝不直接落库为知识
//! （PRD「AI suggests, user decides」）。研究过程由 Agent 循环驱动
//! （`ai::runtime`），工具调用一律走白名单（`ai::tools`）。

use rusqlite::{params, Connection};
use serde_json::json;

use crate::ai::agents::AgentRole;
use crate::ai::provider::default_provider;
use crate::ai::runtime;
use crate::application::dto::{
    AgentStepDto, ResearchReport, ResearchTaskCard, StartResearchInput,
};
use crate::domain::review::review::ReviewTarget;
use crate::error::{AppError, AppResult};
use crate::events::EventSink;
use crate::infrastructure::{research_repository, review_repository};

/// 最近的研究任务历史（新→旧，Phase 6 收尾）。
pub fn list_tasks(conn: &Connection, limit: usize) -> AppResult<Vec<ResearchTaskCard>> {
    Ok(research_repository::list_tasks(conn, limit)?
        .into_iter()
        .map(|row| ResearchTaskCard {
            id: row.id,
            question: row.question,
            status: row.status,
            summary: row.summary,
            created_at: row.created_at,
        })
        .collect())
}

/// 启动一次多步研究：创建任务 → Agent 循环 → Findings 入 Review。
///
/// 失败路径同样诚实：任务被标记为 `failed` 并把错误写入 `findings`，
/// 然后把错误上抛给 UI 展示。
pub fn start_research(
    conn: &Connection,
    input: StartResearchInput,
    sink: Option<&EventSink>,
) -> AppResult<ResearchReport> {
    let question = input.question.trim().to_string();
    if question.is_empty() {
        return Err(AppError::Domain("研究问题不能为空".into()));
    }

    if !default_provider(conn).enabled() {
        return Ok(ResearchReport {
            task_id: String::new(),
            question,
            answer: String::new(),
            enabled: false,
            note: Some(
                "AI 未启用：请先在 Settings → AI 运行时 配置 API Key，再启动研究。".into(),
            ),
            steps: Vec::new(),
            review_id: None,
        });
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO research_tasks(id, question_text, status) VALUES (?1, ?2, 'running')",
        params![task_id, question],
    )?;

    let run_id = input
        .run_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    match runtime::run(conn, AgentRole::Research, &question, &run_id, sink) {
        Ok(run) => {
            let steps_value = serde_json::to_value(&run.steps)
                .unwrap_or_else(|_| json!([]));
            let findings = json!({
                "summary": run.answer,
                "steps": steps_value,
            });

            conn.execute(
                "UPDATE research_tasks SET status='completed', findings=?2, run_id=?3, updated_at=datetime('now') WHERE id=?1",
                params![task_id, findings.to_string(), run_id],
            )?;

            // Findings 只进 Review，不直接落库（本服务的核心不变量）。
            let review_id = review_repository::insert_pending(
                conn,
                ReviewTarget::ResearchFinding,
                &task_id,
                findings,
            )?;

            Ok(ResearchReport {
                task_id,
                question,
                answer: run.answer,
                enabled: true,
                note: None,
                steps: run
                    .steps
                    .into_iter()
                    .map(|step| AgentStepDto {
                        tool: step.tool,
                        args: step.args,
                        summary: step.summary,
                    })
                    .collect(),
                review_id: Some(review_id.as_str().to_string()),
            })
        }
        Err(err) => {
            conn.execute(
                "UPDATE research_tasks SET status='failed', findings=?2, updated_at=datetime('now') WHERE id=?1",
                params![task_id, json!({ "error": err.to_string() }).to_string()],
            )?;
            Err(err)
        }
    }
}
