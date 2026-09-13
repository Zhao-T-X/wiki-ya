//! Ask 问答命令（Phase 6）。
//!
//! 只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::ask_service;
use crate::application::dto::{AskRequest, AskResponse};
use crate::error::AppError;
use crate::AppState;

/// 基于知识库提问，返回带引用的诚实回答（流式事件经 `agent-events` 推送）。
#[tauri::command]
pub fn ask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: AskRequest,
) -> Result<AskResponse, AppError> {
    let conn = state.open()?;
    let sink = super::agent_sink(&app, input.run_id.is_some());
    ask_service::ask(&conn, input, sink.as_ref())
}
