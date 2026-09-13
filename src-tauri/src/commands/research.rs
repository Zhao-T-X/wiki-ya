//! Research 相关命令（Phase 6）。

use tauri::State;

use crate::application::dto::{ResearchReport, ResearchTaskCard, StartResearchInput};
use crate::application::research_service;
use crate::error::AppError;
use crate::AppState;

/// 最近的研究任务历史（新→旧）。
#[tauri::command]
pub fn list_research_tasks(
    state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<Vec<ResearchTaskCard>, AppError> {
    let _ = input;
    let conn = state.open()?;
    research_service::list_tasks(&conn, 20)
}

/// 启动一次多步研究（Phase 6）。过程事件经 `agent-events` 实时推送。
///
/// Findings 只进 Review 队列，绝不直接落库（AI suggests, user decides）。
#[tauri::command]
pub fn start_research(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: StartResearchInput,
) -> Result<ResearchReport, AppError> {
    let conn = state.open()?;
    let sink = super::agent_sink(&app, input.run_id.is_some());
    research_service::start_research(&conn, input, sink.as_ref())
}
