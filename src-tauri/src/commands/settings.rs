//! 元信息相关命令。
//!
//! 这三个命令不需要业务参数，但契约约定"每个 command 只接收一个名为
//! `input` 的对象"，因此仍然声明了 `input` 参数并显式忽略它——
//! 保持前后端签名一致，比省一个参数更重要。

use tauri::State;

use crate::application::dto::{AiSettings, AppInfo, HealthReport, Registries, UpdateAiSettings};
use crate::application::settings_service;
use crate::error::AppError;
use crate::AppState;

/// 应用元信息（版本、数据库路径、注册表指纹、AI 是否启用）。
#[tauri::command]
pub fn app_info(
    state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<AppInfo, AppError> {
    let _ = input;
    let conn = state.open()?;
    settings_service::app_info(&state.db_path, &conn)
}

/// 全部受控词表。前端**不允许**硬编码枚举值。
#[tauri::command]
pub fn list_registries(
    _state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<Registries, AppError> {
    let _ = input;
    settings_service::list_registries()
}

/// Knowledge Health —— 全部指标都是真实计数。
#[tauri::command]
pub fn knowledge_health(
    state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<HealthReport, AppError> {
    let _ = input;
    let conn = state.open()?;
    settings_service::knowledge_health(&conn)
}

/// 读取 AI 运行时设置（不含明文 API Key，仅告知是否已配置）。
#[tauri::command]
pub fn get_settings(
    state: State<'_, AppState>,
    input: Option<serde_json::Value>,
) -> Result<AiSettings, AppError> {
    let _ = input;
    let conn = state.open()?;
    settings_service::get_ai_settings(&conn)
}

/// 更新 AI 运行时设置（API Key / Base URL / Chat 模型 / 向量模型）并回读。
#[tauri::command]
pub fn update_settings(
    state: State<'_, AppState>,
    input: UpdateAiSettings,
) -> Result<AiSettings, AppError> {
    let mut conn = state.open()?;
    settings_service::update_ai_settings(&mut conn, input)
}
