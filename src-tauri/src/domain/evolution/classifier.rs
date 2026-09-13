//! 演化关系的枚举与映射。

use crate::string_enum;

string_enum! {
    /// Claim 之间的关系类型（6 个）。
    ///
    /// 取值 = PRD §19 的 5 种 ∪ 参考实现的 `unclear`（决策 D1）：
    /// - `supplements` 是 wiki-ya 的新增能力（PRD 要求，参考实现没有）
    /// - `unclear` 是参考实现的判定结果，用于表达「结构上无法比较」
    ///
    /// `unclear` **不会由确定性路径写入数据库**（`detect` 会跳过它），
    /// 保留它是为了让 LLM 判定与人工修正有地方可落。
    pub enum ClaimRelationType {
        Duplicate => "duplicate",
        Coexists => "coexists",
        Supplements => "supplements",
        Supersedes => "supersedes",
        Contradicts => "contradicts",
        Unclear => "unclear",
    }
}

impl ClaimRelationType {
    /// 该关系类型的判定是否需要人工确认（INV-10 的另一面）。
    pub fn is_human_decision(&self) -> bool {
        matches!(
            self,
            ClaimRelationType::Supersedes
                | ClaimRelationType::Contradicts
                | ClaimRelationType::Supplements
                | ClaimRelationType::Unclear
        )
    }

    /// 该关系是否改变「什么算当前知识」。
    ///
    /// 只有 `supersedes` 会——这正是它必须人工确认的原因。
    pub fn affects_current_knowledge(&self) -> bool {
        matches!(self, ClaimRelationType::Supersedes)
    }
}

string_enum! {
    /// Claim 关系的审核状态（3 个）。
    pub enum ClaimRelationStatus {
        Candidate => "candidate",
        Accepted => "accepted",
        Rejected => "rejected",
    }
}

impl ClaimRelationStatus {
    pub const DEFAULT: ClaimRelationStatus = ClaimRelationStatus::Candidate;

    /// 只有 `accepted` 才产生事实效果。
    pub fn is_effective(&self) -> bool {
        matches!(self, ClaimRelationStatus::Accepted)
    }
}

string_enum! {
    /// 演化分类结果（6 个）—— 对「这条新知识是什么」的结论。
    ///
    /// 与 [`ClaimRelationType`] 的区别：这里没有 `unclear`。
    /// 分类必须给出一个立场：要么是全新的，要么明确了与已有知识的关系。
    /// 「说不清」属于关系判定的中间态，不该成为分类结论。
    ///
    /// 注意 `New`：新知识的默认结果，也是唯一不需要任何已有 Claim 的情形。
    pub enum EvolutionClassification {
        New => "new",
        Duplicate => "duplicate",
        Coexists => "coexists",
        Supplements => "supplements",
        Supersedes => "supersedes",
        Contradicts => "contradicts",
    }
}

impl EvolutionClassification {
    /// 关系类型 → 分类结果。`unclear` 没有对应分类。
    pub fn from_relation(relationship: ClaimRelationType) -> Option<EvolutionClassification> {
        match relationship {
            ClaimRelationType::Duplicate => Some(EvolutionClassification::Duplicate),
            ClaimRelationType::Coexists => Some(EvolutionClassification::Coexists),
            ClaimRelationType::Supplements => Some(EvolutionClassification::Supplements),
            ClaimRelationType::Supersedes => Some(EvolutionClassification::Supersedes),
            ClaimRelationType::Contradicts => Some(EvolutionClassification::Contradicts),
            ClaimRelationType::Unclear => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_types_cover_both_prd_and_reference() {
        assert_eq!(ClaimRelationType::ALL.len(), 6);
        assert!(ClaimRelationType::ALL.iter().any(|r| r.as_str() == "supplements"));
        assert!(ClaimRelationType::ALL.iter().any(|r| r.as_str() == "unclear"));
    }

    #[test]
    fn only_supersedes_changes_current_knowledge() {
        assert!(ClaimRelationType::Supersedes.affects_current_knowledge());
        assert!(!ClaimRelationType::Contradicts.affects_current_knowledge());
        assert!(!ClaimRelationType::Coexists.affects_current_knowledge());
    }

    #[test]
    fn classification_has_no_unclear_state() {
        assert_eq!(EvolutionClassification::ALL.len(), 6);
        assert!(!EvolutionClassification::ALL
            .iter()
            .any(|c| c.as_str() == "unclear"));
        assert!(EvolutionClassification::from_relation(ClaimRelationType::Unclear).is_none());
    }

    #[test]
    fn relation_to_classification_is_consistent() {
        for relationship in ClaimRelationType::ALL {
            if *relationship == ClaimRelationType::Unclear {
                continue;
            }
            let classification = EvolutionClassification::from_relation(*relationship).unwrap();
            assert_eq!(classification.as_str(), relationship.as_str());
        }
    }
}
