//! Current Knowledge 的**唯一入口**（ARCH-001 / K-001）。
//!
//! 「什么算当前知识」只允许有一个答案。否则 Search / Ask / Timeline / UI
//! 会各自实现一套（`status == candidate` vs `status != superseded` vs
//! "最新的一条"），最终给出互相矛盾的结论。
//!
//! 这里不新增状态：Current 仍然是一个**派生视图**，由
//! `status` + 演化覆盖 + 时间有效性共同决定（见 [`temporal`]）。

use std::collections::HashMap;

use crate::domain::evolution::conflict::ClaimView;
use crate::domain::evolution::temporal::{resolve_current, Lifecycle, ResolvedClaim, Validity};

/// 当前知识解析器。
pub struct KnowledgeResolver;

impl KnowledgeResolver {
    /// 派生「当前知识」视图（含被保留的历史条目）。
    pub fn resolve(claims: &[ClaimView], validities: &[Validity<'_>], now: &str) -> Vec<ResolvedClaim> {
        resolve_current(claims, validities, now)
    }

    /// 派生 `claim id → lifecycle` 映射，供卡片等展示层直接消费。
    ///
    /// 注意：**不在结果里的 id** 并不等于「历史」（它可能是 rejected /
    /// archived / draft 这类被排除的状态），调用方必须自行区分——这正是
    /// `knowledge_service` 里那处 `None` 分支存在的原因。
    pub fn lifecycle_map(
        claims: &[ClaimView],
        validities: &[Validity<'_>],
        now: &str,
    ) -> HashMap<String, Lifecycle> {
        Self::resolve(claims, validities, now)
            .into_iter()
            .map(|resolved| (resolved.id.into_string(), resolved.lifecycle))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::common::ids::{ClaimId, EntityId};
    use crate::domain::knowledge::claim::{ClaimObject, ClaimStatus, Polarity};
    use crate::domain::ontology::predicate::ClaimPredicate;

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

    #[test]
    fn lifecycle_map_is_the_single_source_of_current_truth() {
        let claims = vec![
            claim("old", ClaimStatus::Superseded, ClaimPredicate::Is),
            claim("new", ClaimStatus::Verified, ClaimPredicate::Is),
            claim("wrong", ClaimStatus::Rejected, ClaimPredicate::Uses),
        ];
        let validities = vec![Validity::default(); claims.len()];
        let map = KnowledgeResolver::lifecycle_map(&claims, &validities, "2026-06-01 00:00:00");

        assert_eq!(map.get("new"), Some(&Lifecycle::Current));
        // old 被 new 覆盖 → 从视图剔除（不是"历史"，因为它让位了）
        assert!(!map.contains_key("old"));
        // rejected 被排除 → 也不在 map 里（调用方须与"历史"区分）
        assert!(!map.contains_key("wrong"));
    }
}
