//! 审核相关的确定性文本与优先级。
//!
//! 这一层只产出「四问」的**回答文本**与审核优先级，不做 I/O、不依赖应用层。
//! 具体把文本包成 IPC 卡片（`ReviewItem`）的是 application 层的 review_service——
//! 保持分层：领域层不知道 `ClaimRelationCard` 长什么样。

use crate::domain::evolution::classifier::ClaimRelationType;
use crate::string_enum;

string_enum! {
    /// 审核状态（4 个）。
    ///
    /// `superseded` 在这里的含义与 Claim 不同：指**这条审核本身**被更新的
    /// 提案取代了（例如同一个关系被重新判定），保留它是为了不丢审核历史。
    pub enum ReviewStatus {
        Pending => "pending",
        Accepted => "accepted",
        Rejected => "rejected",
        Superseded => "superseded",
    }
}

impl ReviewStatus {
    pub const DEFAULT: ReviewStatus = ReviewStatus::Pending;

    pub fn is_open(&self) -> bool {
        matches!(self, ReviewStatus::Pending)
    }
}

string_enum! {
    /// 审核对象类型（7 个）。
    pub enum ReviewTarget {
        Claim => "claim",
        ClaimRelation => "claim_relation",
        Entity => "entity",
        Relation => "relation",
        Evidence => "evidence",
        ResearchFinding => "research_finding",
        AgentProposal => "agent_proposal",
    }
}

/// 每种关系对应的变化描述与影响描述。
///
/// 用确定性文本回答「四问」中的 what_changed 与 impact，不调用 LLM——
/// 审核界面是用户做判断的地方，让模型来解释提案会把解释权也交出去。
pub fn describe(
    relationship: ClaimRelationType,
    source_text: &str,
    target_text: &str,
) -> (String, String) {
    match relationship {
        ClaimRelationType::Duplicate => (
            format!("「{source_text}」与已有知识「{target_text}」是同一陈述"),
            "不改变任何知识，仅把新的证据来源合并到同一条知识上。".to_string(),
        ),
        ClaimRelationType::Coexists => (
            format!("「{source_text}」与已有知识「{target_text}」可以同时成立"),
            "两条知识将并存，历史不会丢失。".to_string(),
        ),
        ClaimRelationType::Supplements => (
            format!("「{source_text}」补充了已有知识「{target_text}」"),
            "已有知识保持不变，新知识作为补充一并保留。".to_string(),
        ),
        ClaimRelationType::Supersedes => (
            format!("「{source_text}」取代了已有知识「{target_text}」"),
            "已有知识将不再作为「当前知识」，但仍保留在历史中，随时可以回答「过去是什么」。"
                .to_string(),
        ),
        ClaimRelationType::Contradicts => (
            format!("「{source_text}」与已有知识「{target_text}」互相矛盾"),
            "两者都不能被自动认定为当前知识，需要你判断哪一个成立；在你决定之前，系统不会改动任何一条。"
                .to_string(),
        ),
        ClaimRelationType::Unclear => (
            format!("「{source_text}」与「{target_text}」的关系无法确定"),
            "不做任何改动。可以在补充信息后重新分析。".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_relationship_explains_what_changed_and_impact() {
        for relationship in ClaimRelationType::ALL {
            let (what, impact) = describe(*relationship, "A", "B");
            assert!(!what.is_empty(), "{relationship} 缺少 what changed");
            assert!(!impact.is_empty(), "{relationship} 缺少 impact");
        }
    }

    #[test]
    fn supersedes_explains_history_is_kept() {
        let (_, impact) = describe(ClaimRelationType::Supersedes, "A", "B");
        assert!(impact.contains("历史"));
        let (what, _) = describe(ClaimRelationType::Supersedes, "A", "B");
        assert!(what.contains("取代"));
    }

    #[test]
    fn contradictions_state_nothing_changes_without_the_user() {
        let (_, impact) = describe(ClaimRelationType::Contradicts, "A", "B");
        assert!(impact.contains("不会改动"));
    }

    #[test]
    fn review_status_and_target_vocabularies_match_the_schema() {
        assert_eq!(ReviewStatus::ALL.len(), 4);
        assert_eq!(ReviewTarget::ALL.len(), 7);
        assert!(ReviewStatus::Pending.is_open());
    }
}
