//! Extraction 运行事件（EXTRACTION-001）。
//!
//! 经 Tauri 全局频道 `extraction-events` 推给前端，实现实时进度：
//! 后台任务每推进一个阶段 / 处理完一批块 / 完成，就发一条事件，
//! 前端无需轮询 SQLite。
//!
//! 契约定性：**事件名（`type` 标签）一旦发布不得改名，只能新增**。

use serde::Serialize;

use crate::domain::extraction::ExtractionStage;

/// 一次抽取运行的实时事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtractionEvent {
    /// 运行创建、后台任务接管。
    Started {
        run_id: String,
        document_id: String,
    },
    /// 阶段切换（Preparing → Chunking → … → Finalizing）。
    StageChanged {
        run_id: String,
        stage: ExtractionStage,
    },
    /// 抽取进度（已处理 / 总块数）。
    Progress {
        run_id: String,
        processed: usize,
        total: usize,
    },
    /// 累计发现的候选数。
    CandidateFound {
        run_id: String,
        count: usize,
    },
    /// 比对完成：相对库内已有知识的"变更数"（去重后）。
    ComparisonCompleted {
        run_id: String,
        changes: usize,
    },
    /// 运行成功完成（结果在 `result_json`）。
    Completed {
        run_id: String,
    },
    /// 运行失败。
    Failed {
        run_id: String,
        error: String,
    },
    /// 用户取消（后台任务在合适时机停下）。
    Cancelled {
        run_id: String,
    },
}

/// 事件接收端：Tauri 命令把它接到 `AppHandle::emit`（推前端）。
///
/// 与 [`crate::events::EventSink`] 同理，只是面向 `ExtractionEvent`
/// 并打到独立的 `extraction-events` 频道，避免与 Agent 事件混流。
pub type ExtractionSink = std::sync::Arc<dyn Fn(&ExtractionEvent) + Send + Sync>;
