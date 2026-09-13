//! `compare_claim` —— 两条 Claim 之间到底什么关系。
//!
//! 整个判定**只使用已存储的结构**：主语 id、谓语、宾语同一性、极性。
//! 因此同样的输入永远给出同样的答案，且不会有任何东西在用户背后被改动。
//!
//! 判定表（对应 `docs/领域枚举与不变量定义.md` §4.4）：
//!
//! | 条件 | 结论 | 置信度 | 建议动作 | 自动确认 |
//! |---|---|---|---|---|
//! | 主语/谓语不同 | unclear | 0.00 | ignore | — |
//! | 宾语相同、极性相同 | duplicate | 0.97 | link_evidence | ✅ |
//! | 宾语相同、极性不同 | contradicts | 0.90 | review | ❌ |
//! | 宾语不同、谓语单值 | contradicts | 0.55 | review | ❌ |
//! | 宾语不同、谓语可多值 | coexists | 0.70 | keep_both | ✅ |
//! | 任一侧宾语未消解 | unclear | 0.20 | review | — |
//!
//! **`supersedes` 不在表内。** 见本模块末尾的 `NEVER_INFERRED` 常量。

use crate::domain::common::ids::{ClaimId, EntityId};
use crate::domain::common::Timestamp;
use crate::domain::evolution::classifier::ClaimRelationType;
use crate::domain::evolution::rules::{is_functional, review_priority};
use crate::domain::knowledge::claim::{ClaimObject, ClaimStatus, Polarity};
use crate::domain::ontology::predicate::ClaimPredicate;

/// 建议动作。决定这条关系是自动确认还是进人工队列。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedAction {
    /// 同一陈述的另一个来源：把证据挂上去即可，不改动知识。
    LinkEvidence,
    /// 两条事实并存：同样不改动知识。
    KeepBoth,
    /// 可能改变知识：必须人工判断。
    Review,
    /// 结构上无法比较：忽略，不入库。
    Ignore,
}

impl SuggestedAction {
    pub const fn as_str(&self) -> &'static str {
        match self {
            SuggestedAction::LinkEvidence => "link_evidence",
            SuggestedAction::KeepBoth => "keep_both",
            SuggestedAction::Review => "review",
            SuggestedAction::Ignore => "ignore",
        }
    }

    /// 该动作是否可以在无人参与的情况下直接落库（INV-10）。
    ///
    /// 只有"不改变知识"的动作可以自动化——重复链接与并存事实都属于此类。
    pub fn can_auto_accept(&self) -> bool {
        matches!(self, SuggestedAction::LinkEvidence | SuggestedAction::KeepBoth)
    }
}

/// 判定所需的最小 Claim 视图。
///
/// 刻意不直接用 `Claim`：判定只依赖这几个字段，把入参收窄能让
/// 「判定逻辑依赖了什么」一目了然，也让单元测试不必构造整个 Claim。
#[derive(Debug, Clone)]
pub struct ClaimView {
    pub id: ClaimId,
    pub subject_id: EntityId,
    pub predicate: ClaimPredicate,
    pub object: Option<ClaimObject>,
    pub polarity: Polarity,
    pub status: ClaimStatus,
    pub created_at: Timestamp,
}

impl ClaimView {
    /// 归一化宾语同一性（有实体时走实体 id）。
    pub fn object_key(&self) -> String {
        self.object
            .as_ref()
            .map(ClaimObject::identity_key)
            .unwrap_or_default()
    }
}

/// 一条判定结果。
#[derive(Debug, Clone)]
pub struct Verdict {
    pub relationship: ClaimRelationType,
    /// 与之比较的已有 Claim。永远是**传入的那一条**，不受调用顺序影响。
    pub related_claim_id: ClaimId,
    pub confidence: f32,
    pub reason: String,
    pub suggested_action: SuggestedAction,
}

impl Verdict {
    /// 是否值得写进 `claim_relations`。
    ///
    /// `unclear` 不入库：它不表达任何知识，只会污染审核队列。
    pub fn is_persistable(&self) -> bool {
        self.relationship != ClaimRelationType::Unclear
            && self.suggested_action != SuggestedAction::Ignore
    }

    /// 落库时的初始状态。
    pub fn initial_status(&self) -> crate::domain::evolution::classifier::ClaimRelationStatus {
        use crate::domain::evolution::classifier::ClaimRelationStatus;
        if self.suggested_action.can_auto_accept() {
            ClaimRelationStatus::Accepted
        } else {
            ClaimRelationStatus::Candidate
        }
    }
}

