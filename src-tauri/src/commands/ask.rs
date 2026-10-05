//! Ask 问答命令（Phase 6）。
//!
//! 只做三件事：`Deserialize → Service → Serialize`，不写业务逻辑。

use tauri::State;

use crate::application::ask_service;
use crate::application::dto::{AskRequest, AskResponse};
use crate::error::AppError;
use crate::AppState;

/// 基于知识库提问，返回带引用的诚实回答（流式事件经 `run-events` 推送）。
///
/// PERF-09：整条问答链是「同步阻塞」的（`reqwest::blocking` 的多次模型调用 +
/// 语义检索 + SQLite 写），整段跑完可能几十秒。命令改 async 只把它挪出主线程，
/// 挪到 tokio worker 上仍会把 worker 占死——前端轮询等 async 命令会跟着排队。
/// 故整段包进 `spawn_blocking`：既不卡 UI，也不占 worker。
#[tauri::command]
pub async fn ask(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: AskRequest,
) -> Result<AskResponse, AppError> {
    let db_path = state.db_path.clone();
    let run_events = super::run_sink(&app);
    // 先在当前线程开连接失败即刻返回，不为一个必然失败的请求排 blocking 线程。
    state.open()?;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = crate::infrastructure::db::open(&db_path)?;
        ask_service::ask(&conn, input, Some(&run_events))
    })
    .await
    .map_err(|err| AppError::Internal(format!("Ask 任务异常终止：{err}")))?
}
