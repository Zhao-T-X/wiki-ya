//! 检索融合与预算。

use std::collections::HashMap;

use crate::string_enum;

string_enum! {
    /// 检索方式。UI 用它解释「这个结果是怎么来的」。
    pub enum SearchMethod {
        /// 词法检索（FTS5 / trigram）。无 AI 也能用，是 Local-first 的底线。
        Lexical => "lexical",
        /// 向量检索。需要嵌入模型。
        Semantic => "semantic",
        /// 多路融合（RRF）。
        Hybrid => "hybrid",
    }
}

string_enum! {
    /// 可检索的对象类型。
    pub enum SearchHitKind {
        Document => "document",
        Chunk => "chunk",
        Claim => "claim",
        Entity => "entity",
    }
}

/// RRF 的平滑常数 `k`。
///
/// 60 是参考实现使用的值：它让排名 1 与排名 2 的得分差异很小
/// （1/61 vs 1/62），从而避免任何单一召回通道主导结果。
/// 换值会显著改变排序，因此**不是随手可调的参数**。
pub const RRF_K: f32 = 60.0;

/// 一路召回中的一个结果。
#[derive(Debug, Clone, PartialEq)]
pub struct RankedItem {
    pub id: String,
    pub kind: SearchHitKind,
    pub title: String,
    pub snippet: String,
}

/// RRF 融合。
///
/// 每路结果按**自身排名**贡献 `1/(k + rank)`，与各路的原始分数无关——
/// 这正是它能把 BM25 分数与余弦相似度这种量纲完全不同的信号
/// 放在一起排序的原因。
pub fn rrf_fuse(result_sets: &[Vec<RankedItem>], limit: usize) -> Vec<(RankedItem, f32)> {
    let mut scores: HashMap<&str, f32> = HashMap::new();
    let mut items: HashMap<&str, &RankedItem> = HashMap::new();

    for set in result_sets {
        for (index, item) in set.iter().enumerate() {
            let rank = index + 1;
            *scores.entry(item.id.as_str()).or_insert(0.0) += 1.0 / (RRF_K + rank as f32);
            // 同一对象可能在多路出现：保留第一次见到的展示信息即可，
            // 因为 id 相同意味着内容相同。
            items.entry(item.id.as_str()).or_insert(item);
        }
    }

    let mut merged: Vec<(&str, f32)> = scores.into_iter().collect();
    merged.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(right.0)) // 分数相同时按 id 稳定排序
    });

    merged
        .into_iter()
        .take(limit)
        .filter_map(|(id, score)| items.get(id).map(|item| ((*item).clone(), score)))
        .collect()
}

/// 检索预算 —— 检索**不允许**返回无限结果（TDD §32）。
///
/// 这不只是性能考虑：没有预算，一个宽泛的查询会把上下文塞满，
/// 直接违背 Context Efficiency 的目标。
#[derive(Debug, Clone, Copy)]
pub struct RetrievalBudget {
    pub max_candidates: usize,
    pub max_claims: usize,
    pub max_evidence: usize,
    pub max_tokens: usize,
}

impl Default for RetrievalBudget {
    fn default() -> Self {
        RetrievalBudget {
            max_candidates: 40,
            max_claims: 20,
            max_evidence: 8,
            max_tokens: 4_000,
        }
    }
}

impl RetrievalBudget {
    /// 按预算裁剪候选数量。
    pub fn clamp_candidates(&self, requested: usize) -> usize {
        requested.min(self.max_candidates).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> RankedItem {
        RankedItem {
            id: id.to_string(),
            kind: SearchHitKind::Chunk,
            title: id.to_string(),
            snippet: String::new(),
        }
    }

    #[test]
    fn rrf_scores_use_the_documented_constant() {
        let fused = rrf_fuse(&[vec![item("a")]], 10);
        let expected = 1.0 / (RRF_K + 1.0);
        assert!((fused[0].1 - expected).abs() < 1e-6);
    }

    #[test]
    fn appearing_in_two_channels_ranks_above_appearing_in_one() {
        let lexical = vec![item("both"), item("lexical-only")];
        let semantic = vec![item("both"), item("semantic-only")];
        let fused = rrf_fuse(&[lexical, semantic], 10);
        assert_eq!(fused[0].0.id, "both");
    }

    #[test]
    fn lower_rank_in_one_channel_can_be_beaten_by_two_channel_presence() {
        // top-of-one vs second-in-two：这正是不让单一通道主导的意义。
        let lexical = vec![item("only-top")];
        let semantic = vec![item("first"), item("second")];
        let lexical2 = vec![item("second")];
        let fused = rrf_fuse(&[lexical, semantic, lexical2], 10);
        assert_eq!(fused[0].0.id, "second");
    }

    #[test]
    fn limit_is_respected_and_order_is_stable() {
        let fused = rrf_fuse(&[vec![item("a"), item("b"), item("c")]], 2);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].0.id, "a");
        assert_eq!(fused[1].0.id, "b");
    }

    #[test]
    fn empty_input_is_handled() {
        assert!(rrf_fuse(&[], 5).is_empty());
        assert!(rrf_fuse(&[vec![]], 5).is_empty());
    }

    #[test]
    fn budget_clamps_requested_candidates_and_never_returns_zero() {
        let budget = RetrievalBudget::default();
        assert_eq!(budget.clamp_candidates(1_000), budget.max_candidates);
        assert_eq!(budget.clamp_candidates(0), 1);
        assert_eq!(budget.clamp_candidates(5), 5);
    }

    #[test]
    fn methods_and_kinds_match_the_ipc_contract() {
        assert_eq!(SearchMethod::Lexical.as_str(), "lexical");
        assert_eq!(SearchMethod::Hybrid.as_str(), "hybrid");
        assert_eq!(SearchHitKind::Claim.as_str(), "claim");
        assert_eq!(SearchHitKind::ALL.len(), 4);
    }
}