fn verdict(
    relationship: ClaimRelationType,
    existing: &ClaimView,
    confidence: f32,
    reason: impl Into<String>,
    action: SuggestedAction,
) -> Verdict {
    Verdict {
        relationship,
        related_claim_id: existing.id.clone(),
        confidence: (confidence.clamp(0.0, 1.0) * 1000.0).round() / 1000.0,
        reason: reason.into(),
        suggested_action: action,
    }
}

/// 判断 `new` 相对 `existing` 的关系。
///
/// 结果与「哪一条先入库」无关：返回的 `related_claim_id` 恒为 `existing.id`。
pub fn compare_claim(new: &ClaimView, existing: &ClaimView) -> Verdict {
    let same_subject = new.subject_id == existing.subject_id;
    let same_predicate = new.predicate == existing.predicate;

    if !(same_subject && same_predicate) {
        return verdict(
            ClaimRelationType::Unclear,
            existing,
            0.0,
            "主语或谓语不同，结构上没有可比性",
            SuggestedAction::Ignore,
        );
    }

    let new_object = new.object_key();
    let old_object = existing.object_key();

    if !new_object.is_empty() && new_object == old_object {
        if new.polarity == existing.polarity {
            return verdict(
                ClaimRelationType::Duplicate,
                existing,
                0.97,
                "主语、谓语、宾语与极性都相同——同一陈述被另一个来源再次断言",
                SuggestedAction::LinkEvidence,
            );
        }
        return verdict(
            ClaimRelationType::Contradicts,
            existing,
            0.9,
            format!(
                "同一陈述被同时断言为 {} 与 {}，来源之间存在分歧",
                existing.polarity, new.polarity
            ),
            SuggestedAction::Review,
        );
    }

    if !new_object.is_empty() && !old_object.is_empty() {
        if is_functional(new.predicate) {
            return verdict(
                ClaimRelationType::Contradicts,
                existing,
                0.55,
                format!(
                    "`{}` 同一主语只能有一个取值，但取值发生了变化——请先比对来源，再决定哪个是当前知识",
                    new.predicate
                ),
                SuggestedAction::Review,
            );
        }
        return verdict(
            ClaimRelationType::Coexists,
            existing,
            0.7,
            "主语与谓语相同但宾语不同，两者可以同时成立",
            SuggestedAction::KeepBoth,
        );
    }

    verdict(
        ClaimRelationType::Unclear,
        existing,
        0.2,
        "有一侧宾语缺失或未消解为实体，无法可靠比较",
        SuggestedAction::Review,
    )
}

