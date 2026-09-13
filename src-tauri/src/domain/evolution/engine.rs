//! 演化引擎的对外入口。
//!
//! 只做**编排**：确定性判定 → 排序 → 得到分类结论。
//! 任何写库动作都不在这里发生（那属于 `application::evolution_service`），
//! 因此本模块是纯函数，可以随便测。

use crate::domain::evolution::classifier::EvolutionClassification;
use crate::domain::evolution::conflict::{compare_all, ClaimView, SuggestedAction, Verdict};

/// 分析新 Claim 与全部候选的关系，最值得关注的排在前面。
pub fn analyze(incoming: &ClaimView, existing: &[ClaimView]) -> Vec<Verdict> {
    compare_all(incoming, existing)
}

/// 得出演化分类结论。
///
/// 规则：
/// - 没有任何候选 → `New`（这是"新知识"的定义）
/// - 取排在最前、且**可落库**的判定，映射成分类
/// - 全部不可落库（都是 `unclear` / `ignore`）→ `New`
///
/// 这里永远不会返回 `Supersedes`：见
/// [`crate::domain::evolution::conflict::NEVER_INFERRED`]。
pub fn classify(incoming: &ClaimView, existing: &[ClaimView]) -> EvolutionClassification {
    if existing.is_empty() {
        return EvolutionClassification::New;
    }

    let verdicts = analyze(incoming, existing);
    verdicts
        .iter()
        .filter(|verdict| verdict.is_persistable())
        .find_map(|verdict| EvolutionClassification::from_relation(verdict.relationship))
        .unwrap_or(EvolutionClassification::New)
}

/// 一次分析的汇总，供 UI 展示「这次捕获发现了什么」。
#[derive(Debug, Clone, Default)]
pub struct AnalysisSummary {
    pub candidates_compared: usize,
    pub duplicates: usize,
    pub coexists: usize,
    pub contradictions: usize,
    /// 需要人工决策的条数（`suggested_action == review`）。
    pub needs_review: usize,
}

impl AnalysisSummary {
    pub fn from_verdicts(verdicts: &[Verdict], candidates_compared: usize) -> AnalysisSummary {
        use crate::domain::evolution::classifier::ClaimRelationType;
        let mut summary = AnalysisSummary {
            candidates_compared,
            ..Default::default()
        };
        for verdict in verdicts {
            match verdict.relationship {
                ClaimRelationType::Duplicate => summary.duplicates += 1,
                ClaimRelationType::Coexists => summary.coexists += 1,
                ClaimRelationType::Contradicts => summary.contradictions += 1,
                _ => {}
            }
            if verdict.suggested_action == SuggestedAction::Review {
                summary.needs_review += 1;
            }
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::common::ids::{ClaimId, EntityId};
    use crate::domain::knowledge::claim::{ClaimObject, ClaimStatus, Polarity};
    use crate::domain::ontology::predicate::ClaimPredicate;

    fn view(id: &str, predicate: ClaimPredicate, object: &str, polarity: Polarity) -> ClaimView {
        ClaimView {
            id: ClaimId::from_raw(id),
            subject_id: EntityId::from_raw("e1"),
            predicate,
            object: Some(ClaimObject::Entity(EntityId::from_raw(object))),
            polarity,
            status: ClaimStatus::Candidate,
            created_at: "2026-01-01 00:00:00".into(),
        }
    }

    #[test]
    fn no_candidates_means_genuinely_new_knowledge() {
        let incoming = view("c1", ClaimPredicate::Uses, "e2", Polarity::Positive);
        assert_eq!(classify(&incoming, &[]), EvolutionClassification::New);
    }

    #[test]
    fn duplicate_is_classified_as_duplicate() {
        let incoming = view("c2", ClaimPredicate::Uses, "e2", Polarity::Positive);
        let existing = vec![view("c1", ClaimPredicate::Uses, "e2", Polarity::Positive)];
        assert_eq!(
            classify(&incoming, &existing),
            EvolutionClassification::Duplicate
        );
    }

    #[test]
    fn single_valued_change_is_surfaced_as_contradiction_for_the_human() {
        let incoming = view("c2", ClaimPredicate::Is, "e3", Polarity::Positive);
        let existing = vec![view("c1", ClaimPredicate::Is, "e2", Polarity::Positive)];
        let classification = classify(&incoming, &existing);
        assert_eq!(classification, EvolutionClassification::Contradicts);
        assert_ne!(classification, EvolutionClassification::Supersedes);
    }

    #[test]
    fn unrelated_candidates_still_yield_new() {
        let incoming = view("c2", ClaimPredicate::Uses, "e2", Polarity::Positive);
        let existing = vec![view("c1", ClaimPredicate::Supports, "e3", Polarity::Positive)];
        assert_eq!(classify(&incoming, &existing), EvolutionClassification::New);
    }

    #[test]
    fn summary_counts_only_actionable_signals() {
        let incoming = view("c9", ClaimPredicate::Uses, "e2", Polarity::Positive);
        let existing = vec![
            view("c1", ClaimPredicate::Uses, "e3", Polarity::Positive),
            view("c2", ClaimPredicate::Uses, "e2", Polarity::Positive),
            view("c3", ClaimPredicate::Uses, "e2", Polarity::Negative),
        ];
        let verdicts = analyze(&incoming, &existing);
        let summary = AnalysisSummary::from_verdicts(&verdicts, existing.len());
        assert_eq!(summary.candidates_compared, 3);
        assert_eq!(summary.duplicates, 1);
        assert_eq!(summary.coexists, 1);
        assert_eq!(summary.contradictions, 1);
        assert_eq!(summary.needs_review, 1);
    }
}
