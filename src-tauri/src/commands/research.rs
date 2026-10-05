//! Research 相关命令（Phase 6）。

use tauri::State;

use crate::application::dto::{ResearchReport, ResearchTaskCard, StartResearchInput};
use crate::application::research_service;
use crate::error::AppError;
use crate::AppState;

/// 最近的研究任务历史（新→旧）。
#[tauri::command]
pub async fn list_research_tasks(
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
pub async fn start_research(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    input: StartResearchInput,
) -> Result<ResearchReport, AppError> {
    // PERF-05：研究是「多轮 ReAct」——每轮一次模型调用（+ 可能一次工具调用），
    // 全程同步阻塞。原先直接在本命令里同步执行，**会把 IPC 线程占住整段时间**
    // （表现为 UI 卡死、其它命令排队）。这里改为：命令立即让出，
    // 先取研究闸门（限流），再把整段阻塞工作 offload 到 blocking 线程池。
    let db_path = state.db_path.clone();
    let run_events = super::run_sink(&app);

    // 先取许可再 offload：排队的研究**不占用任何线程**，
    // 也不会在拿到闸门之前就发起模型调用。
    let _permit = crate::ai::scheduler::acquire_agent().await;
    let handle = tauri::async_runtime::spawn_blocking(move || {
        let conn = crate::infrastructure::db::open(&db_path)?;
        research_service::start_research(&conn, input, Some(&run_events))
    });

    match handle.await {
        Ok(result) => result,
        Err(err) => Err(AppError::Internal(format!("研究任务线程异常终止：{err}"))),
    }
}
