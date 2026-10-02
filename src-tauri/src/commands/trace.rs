//! Run Trace 命令（M1）。
//!
//! 命令只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::dto::{IdInput, RunTraceDto};
use crate::application::trace_service;
use crate::error::AppError;
use crate::AppState;

/// 读取一条 Run 的完整 Trace（登记 + 类型相关明细）。
#[tauri::command]
pub fn get_run_trace(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<RunTraceDto, AppError> {
    let conn = state.open()?;
    trace_service::get_trace(&conn, &input.id)
}
