//! Extraction Run 命令（EXTRACTION-001）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。
//!
//! `start_extraction` 是**异步**的：它创建 Run 并立即返回 `run_id`，
//! 真正的抽取在后台跑，进度由 `extraction-events` 实时推给前端。

use tauri::State;

use crate::application::dto::{ExtractionRunDto, IdInput, LimitInput};
use crate::application::extraction_service;
use crate::error::AppError;
use crate::AppState;

/// 启动一次文档抽取（后台 Run）。
///
/// 立即返回 `run_id`，不等待抽取完成——前端据此订阅 `extraction-events`
/// 实时进度，或轮询 `get_extraction_run`。
#[tauri::command]
pub async fn start_extraction(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<String, AppError> {
    let conn = state.open()?;
    let run_id = extraction_service::create_run(&conn, &input.id)?;

    // 命令立刻返回；后台任务在独立线程跑完整管线。
    tauri::async_runtime::spawn(extraction_service::execute(
        app,
        state.db_path.clone(),
        run_id.clone(),
    ));

    Ok(run_id)
}

/// 读取一条 Run 的当前快照。
#[tauri::command]
pub fn get_extraction_run(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<ExtractionRunDto, AppError> {
    let conn = state.open()?;
    extraction_service::get_run(&conn, &input.id)
}

/// 列出最近的 Run（侧栏 Activity 用）。
#[tauri::command]
pub fn list_extraction_runs(
    state: State<'_, AppState>,
    input: Option<LimitInput>,
) -> Result<Vec<ExtractionRunDto>, AppError> {
    let conn = state.open()?;
    let limit = input.and_then(|i| i.limit).unwrap_or(20);
    extraction_service::list_runs(&conn, limit)
}

/// 取消一条还在跑的 Run。已终态则无操作。
#[tauri::command]
pub fn cancel_extraction(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<bool, AppError> {
    let conn = state.open()?;
    extraction_service::cancel_run(&conn, &input.id)
}
