//! 统一 Run 模型（M1：Run → Skill → Tool → Result → Trace）。
//!
//! 一次"系统为用户做的努力"——抽取、问答、研究、未来的 Skill 执行——
//! 都登记为一条 [`RunRecord`]。明细留在各自的表（extraction_runs 的
//! 阶段/进度、agent_runs 的步骤），`runs` 承载统一身份与生命周期，
//! Trace 用一条 JOIN 就能把 Source → Run → Result → Review 串起来。
//!
//! 关键不变量：
//! - **同一 id 全库一致**：`runs.id` 与 `extraction_runs.id` / `agent_runs.id`
//!   相同（登记，不另发号），保证跨表 JOIN 无歧义。
//! - `status` 沿用与 Extraction Run 相同的字面量集合（queued/running/…）。

use crate::string_enum;

string_enum! {
    /// Run 的类型（决定明细去哪张表、事件走哪条语义）。
    pub enum RunType {
        /// 异步知识抽取（明细：extraction_runs）。
        Extraction => "extraction",
        /// Agent 循环（ReAct）运行（明细：agent_runs）。
        Agent => "agent",
        /// Ask 问答（预留：当前 ask 的遥测归入 agent）。
        Ask => "ask",
        /// Skill 执行（M2 起启用）。
        Skill => "skill",
        /// Review 决策过程（预留）。
        Review => "review",
    }
}

// 生命周期状态与 Extraction Run 完全同构：同一套字面量，同一套终态语义。
pub use crate::domain::extraction::ExtractionRunStatus as RunStatus;

/// 一条统一登记的 Run。
#[derive(Debug, Clone)]
pub struct RunRecord {
    pub id: String,
    pub parent_run_id: Option<String>,
    pub run_type: RunType,
    pub actor: String,
    pub status: RunStatus,
    pub stage: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub metadata: String,
    /// 本次 Run 的真实 token 账本（JSON 序列化的 `TokenUsage`，PR-07）。
    /// 未发生模型调用或 provider 未上报时为 `None`。
    pub usage_json: Option<String>,
}
