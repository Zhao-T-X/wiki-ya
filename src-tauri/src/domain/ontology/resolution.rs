//! Entity Resolution —— 把表面字符串折到同一个实体上。
//!
//! 顺序是固定的，且**确定性级别从高到低**（决策 D8：形态归一先于别名查表，
//! 命中率更高）：
//!
//! ```text
//! Exact → Normalized → Alias → Fuzzy → Semantic
//! ```
//!
//! 前四级是确定性的、可复现的；第五级（Semantic）**只能产出一个
//! `EntityMergeProposal` 提案，绝不能直接合并**——合并两个不该合并的实体，
//! 是这类系统最难修复的破坏。
//!
//! 本模块只做**纯计算**（归一化、相似度、策略）；真正的数据库查表
//! 由 application 层的用例编排，这样 Domain 不必知道 SQLite。

use crate::string_enum;

pub use crate::domain::ontology::predicate::normalize_name;

string_enum! {
    /// 消解级别，从最确定到最不确定。
    pub enum EntityResolutionStep {
        Exact => "exact",
        Normalized => "normalized",
        Alias => "alias",
        Fuzzy => "fuzzy",
        Semantic => "semantic",
    }
}

impl EntityResolutionStep {
    /// 该级别是否安全到可以自动合并。
    ///
    /// 只有精确与归一化命中才允许直接复用已有实体；
    /// 别名命中允许复用（别名本身就是人工维护的等价声明），
    /// 而模糊与语义一律只能提案。
    pub fn allows_automatic_merge(&self) -> bool {
        matches!(
            self,
            EntityResolutionStep::Exact | EntityResolutionStep::Normalized
        )
    }

    /// 该级别是否必须进入人工审核。
    pub fn requires_review(&self) -> bool {
        matches!(
            self,
            EntityResolutionStep::Fuzzy | EntityResolutionStep::Semantic
        )
    }
}

/// 模糊匹配阈值。
///
/// 0.82 是刻意偏保守的值：宁可多留一个人工确认，
/// 也不要把 "Apple" 和 "Apple Pie" 折成一个实体。
pub const FUZZY_THRESHOLD: f32 = 0.82;

/// 短于该长度不做模糊匹配。
///
/// 三个字符的两串字在 Dice 系数下极易达到高分（"Rust" vs "Rus"），
/// 但它们的语义差别可以极大。
pub const MIN_FUZZY_NAME_CHARS: usize = 5;

/// 二元组 Dice 系数（纯确定性、无依赖）。
///
/// 选它而不是编辑距离的理由：对"词序调换"和"多词插入"更宽容
/// （`async fn in trait` vs `async fn in traits`），而这两类恰是
/// 技术名词最常见的写法差异。
pub fn similarity(left: &str, right: &str) -> f32 {
    let a = normalize_name(left);
    let b = normalize_name(right);
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let bigrams_a = bigrams(&a);
    let bigrams_b = bigrams(&b);
    if bigrams_a.is_empty() || bigrams_b.is_empty() {
        return 0.0;
    }

    let mut remaining = bigrams_b.clone();
    let mut intersection = 0usize;
    for gram in &bigrams_a {
        if let Some(index) = remaining.iter().position(|g| g == gram) {
            remaining.remove(index);
            intersection += 1;
        }
    }

    2.0 * intersection as f32 / (bigrams_a.len() + bigrams_b.len()) as f32
}

/// 是否值得作为模糊候选交给人工确认。
pub fn is_fuzzy_candidate(left: &str, right: &str) -> bool {
    let normalized_left = normalize_name(left);
    let normalized_right = normalize_name(right);
    if normalized_left.chars().count() < MIN_FUZZY_NAME_CHARS
        || normalized_right.chars().count() < MIN_FUZZY_NAME_CHARS
    {
        return false;
    }
    similarity(&normalized_left, &normalized_right) >= FUZZY_THRESHOLD
}

fn bigrams(value: &str) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 2 {
        return vec![value.to_string()];
    }
    chars
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_aliases_can_auto_merge_but_fuzzy_cannot() {
        assert!(EntityResolutionStep::Exact.allows_automatic_merge());
        assert!(EntityResolutionStep::Normalized.allows_automatic_merge());
        assert!(!EntityResolutionStep::Fuzzy.allows_automatic_merge());
        assert!(EntityResolutionStep::Semantic.requires_review());
    }

    #[test]
    fn normalisation_collapses_whitespace_and_case() {
        assert_eq!(normalize_name("  Rust  Language "), "rust language");
        assert_eq!(
            normalize_name("Async  FN in Trait"),
            "async fn in trait"
        );
    }

    #[test]
    fn word_order_variation_still_scores_high() {
        let score = similarity("async fn in trait", "async fn in traits");
        assert!(score >= FUZZY_THRESHOLD, "score was {score}");
    }

    #[test]
    fn short_names_never_fuzzy_match() {
        // "Rust" 与 "Rus" 形态接近，但绝不允许自动归一。
        assert!(!is_fuzzy_candidate("Rust", "Rus"));
        assert!(!is_fuzzy_candidate("C", "C++"));
    }

    #[test]
    fn clearly_different_names_are_not_candidates() {
        assert!(!is_fuzzy_candidate("SQLite", "PostgreSQL"));
        assert!(!is_fuzzy_candidate("Apple", "Apple Pie Inc"));
    }

    #[test]
    fn identical_names_score_one() {
        assert_eq!(similarity("Rust", "rust"), 1.0);
        assert_eq!(similarity("", "rust"), 0.0);
    }
}
