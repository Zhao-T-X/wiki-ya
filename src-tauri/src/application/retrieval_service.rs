//! 混合检索（Phase 6，先落地词法，语义由 semantic-retrieval 成员增强）。
//!
//! 现有 `search_service` 已是纯词法（FTS5 + LIKE 兜底）；本服务把它映射成
//! 与 `ContextKind` 对齐的 `RetrievedPassage`，供 Ask 的上下文编排消费。
//! 语义路径（embeddings + RRF）作为后续增强，不破坏本文件对外签名。

use rusqlite::Connection;

use crate::ai::accounting::TokenUsage;
use crate::ai::config::AiConfig;
use crate::ai::context::ContextKind;
use crate::ai::provider;
use crate::application::dto::{SearchInput, SearchResponse};
use crate::application::search_service;
use crate::error::AppResult;
use crate::infrastructure::embedding_repository;

/// 一条检索到的原文片段，已与 `ContextKind` 对齐。
#[derive(Debug, Clone)]
pub struct RetrievedPassage {
    pub kind: ContextKind,
    pub id: String,
    pub title: String,
    pub content: String,
    pub score: f32,
    pub source_id: Option<String>,
}

/// 混合检索入口（稳定契约：只返回片段，不暴露用量）。
///
/// `use_semantic` 为 true 且 embeddings 可用时走语义 + 词法 RRF 融合；
/// 否则（或语义不可用时）诚实降级为纯词法。
pub fn retrieve(
    conn: &Connection,
    query: &str,
    limit: usize,
    use_semantic: bool,
) -> AppResult<Vec<RetrievedPassage>> {
    retrieve_with_usage(conn, query, limit, use_semantic).map(|(passages, _)| passages)
}

/// 混合检索入口 + 真实 embedding 用量（PR-06）。
///
/// 与 [`retrieve`] 行为完全一致，额外返回本次语义检索消耗的真实
/// embedding token（来自 provider 的 `usage`）；语义路径未触发/降级时为 0。
pub fn retrieve_with_usage(
    conn: &Connection,
    query: &str,
    limit: usize,
    use_semantic: bool,
) -> AppResult<(Vec<RetrievedPassage>, TokenUsage)> {
    // 词法检索始终执行，作为保底与融合基座。
    let lexical = lexical_search(conn, query, limit)?;

    if use_semantic {
        if let Ok((semantic, usage)) = semantic_search_with_usage(conn, query, limit) {
            return Ok((fuse_rrf(lexical, semantic, limit), usage));
        }
    }
    Ok((lexical, TokenUsage::default()))
}

/// 词法检索（现有 search_service）。
pub fn lexical_search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> AppResult<Vec<RetrievedPassage>> {
    let input = SearchInput {
        query: query.to_string(),
        limit: Some(limit),
        semantic: Some(false),
        kinds: None,
    };
    let response: SearchResponse = search_service::search(conn, input)?;
    Ok(map_hits(response))
}

/// 语义检索：向量化全部 chunk → 存库 → 向量化 query → 暴力余弦近邻。
///
/// 诚实优先：无 API Key / embed 报错 / 空库 / query 向量为空，一律返回
/// `Ok(Vec::new())`，让 `retrieve` 静默回退到词法，绝不伪造相似度。
pub fn semantic_search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> AppResult<Vec<RetrievedPassage>> {
    semantic_search_with_usage(conn, query, limit).map(|(passages, _)| passages)
}

