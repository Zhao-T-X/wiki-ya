//! 混合检索（Phase 6，先落地词法，语义由 semantic-retrieval 成员增强）。
//!
//! 现有 `search_service` 已是纯词法（FTS5 + LIKE 兜底）；本服务把它映射成
//! 与 `ContextKind` 对齐的 `RetrievedPassage`，供 Ask 的上下文编排消费。
//! 语义路径（embeddings + RRF）作为后续增强，不破坏本文件对外签名。

use rusqlite::Connection;

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

/// 混合检索入口。
///
/// `use_semantic` 为 true 且 embeddings 可用时走语义 + 词法 RRF 融合；
/// 否则（或语义不可用时）诚实降级为纯词法。函数签名是稳定契约，
/// semantic-retrieval 成员在此之内扩展，不改变对外签名与返回结构。
pub fn retrieve(
    conn: &Connection,
    query: &str,
    limit: usize,
    use_semantic: bool,
) -> AppResult<Vec<RetrievedPassage>> {
    // 词法检索始终执行，作为保底与融合基座。
    let lexical = lexical_search(conn, query, limit)?;

    if use_semantic {
        if let Ok(semantic) = semantic_search(conn, query, limit) {
            return Ok(fuse_rrf(lexical, semantic, limit));
        }
    }
    Ok(lexical)
}

/// 词法检索（现有 search_service）。
pub fn lexical_search(conn: &Connection, query: &str, limit: usize) -> AppResult<Vec<RetrievedPassage>> {
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
    // 把所有失败路径统一降级为空（保留签名与对外语义）。
    let run = || -> AppResult<Vec<RetrievedPassage>> {
        let config = AiConfig::from_settings(conn);
        let provider = provider::default_provider(conn);

        // 取全部 chunk 文本（空库直接降级）。
        let chunks = embedding_repository::all_chunk_texts(conn)?;
        if chunks.is_empty() {
            return Ok(Vec::new());
        }
        let texts: Vec<String> = chunks.iter().map(|(_, c)| c.clone()).collect();

        // 向量化全部 chunk，并覆盖写入（PRIMARY KEY 幂等）。
        let embeddings = provider.embed(&texts)?;
        if embeddings.len() != chunks.len() {
            return Ok(Vec::new());
        }
        for (chunk, vec) in chunks.iter().zip(embeddings.iter()) {
            embedding_repository::store_embedding(conn, &chunk.0, vec, &config.embedding_model)?;
        }

        // 向量化 query。
        let query_vecs = provider.embed(&vec![query.to_string()])?;
        let query_vec = match query_vecs.into_iter().next() {
            Some(v) if !v.is_empty() => v,
            _ => return Ok(Vec::new()),
        };

        embedding_repository::nearest_chunks(conn, &query_vec, limit)
    };

    Ok(run().unwrap_or_else(|_| Vec::new()))
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

    result.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
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
