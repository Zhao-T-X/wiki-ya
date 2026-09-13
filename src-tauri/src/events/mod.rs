//! 事件总线（TDD §53/§84）。
//!
//! `AppEvent` 同时服务于：UI 流式更新、Run Trace、调试与审计。
//! 因此事件名是**对外契约**，一旦发布不得改名，只能新增。

pub mod app_event;
pub mod extraction_event;

pub use app_event::AppEvent;
pub use extraction_event::{ExtractionEvent, ExtractionSink};

use std::sync::Arc;

/// 事件接收端（TDD §53）。
///
/// 由 Commands 层注入：Tauri 命令把它接到 `AppHandle::emit`（推前端），
/// 测试可以把它接到收集器，审计可以接到落盘。Service / Runtime 只依赖
/// 这个函数指针，不感知 Tauri。
pub type EventSink = Arc<dyn Fn(&AppEvent) + Send + Sync>;
