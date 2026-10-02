//! 统一 Run 事件（M1：单一枚举 + 单一频道 `run-events`）。
//!
//! 之前 Extraction 与 Agent 各有一套事件（ExtractionEvent / AppEvent 的
//! Agent 变体）与两个频道、两种启用语义。`RunEvent` 是统一收口：
//! 任何类型的 Run 都发同一组事件，前端一个 hook 订阅即可看到全部活动。
//!
//! 过渡策略：旧频道（agent-events / extraction-events）保留照发，
//! 前端切换到 `run-events` 后再移除（M1 不做破坏性切换）。

use serde::Serialize;

use crate::domain::run::RunType;

/// 一次 Run 生命周期中的统一事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RunEvent {
    /// Run 开始执行（queued → running）。
    Started {
        run_id: String,
        run_type: RunType,
    },
    /// 阶段变化（各类型自行定义阶段字面量）。
    StageChanged {
        run_id: String,
        stage: String,
    },
    /// 进度推进（如抽取的 processed/total 块）。
    Progress {
        run_id: String,
        processed: usize,
        total: usize,
    },
    /// 流式文本增量（Ask 回答 / Research Findings 的打字机效果）。
    TokenDelta {
        run_id: String,
        delta: String,
    },
    /// 一次工具调用开始。
    ToolCalled {
        run_id: String,
        tool: String,
        summary: String,
    },
    /// 一次工具调用结束。
    ToolCompleted {
        run_id: String,
        tool: String,
        ok: bool,
        summary: String,
    },
    /// 产出了候选知识（抽取/Skill 产物）。
    CandidateCreated {
        run_id: String,
        count: usize,
    },
    /// 产生了演化 / 纠正提案（进入 Review）。
    ProposalCreated {
        run_id: String,
        count: usize,
    },
    /// 成功完成。
    Completed {
        run_id: String,
    },
    /// 失败终止。
    Failed {
        run_id: String,
        error: String,
    },
    /// 用户取消。
    Cancelled {
        run_id: String,
    },
}

/// Run 事件接收端（与 `EventSink` 同构）：Commands 层注入 Tauri 发射器，
/// 测试注入收集器；Service / Runtime 不感知 Tauri。
pub type RunSink = std::sync::Arc<dyn Fn(&RunEvent) + Send + Sync>;