/// 与一组已有 Claim 比较，最值得关注的关系排在前面。
pub fn compare_all(new: &ClaimView, existing: &[ClaimView]) -> Vec<Verdict> {
    let mut verdicts: Vec<Verdict> = existing
        .iter()
        .map(|other| compare_claim(new, other))
        .collect();
    verdicts.sort_by(|left, right| {
        let left_priority = review_priority(left.relationship.as_str());
        let right_priority = review_priority(right.relationship.as_str());
        left_priority
            .cmp(&right_priority)
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    verdicts
}

/// 永远不会由确定性规则推断出的关系类型。
///
/// 判断"新值取代了旧值"需要时间或文本证据（"X 已经取代 Y"），
/// 结构比较给不出这个结论。把它显式写成常量，是为了让
/// 「为什么 `compare_claim` 里没有 supersedes 分支」有一个可引用的答案，
/// 也为了让未来的修改者先看到这条约束。
pub const NEVER_INFERRED: ClaimRelationType = ClaimRelationType::Supersedes;

#[cfg(test)]
mod tests {
    use super::*;

    fn view(
        id: &str,
        subject: &str,
        predicate: ClaimPredicate,
        object: Option<ClaimObject>,
        polarity: Polarity,
    ) -> ClaimView {
        ClaimView {
            id: ClaimId::from_raw(id),
            subject_id: EntityId::from_raw(subject),
            predicate,
            object,
            polarity,
            status: ClaimStatus::Candidate,
            created_at: "2026-01-01 00:00:00".into(),
        }
    }

    fn entity_object(id: &str) -> Option<ClaimObject> {
        Some(ClaimObject::Entity(EntityId::from_raw(id)))
    }

    #[test]
    fn same_statement_with_same_polarity_is_a_duplicate() {
        let new = view(
            "c2",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let old = view(
            "c1",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Duplicate);
        assert_eq!(v.suggested_action, SuggestedAction::LinkEvidence);
        assert!(v.suggested_action.can_auto_accept());
        assert_eq!(v.related_claim_id.as_str(), "c1");
    }

    #[test]
    fn opposite_polarity_on_the_same_statement_is_a_contradiction() {
        let new = view(
            "c2",
            "e1",
            ClaimPredicate::Supports,
            Some(ClaimObject::Literal("async fn in trait".into())),
            Polarity::Negative,
        );
        let old = view(
            "c1",
            "e1",
            ClaimPredicate::Supports,
            Some(ClaimObject::Literal("async fn in trait".into())),
            Polarity::Positive,
        );
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Contradicts);
        assert_eq!(v.suggested_action, SuggestedAction::Review);
        assert!(!v.suggested_action.can_auto_accept());
    }

    #[test]
    fn functional_predicate_with_a_new_object_asks_the_human() {
        let new = view(
            "c2",
            "e1",
            ClaimPredicate::Is,
            entity_object("e3"),
            Polarity::Positive,
        );
        let old = view(
            "c1",
            "e1",
            ClaimPredicate::Is,
            entity_object("e2"),
            Polarity::Positive,
        );
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Contradicts);
        assert_eq!(v.confidence, 0.55);
        assert_eq!(v.suggested_action, SuggestedAction::Review);
    }

    #[test]
    fn multi_valued_predicate_coexists_and_is_safe_to_automate() {
        let new = view(
            "c2",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e3"),
            Polarity::Positive,
        );
        let old = view(
            "c1",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Coexists);
        assert_eq!(v.suggested_action, SuggestedAction::KeepBoth);
        assert!(v.suggested_action.can_auto_accept());
    }

    #[test]
    fn unresolved_objects_are_honestly_unclear() {
        let new = view(
            "c2",
            "e1",
            ClaimPredicate::Uses,
            Some(ClaimObject::Literal("something".into())),
            Polarity::Positive,
        );
        let old = view("c1", "e1", ClaimPredicate::Uses, None, Polarity::Positive);
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Unclear);
        assert!(!v.is_persistable());
    }

    #[test]
    fn different_subjects_never_relate() {
        let new = view(
            "c2",
            "e9",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let old = view(
            "c1",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let v = compare_claim(&new, &old);
        assert_eq!(v.relationship, ClaimRelationType::Unclear);
        assert_eq!(v.suggested_action, SuggestedAction::Ignore);
    }

    #[test]
    fn supersedes_is_never_inferred_by_deterministic_rules() {
        // 用各种会造成"知识变更"的组合反复验证：永远不会得到 supersedes。
        let cases = [
            (
                view("c2", "e1", ClaimPredicate::Is, entity_object("e3"), Polarity::Positive),
                view("c1", "e1", ClaimPredicate::Is, entity_object("e2"), Polarity::Positive),
            ),
            (
                view("c2", "e1", ClaimPredicate::Uses, entity_object("e3"), Polarity::Negative),
                view("c1", "e1", ClaimPredicate::Uses, entity_object("e3"), Polarity::Positive),
            ),
        ];
        for (new, old) in cases {
            assert_ne!(compare_claim(&new, &old).relationship, NEVER_INFERRED);
        }
    }

    #[test]
    fn results_are_sorted_by_priority_then_confidence() {
        let new = view(
            "c9",
            "e1",
            ClaimPredicate::Uses,
            entity_object("e2"),
            Polarity::Positive,
        );
        let existing = vec![
            view("c1", "e1", ClaimPredicate::Uses, entity_object("e3"), Polarity::Positive), // coexists
            view("c2", "e1", ClaimPredicate::Uses, entity_object("e2"), Polarity::Positive), // duplicate
            view("c3", "e1", ClaimPredicate::Uses, entity_object("e2"), Polarity::Negative), // contradicts
        ];
        let verdicts = compare_all(&new, &existing);
        let order: Vec<&str> = verdicts
            .iter()
            .map(|v| v.relationship.as_str())
            .collect();
        assert_eq!(order, vec!["duplicate", "contradicts", "coexists"]);
    }
}
