//! Agent Profile 命令（M4）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::agent_profile_service;
use crate::application::agent_profile_crud;
use crate::application::dto::{AgentProfileInput, IdInput, RunAgentProfileInput};
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

/// 创建 Agent Profile（M12：policy 上限 propose，结构上拿不到 MUTATE）。
#[tauri::command]
pub fn create_agent_profile(
    state: State<'_, AppState>,
    input: AgentProfileInput,
) -> Result<(), AppError> {
    let mut conn = state.open()?;
    agent_profile_crud::create_profile(&mut conn, &input.into())
}

/// 更新 Agent Profile（整体替换字段）。
#[tauri::command]
pub fn update_agent_profile(
    state: State<'_, AppState>,
    input: AgentProfileInput,
) -> Result<(), AppError> {
    let mut conn = state.open()?;
    agent_profile_crud::update_profile(&mut conn, &input.into())
}

/// 删除 Agent Profile（默认 Profile 不可删除，重启会恢复出厂预置）。
#[tauri::command]
pub fn delete_agent_profile(state: State<'_, AppState>, input: IdInput) -> Result<(), AppError> {
    let conn = state.open()?;
    agent_profile_crud::delete_profile(&conn, &input.id)
}

impl From<AgentProfileInput> for crate::domain::agent_profile::AgentProfile {
    fn from(input: AgentProfileInput) -> Self {
        crate::domain::agent_profile::AgentProfile {
            name: input.name.trim().to_string(),
            display_name: if input.display_name.trim().is_empty() {
                input.name.trim().to_string()
            } else {
                input.display_name.trim().to_string()
            },
            model: input.model.trim().to_string(),
            skills: input.skills,
            policy: input.policy,
            system_prompt: input.system_prompt,
        }
    }
}
