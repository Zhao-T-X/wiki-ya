//! Temporal —— 「当前知识」的派生（Rule 4）。
//!
//! Current Knowledge **不是**一份被复制出来的数据，而是
//! `status` + 演化关系 + 时间有效性共同派生出来的结论。
//! 因此 `History ≠ Current State`，两者可以同时被查询。
//!
//! 派生规则（对应实现缺口 G4 的闭合）：
//!
//! 1. 被 `superseded` 的 Claim，如果**已有当前知识覆盖同一个
//!    (主语, 谓语)**，则从结果中剔除——「现任 CEO 是谁」绝不能用
//!    被替换掉的陈述回答。
//! 2. 被 `superseded` 但没有当前知识覆盖它时，**保留并标记为历史**——
//!    「前任 CEO 是谁」仍然需要证据。
//! 3. 时间有效性参与 `covered` 的计算：已过期的 Claim 不能充当
//!    "当前知识"去遮蔽历史，否则知识会在时间维度上凭空消失。

use crate::domain::common::ids::{ClaimId, EntityId};
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::domain::evolution::conflict::ClaimView;
use crate::domain::knowledge::claim::ClaimStatus;

/// 一条 Claim 在「当下」的定位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// 当前知识。
    Current,
    /// 已被取代的历史。
    Superseded,
}

impl Lifecycle {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Lifecycle::Current => "current",
            Lifecycle::Superseded => "superseded",
        }
    }
}

/// 派生结果。
#[derive(Debug, Clone)]
pub struct ResolvedClaim {
    pub id: ClaimId,
    pub subject_id: EntityId,
    pub predicate: ClaimPredicate,
    pub lifecycle: Lifecycle,
}

impl ResolvedClaim {
    pub fn is_current(&self) -> bool {
        self.lifecycle == Lifecycle::Current
    }
}

/// 时间有效性的判定。
///
/// `None` 表示该侧无界。**无界一律视为有效**（决策 D2）：
/// 抽取时间的失败率很高，若把"没有时间"当作"已过期"，
/// 系统会静默丢掉大量知识。宁可保守地认为它仍然成立。
pub fn is_temporally_valid(
    valid_from: Option<&str>,
    valid_until: Option<&str>,
    now: &str,
) -> bool {
    if let Some(from) = valid_from {
        if !from.is_empty() && from > now {
            return false;
        }
    }
    if let Some(until) = valid_until {
        if !until.is_empty() && until <= now {
            return false;
        }
    }
    true
}

/// 时间有效性三元组，供调用方传入（`ClaimView` 不含时间字段，
/// 因为确定性关系判定不需要它）。
#[derive(Debug, Clone, Copy, Default)]
pub struct Validity<'a> {
    pub valid_from: Option<&'a str>,
    pub valid_until: Option<&'a str>,
}

