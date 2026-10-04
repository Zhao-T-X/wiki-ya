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

string_enum! {
    /// 候选的原文锚定支持度（PR-03，本地校验，无需 LLM）。
    ///
    /// 回答"这条候选的知识从哪段原文里来、能否被验证"——
    /// 这是知识可追溯（Provenance）的硬门槛。
    pub enum SupportLevel {
        /// quote 逐字落在来源切片原文中：完全锚定，可进入正常 Review。
        Directly => "directly",
        /// 有切片锚点但无逐字引用（LLM 改写 / 缺 quote）：需人工补证据。
        Partially => "partially",
        /// 无切片锚点，无法验证：必须经人工 grounding 后才可接受。
        Unsupported => "unsupported",
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
    pub support_level: SupportLevel,
    pub status: CandidateStatus,
    pub accepted_claim_id: Option<String>,
    pub reject_reason: Option<String>,
    pub created_at: String,
}

/// 本地 grounding 判定：quote 是否落在 chunk 原文中（**无需 LLM**）。
///
/// - quote 非空且在切片原文中（空白归一化后逐字包含）→ `Directly`；
/// - 有切片锚点但 quote 缺失 / 不逐字 → `Partially`；
/// - 无切片锚点（无法验证）→ `Unsupported`。
///
/// 归一化只折叠空白，保持大小写原样，避免把"改写"误判成"锚定"。
pub fn classify_support(quote: &Option<String>, chunk_text: Option<&str>) -> SupportLevel {
    match (quote.as_deref(), chunk_text) {
        (Some(q), Some(ct)) if !q.trim().is_empty() => {
            if normalize_ws(ct).contains(&normalize_ws(q)) {
                SupportLevel::Directly
            } else {
                SupportLevel::Partially
            }
        }
        (_, Some(_)) => SupportLevel::Partially,
        _ => SupportLevel::Unsupported,
    }
}

/// 折叠所有连续空白为单个空格，便于做稳健的子串匹配。
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_support_tiers() {
        // 逐字落在原文 → directly
        assert_eq!(
            classify_support(&Some("Rust 支持 async".into()), Some("Rust 支持 async fn。")),
            SupportLevel::Directly
        );
        // 空白差异应仍能匹配（归一化）
        assert_eq!(
            classify_support(&Some("Rust  支持".into()), Some("Rust 支持 async。")),
            SupportLevel::Directly
        );
        // quote 不在原文 → partially（有切片锚点）
        assert_eq!(
            classify_support(&Some("不在原文里".into()), Some("Rust 支持 async。")),
            SupportLevel::Partially
        );
        // 有切片但无 quote → partially
        assert_eq!(
            classify_support(&None, Some("Rust 支持 async。")),
            SupportLevel::Partially
        );
        // 无切片锚点 → unsupported
        assert_eq!(classify_support(&Some("x".into()), None), SupportLevel::Unsupported);
        assert_eq!(classify_support(&None, None), SupportLevel::Unsupported);
    }
}

