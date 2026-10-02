//! Candidate —— 候选知识（M6）。
//!
//! 纪律二：**发现 → 留痕；确认 → 生效**。抽取产出的每条候选一产生就
//! 持久化（`pending`），用户在 Review 里接受（→ Claim）/ 拒绝（留痕）。
//! 没确认 ≠ 不存在。

use crate::string_enum;

string_enum! {
    /// 候选生命周期：pending →（accept）accepted /（reject）rejected。
    pub enum CandidateStatus {
        /// 已持久化，等待用户决策。
        Pending => "pending",
        /// 用户接受：已生成 Claim（`accepted_claim_id`）。
        Accepted => "accepted",
        /// 用户拒绝（或校验未通过），原因留痕。
        Rejected => "rejected",
    }
}

/// 一条候选知识（与 `candidates` 表一一对应）。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub run_id: String,
    pub document_id: String,
    pub subject: String,
    pub predicate: String,
    pub object_text: Option<String>,
    pub content: Option<String>,
    pub claim_type: Option<String>,
    pub polarity: Option<String>,
    pub modality: Option<String>,
    pub confidence: Option<f32>,
    pub source_chunk_index: Option<i64>,
    pub source_quote: Option<String>,
    pub sentence: Option<String>,
    pub status: CandidateStatus,
    pub accepted_claim_id: Option<String>,
    pub reject_reason: Option<String>,
    pub created_at: String,
}
