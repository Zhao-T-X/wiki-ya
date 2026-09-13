//! 确定性判定的静态规则。
//!
//! 这些常量刻意**小而保守**：规则的每一处放宽都会凭空制造出
//! 需要用户处理的冲突。参考实现的原话是
//! 「Extend with evidence, not with guesses.」

use crate::domain::ontology::predicate::ClaimPredicate;

/// 单值谓语：同一主语在同一时刻只能有一个取值。
///
/// 两个不同宾语出现时，这是**知识发生了变更**，而不是两条并存的事实，
/// 因此判为 `contradicts` 并交人工判断。
///
/// 只有 3 个：`is` / `defined_as` / `classified_as`。
/// 其余谓语（包括看起来很单值的 `uses`）都退化为 `coexists` ——
/// 因为「Rust 用了 A」和「Rust 用了 B」完全可以同时成立。
pub const FUNCTIONAL_PREDICATES: &[ClaimPredicate] = &[
    ClaimPredicate::Is,
    ClaimPredicate::DefinedAs,
    ClaimPredicate::ClassifiedAs,
];

/// 是否是单值谓语。
pub fn is_functional(predicate: ClaimPredicate) -> bool {
    FUNCTIONAL_PREDICATES.contains(&predicate)
}

/// 「最值得先看的关系」排序。
///
/// 数值越小越靠前。`duplicate` 排在最前不是因为它最重要，
/// 而是因为它最便宜——用户一眼就能确认，清掉之后队列立刻变短。
pub fn review_priority(relationship: &str) -> u8 {
    match relationship {
        "duplicate" => 0,
        "supersedes" => 1,
        "contradicts" => 2,
        "coexists" => 3,
        "supplements" => 4,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_three_predicates_are_single_valued() {
        assert_eq!(FUNCTIONAL_PREDICATES.len(), 3);
        assert!(is_functional(ClaimPredicate::Is));
        assert!(is_functional(ClaimPredicate::DefinedAs));
        assert!(is_functional(ClaimPredicate::ClassifiedAs));
        // 这些看起来"单值"，但实际可以并存
        assert!(!is_functional(ClaimPredicate::Uses));
        assert!(!is_functional(ClaimPredicate::DependsOn));
        assert!(!is_functional(ClaimPredicate::Supports));
    }

    #[test]
    fn duplicate_is_reviewed_first_and_unknown_sorts_last() {
        assert!(review_priority("duplicate") < review_priority("supersedes"));
        assert!(review_priority("supersedes") < review_priority("contradicts"));
        assert!(review_priority("contradicts") < review_priority("coexists"));
        assert_eq!(review_priority("nonsense"), 5);
    }
}
