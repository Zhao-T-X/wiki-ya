//! Agent Profile 命令（M4）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::agent_profile_service;
use crate::application::dto::RunAgentProfileInput;
use crate::domain::agent_profile::AgentProfile;
use crate::error::AppError;
use crate::AppState;

/// 列出全部 Agent Profile。
#[tauri::command]
pub fn list_agent_profiles(
    state: State<'_, AppState>,
) -> Result<Vec<AgentProfile>, AppError> {
    let conn = state.open()?;
    agent_profile_service::list_profiles(&conn)
}

/// 启动一次 Agent Run：按 Profile 声明的顺序执行其 Skill（每个 Skill
/// 是一个子 Skill Run）。立即返回 `run_id`，过程经 `run-events` 推送。
#[tauri::command]
pub async fn run_agent_profile(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: RunAgentProfileInput,
) -> Result<String, AppError> {
    agent_profile_service::start_agent(
        app,
        state.db_path.clone(),
        &input.name,
        input.input,
    )
}