/// 语义检索 + 真实 embedding 用量（PR-06）。
///
/// 失败路径（无 Key / embed 报错 / 空库）统一降级为 `(Vec::new(), 默认用量)`：
/// 报错时我们无法确知已消耗多少 token，故记 0（诚实，不猜测）。
pub fn semantic_search_with_usage(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> AppResult<(Vec<RetrievedPassage>, TokenUsage)> {
    let mut usage = TokenUsage::default();
    let mut run = || -> AppResult<Vec<RetrievedPassage>> {
        let config = AiConfig::from_settings(conn);
        let provider = provider::default_provider(conn);

        // PERF-01：只向量化「当前模型下还没有向量」的 chunk。
        // 稳态（无新增/变更 chunk）时这里返回空集 → 完全跳过 embedding，
        // 每次 Ask 的向量化成本从 O(库规模) 降到 O(1)（仅 query 一条）。
        let missing = embedding_repository::chunks_missing_embedding(conn, &config.embedding_model)?;

        // 分批向量化：单次请求过大既会超 API 上下文，也会撞速率限制。
        // 逐批入库（chunk_id 主键幂等），中途失败时已写入的批次仍然有效。
        for batch in batch_for_embedding(&missing) {
            let texts: Vec<String> = batch.iter().map(|(_, c)| c.clone()).collect();
            let embedded = provider.embed(&texts)?;
            usage.embedding_tokens += embedded.prompt_tokens as u64;
            if embedded.vectors.len() != batch.len() {
                // 诚实降级：返回数量与请求不符说明端点不可信，宁可不用语义结果，
                // 也绝不写出错位/错误的向量。
                return Ok(Vec::new());
            }
            for ((chunk_id, _), vector) in batch.iter().zip(embedded.vectors.iter()) {
                embedding_repository::store_embedding(
                    conn,
                    chunk_id,
                    vector,
                    &config.embedding_model,
                )?;
            }
        }

        // 空库直接降级：没有 chunk 可比，就不为 query 付一次 embedding 成本。
        if !embedding_repository::has_chunks(conn)? {
            return Ok(Vec::new());
        }

        // 向量化 query。
        let embedded_query = provider.embed(&vec![query.to_string()])?;
        usage.embedding_tokens += embedded_query.prompt_tokens as u64;
        let query_vec = match embedded_query.vectors.into_iter().next() {
            Some(v) if !v.is_empty() => v,
            _ => return Ok(Vec::new()),
        };

        embedding_repository::nearest_chunks(conn, &config.embedding_model, &query_vec, limit)
    };

    match run() {
        Ok(passages) => Ok((passages, usage)),
        Err(_) => Ok((Vec::new(), TokenUsage::default())),
    }
}

/// 向量化批次上限：条数 + 字符双预算（PERF-01）。
const EMBED_BATCH_ITEMS: usize = 64;
const EMBED_BATCH_CHARS: usize = 60_000;

/// 把待向量化条目按「条数 + 字符」双预算切批（与 `ai_service::batch_chunks` 同思路）。
fn batch_for_embedding<'a>(items: &'a [(String, String)]) -> Vec<Vec<&'a (String, String)>> {
    let mut out: Vec<Vec<&'a (String, String)>> = Vec::new();
    let mut current: Vec<&'a (String, String)> = Vec::new();
    let mut chars = 0usize;
    for item in items {
        let len = item.1.chars().count();
        if !current.is_empty()
            && (current.len() >= EMBED_BATCH_ITEMS || chars + len > EMBED_BATCH_CHARS)
        {
            out.push(std::mem::take(&mut current));
            chars = 0;
        }
        current.push(item);
        chars += len;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// RRF（Reciprocal Rank Fusion）融合词法与语义结果（TDD §2119）。
///
/// score = Σ 1/(rank + k)，k=60；按 id 去重（语义 content 优先，落空时取词法）；
/// 最终按融合分数降序取前 `limit` 条。
fn fuse_rrf(
    lexical: Vec<RetrievedPassage>,
    semantic: Vec<RetrievedPassage>,
    limit: usize,
) -> Vec<RetrievedPassage> {
    const K: f32 = 60.0;

    // id -> 融合分数。
    let mut scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    // id -> 去重后的命中（语义优先填充 content）。
    let mut merged: std::collections::HashMap<String, RetrievedPassage> =
        std::collections::HashMap::new();

    // 先放语义：决定 content 优先级。
    for (rank, hit) in semantic.iter().enumerate() {
        *scores.entry(hit.id.clone()).or_insert(0.0) += 1.0 / (rank as f32 + K);
        merged.entry(hit.id.clone()).or_insert_with(|| hit.clone());
    }
    // 再放词法：仅补充语义未覆盖的 id。
    for (rank, hit) in lexical.iter().enumerate() {
        *scores.entry(hit.id.clone()).or_insert(0.0) += 1.0 / (rank as f32 + K);
        merged.entry(hit.id.clone()).or_insert_with(|| hit.clone());
    }

    let mut result: Vec<RetrievedPassage> = merged
        .into_iter()
        .map(|(id, mut passage)| {
            passage.score = scores[&id];
            passage
        })
        .collect();

    result.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result.into_iter().take(limit).collect()
}

/// 把 SearchResponse 的命中映射成 RetrievedPassage。
pub fn map_hits(response: SearchResponse) -> Vec<RetrievedPassage> {
    response
        .hits
        .into_iter()
        .filter_map(|hit| {
            let kind = ContextKind::from_search_kind(&hit.kind)?;
            let source_id = hit.document_id.or(hit.claim_id).or(hit.entity_id);
            Some(RetrievedPassage {
                kind,
                id: hit.id,
                title: hit.title,
                content: hit.snippet,
                score: hit.score,
                source_id,
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize, len: usize) -> Vec<(String, String)> {
        (0..n)
            .map(|i| (format!("c{i:03}"), "x".repeat(len)))
            .collect()
    }

    /// PERF-01：分批不得丢条目——丢了就等于静默漏算向量，检索会悄悄变差。
    #[test]
    fn batching_never_drops_items() {
        for (n, len) in [(0usize, 10usize), (1, 10), (10, 10), (200, 10), (3, 50_000)] {
            let input = items(n, len);
            let batches = batch_for_embedding(&input);
            let flattened: Vec<&(String, String)> = batches.iter().flatten().copied().collect();
            assert_eq!(flattened.len(), n, "n={n} len={len} 时条目数必须守恒");
            for (i, item) in flattened.iter().enumerate() {
                assert_eq!(item.0, input[i].0, "顺序必须保持");
            }
        }
    }

    /// PERF-01：单批不得超过条数/字符预算。
    #[test]
    fn batching_respects_budgets() {
        let input = items(200, 1_000);
        for batch in batch_for_embedding(&input) {
            assert!(batch.len() <= EMBED_BATCH_ITEMS, "条数超限");
            let chars: usize = batch.iter().map(|(_, c)| c.chars().count()).sum();
            assert!(chars <= EMBED_BATCH_CHARS, "字符预算超限：{chars}");
        }
        // 200 条 × 1000 字 = 20 万字符 → 至少切成 4 批
        assert!(batch_for_embedding(&input).len() >= 4);
    }
}
