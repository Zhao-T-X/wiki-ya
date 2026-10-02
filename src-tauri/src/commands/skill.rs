//! Skill Runtime 命令（M2）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::dto::{RunSkillInput, SkillDescriptorDto};
use crate::application::skill_service;
use crate::error::AppError;
use crate::AppState;

/// 枚举内置 Skill（名称 / 描述 / 权限 / 输入约定）。
#[tauri::command]
pub fn list_skills() -> Vec<SkillDescriptorDto> {
    skill_service::list_skills()
}

/// 启动一次 Skill Run：立即返回 `run_id`，执行在后台完成，
/// 过程与结果经 `run-events` 推送（订阅 `useRunEvents`）。
#[tauri::command]
pub async fn run_skill(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: RunSkillInput,
) -> Result<String, AppError> {
    skill_service::start_skill(
        app,
        state.db_path.clone(),
        &input.name,
        input.input,
        input.parent_run_id,
    )
}