/// 派生「当前知识」视图。
///
/// `claims` 与 `validities` 必须一一对应（同序）。
pub fn resolve_current(
    claims: &[ClaimView],
    validities: &[Validity<'_>],
    now: &str,
) -> Vec<ResolvedClaim> {
    // 主语 + 谓语 构成「同一件事」的键。
    let covered: std::collections::HashSet<(&str, &str)> = claims
        .iter()
        .zip(validities.iter())
        .filter(|(claim, validity)| {
            claim.status.counts_as_current()
                && is_temporally_valid(validity.valid_from, validity.valid_until, now)
        })
        .map(|(claim, _)| (claim.subject_id.as_str(), claim.predicate.as_str()))
        .collect();

    let mut resolved = Vec::with_capacity(claims.len());
    for (claim, validity) in claims.iter().zip(validities.iter()) {
        // 不参与检索的状态（rejected / archived / draft）直接排除。
        if !claim.status.participates_in_retrieval() {
            continue;
        }

        if claim.status == ClaimStatus::Superseded {
            let key = (claim.subject_id.as_str(), claim.predicate.as_str());
            if covered.contains(&key) {
                // 有当前知识覆盖 → 历史让位
                continue;
            }
            resolved.push(ResolvedClaim {
                id: claim.id.clone(),
                subject_id: claim.subject_id.clone(),
                predicate: claim.predicate,
                lifecycle: Lifecycle::Superseded,
            });
            continue;
        }

        // 时间上已失效的 Claim 仍会出现在结果里，但不算「当前知识」，
        // 因此标记为历史——它的内容依然可被引用（回答"曾经如何"），
        // 但不会被用来回答"现在如何"。
        let fresh = is_temporally_valid(validity.valid_from, validity.valid_until, now);
        resolved.push(ResolvedClaim {
            id: claim.id.clone(),
            subject_id: claim.subject_id.clone(),
            predicate: claim.predicate,
            lifecycle: if fresh {
                Lifecycle::Current
            } else {
                Lifecycle::Superseded
            },
        });
    }

    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::claim::{ClaimObject, Polarity};

    fn claim(id: &str, status: ClaimStatus, predicate: ClaimPredicate) -> ClaimView {
        ClaimView {
            id: ClaimId::from_raw(id),
            subject_id: EntityId::from_raw("e1"),
            predicate,
            object: Some(ClaimObject::Literal(id.into())),
            polarity: Polarity::Positive,
            status,
            created_at: "2026-01-01 00:00:00".into(),
        }
    }

    const NOW: &str = "2026-06-01 00:00:00";

    #[test]
    fn superseded_is_dropped_when_a_current_claim_covers_it() {
        let claims = vec![
            claim("old", ClaimStatus::Superseded, ClaimPredicate::Is),
            claim("new", ClaimStatus::Verified, ClaimPredicate::Is),
        ];
        let validities = vec![Validity::default(), Validity::default()];
        let resolved = resolve_current(&claims, &validities, NOW);
        let ids: Vec<&str> = resolved.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["new"]);
    }

    #[test]
    fn superseded_survives_when_nothing_current_covers_it() {
        let claims = vec![
            claim("old", ClaimStatus::Superseded, ClaimPredicate::Is),
            claim("other", ClaimStatus::Verified, ClaimPredicate::Supports),
        ];
        let validities = vec![Validity::default(), Validity::default()];
        let resolved = resolve_current(&claims, &validities, NOW);
        let history = resolved
            .iter()
            .find(|c| c.id.as_str() == "old")
            .expect("历史不应被删除");
        assert_eq!(history.lifecycle, Lifecycle::Superseded);
        assert!(!history.is_current());
    }

    #[test]
    fn an_expired_claim_cannot_shadow_history() {
        // 当前 claim 已过期 → 它不再遮蔽被取代的历史。
        let claims = vec![
            claim("old", ClaimStatus::Superseded, ClaimPredicate::Is),
            claim("expired", ClaimStatus::Verified, ClaimPredicate::Is),
        ];
        let validities = vec![
            Validity::default(),
            Validity {
                valid_from: None,
                valid_until: Some("2026-05-01 00:00:00"),
            },
        ];
        let resolved = resolve_current(&claims, &validities, NOW);
        let old = resolved.iter().find(|c| c.id.as_str() == "old").unwrap();
        assert_eq!(old.lifecycle, Lifecycle::Superseded);
        let expired = resolved
            .iter()
            .find(|c| c.id.as_str() == "expired")
            .unwrap();
        assert_eq!(expired.lifecycle, Lifecycle::Superseded);
    }

    #[test]
    fn missing_time_bounds_are_treated_as_always_valid() {
        assert!(is_temporally_valid(None, None, NOW));
        assert!(is_temporally_valid(Some("2020-01-01 00:00:00"), None, NOW));
        assert!(is_temporally_valid(Some(""), Some(""), NOW));
    }

    #[test]
    fn future_from_and_past_until_are_both_invalid() {
        assert!(!is_temporally_valid(Some("2027-01-01 00:00:00"), None, NOW));
        assert!(!is_temporally_valid(None, Some("2026-01-01 00:00:00"), NOW));
        assert!(!is_temporally_valid(None, Some(NOW), NOW));
    }

    #[test]
    fn rejected_and_archived_claims_are_never_returned() {
        let claims = vec![
            claim("r", ClaimStatus::Rejected, ClaimPredicate::Uses),
            claim("a", ClaimStatus::Archived, ClaimPredicate::Uses),
            claim("d", ClaimStatus::Draft, ClaimPredicate::Uses),
        ];
        let validities = vec![Validity::default(), Validity::default(), Validity::default()];
        assert!(resolve_current(&claims, &validities, NOW).is_empty());
    }

    #[test]
    fn candidate_and_verified_are_both_current() {
        let claims = vec![
            claim("c", ClaimStatus::Candidate, ClaimPredicate::Uses),
            claim("v", ClaimStatus::Verified, ClaimPredicate::Supports),
        ];
        let validities = vec![Validity::default(), Validity::default()];
        let resolved = resolve_current(&claims, &validities, NOW);
        assert_eq!(resolved.len(), 2);
        assert!(resolved.iter().all(|c| c.is_current()));
    }
}
