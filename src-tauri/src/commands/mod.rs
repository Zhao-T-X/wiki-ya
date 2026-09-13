//! Tauri Commands —— IPC 边界。
//!
//! 每个 command 只做三件事（TDD §56）：
//! `Deserialize → Service → Serialize`
//!
//! **不写业务逻辑**：状态迁移、校验、事务全部下沉到 `application`。

pub mod ai;
pub mod ask;
pub mod documents;
pub mod knowledge;
pub mod migration;
pub mod research;
pub mod review;
pub mod search;
pub mod settings;
pub mod timeline;

use std::sync::Arc;

use tauri::Emitter;

/// 构造 Agent 事件发射器（TDD §53）：把事件经 Tauri 全局 channel
/// `agent-events` 推给前端。`enabled=false`（前端没带 runId）时返回
/// `None`，Service 静默运行、零开销。
pub fn agent_sink(app: &tauri::AppHandle, enabled: bool) -> Option<crate::events::EventSink> {
    if !enabled {
        return None;
    }
    let app = app.clone();
    Some(Arc::new(move |event| {
        let _ = app.emit("agent-events", event);
    }))
}
