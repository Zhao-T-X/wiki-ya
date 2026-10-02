//! Skill Runtime 命令（M2）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::dto::{
    CreateSkillInput, IdInput, RunSkillInput, SkillDescriptorDto, UpdateSkillInput,
};
use crate::application::skill_service;
use crate::error::AppError;
use crate::AppState;

/// 枚举内置 Skill（名称 / 描述 / 权限 / 输入约定）。
#[tauri::command]
pub fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillDescriptorDto>, AppError> {
    let conn = state.open()?;
    skill_service::list_skills(&conn)
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

/// 创建自定义 Skill（M11：强制只读，产不出候选/提案）。
#[tauri::command]
pub fn create_skill(state: State<'_, AppState>, input: CreateSkillInput) -> Result<(), AppError> {
    let conn = state.open()?;
    skill_service::create_skill(&conn, &input.name, &input.description, &input.instructions)
}

/// 更新自定义 Skill：产生新版本（内置 Skill 拒绝）。
#[tauri::command]
pub fn update_skill(state: State<'_, AppState>, input: UpdateSkillInput) -> Result<i64, AppError> {
    let conn = state.open()?;
    skill_service::update_skill(&conn, &input.name, &input.description, &input.instructions)
}

/// 删除自定义 Skill（内置 Skill 拒绝）。
#[tauri::command]
pub fn delete_skill(state: State<'_, AppState>, input: IdInput) -> Result<(), AppError> {
    let conn = state.open()?;
    skill_service::delete_skill(&conn, &input.id)
}
