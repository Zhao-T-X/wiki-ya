//! Tauri Commands —— IPC 边界。
//!
//! 每个 command 只做三件事（TDD §56）：
//! `Deserialize → Service → Serialize`
//!
//! **不写业务逻辑**：状态迁移、校验、事务全部下沉到 `application`。

pub mod agent_profile;
pub mod ai;
pub mod ask;
pub mod candidate;
pub mod documents;
pub mod extraction;
pub mod knowledge;
pub mod migration;
pub mod research;
pub mod review;
pub mod search;
pub mod settings;
pub mod skill;
pub mod timeline;
pub mod trace;

use std::sync::Arc;

use tauri::Emitter;

/// 构造统一 Run 事件发射器（M1）：任何类型的 Run 都发到单一频道
/// `run-events`，前端一个订阅即可看到全部活动。
pub fn run_sink(app: &tauri::AppHandle) -> crate::events::RunSink {
    let app = app.clone();
    Arc::new(move |event| {
        let _ = app.emit("run-events", event);
    })
}
