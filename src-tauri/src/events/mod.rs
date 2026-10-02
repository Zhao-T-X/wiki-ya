//! 事件层（TDD §53）。
//!
//! M1 收尾后只保留统一 [`RunEvent`]：任何类型的 Run（extraction / agent /
//! ask / 未来的 skill）都发同一组事件到单一 Tauri 频道 `run-events`。
//! 旧的 `AppEvent`（agent-events）与 `ExtractionEvent`（extraction-events）
//! 已退役——它们的双频道双枚举是本次统一要消除的割裂。

pub mod run_event;

pub use run_event::{RunEvent, RunSink};
