//! Search 用例 —— 多路召回 + RRF 融合。
//!
//! Phase 1 的可用通道只有**词法**（FTS5 trigram 与 LIKE 兜底），
//! 因此 `method` 永远如实报告为 `lexical`。用户显式请求语义检索时
//! 返回 `notice` 说明原因，而**不是**报错、也不是假装做过了。

use std::collections::HashMap;
use std::time::Instant;

use rusqlite::Connection;

use crate::application::dto::{SearchHit, SearchInput, SearchResponse};
use crate::domain::search::search::{rrf_fuse, RankedItem, RetrievalBudget, SearchHitKind};
use crate::error::{AppError, AppResult};
use crate::infrastructure::fts_repository::{self, SearchHitRow};

/// 解析请求的对象类型；未指定时检索全部**已实现**的类型。
///
/// `chunk` 是契约里合法的取值，但切片级检索要等向量检索（Phase 3/6）
/// 才有意义——单靠词法，切片要么与文档结果重复，要么淹没有效信息。
/// 因此显式请求 chunk 时给出说明而不是返回空结果。
fn parse_kinds(raw: Option<&[String]>) -> AppResult<Vec<SearchHitKind>> {
    let Some(values) = raw else {
        return Ok(vec![
            SearchHitKind::Document,
            SearchHitKind::Claim,
            SearchHitKind::Entity,
        ]);
    };
    if values.is_empty() {
        return Ok(vec![
            SearchHitKind::Document,
            SearchHitKind::Claim,
            SearchHitKind::Entity,
        ]);
    }

    let mut kinds = Vec::new();
    for value in values {
        let parsed: SearchHitKind = value.trim().parse()?;
        if !kinds.contains(&parsed) {
            kinds.push(parsed);
        }
    }
    if kinds.is_empty() {
        return Err(AppError::Invalid("kinds 不能为空".into()));
    }
    Ok(kinds)
}

/// 执行检索。
pub fn search(conn: &Connection, input: SearchInput) -> AppResult<SearchResponse> {
    let query = input.query.trim().to_string();
    let budget = RetrievalBudget::default();
    let limit = budget.clamp_candidates(input.limit.unwrap_or(20));

    if query.is_empty() {
        return Ok(SearchResponse {
            query,
            took_ms: 0,
            method: "lexical".to_string(),
            total: 0,
            hits: Vec::new(),
            notice: None,
        });
    }

    let requested_semantic = input.semantic.unwrap_or(false);
    let requested_kinds = parse_kinds(input.kinds.as_deref())?;
    let chunk_requested = requested_kinds.contains(&SearchHitKind::Chunk);

    let started = Instant::now();

    let mut channels: Vec<Vec<RankedItem>> = Vec::new();
    let mut store: HashMap<String, SearchHit> = HashMap::new();

    for kind in &requested_kinds {
        let rows: Vec<SearchHitRow> = match kind {
            SearchHitKind::Document => fts_repository::search_documents(conn, &query, limit)?,
            SearchHitKind::Claim => fts_repository::search_claims(conn, &query, limit)?,
            SearchHitKind::Entity => fts_repository::search_entities(conn, &query, limit)?,
            SearchHitKind::Chunk => continue,
        };
        if rows.is_empty() {
            continue;
        }

        // 计数用的键是"类型 + id"：同一个 id 在不同类型里是不同对象，
        // 不区分会让 RRF 把文档与实体的分数错误地合并。
        let mut channel: Vec<RankedItem> = Vec::with_capacity(rows.len());
        for row in rows {
            let key = format!("{}:{}", row.kind.as_str(), row.id);
            channel.push(RankedItem {
                id: key.clone(),
                kind: row.kind,
                title: row.title.clone(),
                snippet: row.snippet.clone(),
            });
            store.insert(
                key.clone(),
                SearchHit {
                    kind: row.kind.as_str().to_string(),
                    id: row.id,
                    title: row.title,
                    snippet: row.snippet,
                    score: row.score,
                    method: "lexical".to_string(),
                    matched_in: row.matched_in,
                    document_id: row.document_id,
                    claim_id: row.claim_id,
                    entity_id: row.entity_id,
                },
            );
        }

        // 每一路内部先按自身得分排序，RRF 只消费排名。
        channel.sort_by(|left, right| {
            let left_score = store.get(&left.id).map(|hit| hit.score).unwrap_or(0.0);
            let right_score = store.get(&right.id).map(|hit| hit.score).unwrap_or(0.0);
            right_score
                .partial_cmp(&left_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.id.cmp(&right.id))
        });
        channels.push(channel);
    }

    let fused = rrf_fuse(&channels, limit);
    let mut hits: Vec<SearchHit> = Vec::with_capacity(fused.len());
    for (item, score) in fused {
        if let Some(mut hit) = store.remove(&item.id) {
            hit.score = (score * 1000.0).round() / 1000.0;
            hits.push(hit);
        }
    }

    let took_ms = started.elapsed().as_millis() as u64;
    let total = hits.len() as i64;

    let mut notices: Vec<String> = Vec::new();
    if requested_semantic {
        notices.push(
            "语义检索需要 AI Runtime（Phase 6），本次结果全部来自词法检索。".to_string(),
        );
    }
    if chunk_requested {
        notices.push(
            "切片级检索将随向量检索一起提供（Phase 3/6）；当前可按文档级结果查看对应切片。"
                .to_string(),
        );
    }

    Ok(SearchResponse {
        query,
        took_ms,
        // 所有通道都是词法的，因此排名融合不改变检索方式的诚实汇报。
        method: "lexical".to_string(),
        total,
        hits,
        notice: if notices.is_empty() {
            None
        } else {
            Some(notices.join(" "))
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::capture_service;
    use crate::application::dto::CreateDocumentInput;
    use crate::infrastructure::db::tests::memory_db;

    fn seed(conn: &mut Connection) {
        capture_service::create_document(
            conn,
            CreateDocumentInput {
                title: "Rust async".into(),
                content: "Rust 支持 async fn in trait。".into(),
                source_type: None,
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
        capture_service::create_document(
            conn,
            CreateDocumentInput {
                title: "苹果的SEO是乔布斯".into(),
                content: "与关键词无关的正文。".into(),
                source_type: None,
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
    }

    fn input(query: &str) -> SearchInput {
        SearchInput {
            query: query.into(),
            limit: None,
            semantic: None,
            kinds: None,
        }
    }

    #[test]
    fn empty_queries_return_an_empty_response_not_everything() {
        let mut conn = memory_db();
        seed(&mut conn);
        let response = search(&conn, input("   ")).unwrap();
        assert_eq!(response.total, 0);
        assert!(response.hits.is_empty());
    }

    #[test]
    fn english_and_chinese_substring_queries_both_work() {
        let mut conn = memory_db();
        seed(&mut conn);

        let english = search(&conn, input("async")).unwrap();
        assert_eq!(english.total, 1);
        assert_eq!(english.hits[0].kind, "document");
        assert_eq!(english.hits[0].method, "lexical");

        let chinese = search(&conn, input("苹果")).unwrap();
        assert_eq!(chinese.total, 1);
        assert!(chinese.hits[0].matched_in.contains(&"title".to_string()));
    }

    #[test]
    fn method_is_reported_honestly_as_lexical() {
        let mut conn = memory_db();
        seed(&mut conn);
        let response = search(&conn, input("async")).unwrap();
        assert_eq!(response.method, "lexical");
        assert!(response.notice.is_none());
    }

    #[test]
    fn requesting_semantic_degrades_with_an_explanation_instead_of_failing() {
        let mut conn = memory_db();
        seed(&mut conn);
        let mut request = input("async");
        request.semantic = Some(true);

        let response = search(&conn, request).unwrap();
        assert_eq!(response.method, "lexical");
        assert_eq!(response.total, 1, "降级后仍应返回词法结果");
        assert!(response.notice.unwrap().contains("Phase 6"));
    }

    #[test]
    fn chunk_kind_is_explained_rather_than_silently_empty() {
        let mut conn = memory_db();
        seed(&mut conn);
        let mut request = input("async");
        request.kinds = Some(vec!["chunk".into()]);

        let response = search(&conn, request).unwrap();
        assert_eq!(response.total, 0);
        assert!(response.notice.unwrap().contains("切片级检索"));
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        let mut conn = memory_db();
        seed(&mut conn);
        let mut request = input("async");
        request.kinds = Some(vec!["nonsense".into()]);
        assert_eq!(
            search(&conn, request).unwrap_err().code(),
            "DOMAIN_RULE_VIOLATION"
        );
    }

    #[test]
    fn limit_is_clamped_by_the_retrieval_budget() {
        let mut conn = memory_db();
        for index in 0..60 {
            capture_service::create_document(
                &mut conn,
                CreateDocumentInput {
                    title: format!("note {index}"),
                    content: format!("shared keyword number {index}"),
                    source_type: None,
                    source_uri: None,
                    metadata: None,
                },
            )
            .unwrap();
        }
        let mut request = input("shared");
        request.limit = Some(1_000);
        let response = search(&conn, request).unwrap();
        assert_eq!(
            response.total as usize,
            RetrievalBudget::default().max_candidates
        );
    }

    #[test]
    fn results_are_ordered_by_fused_score_and_report_positive_scores() {
        let mut conn = memory_db();
        seed(&mut conn);
        let response = search(&conn, input("Rust")).unwrap();
        assert!(!response.hits.is_empty());
        for pair in response.hits.windows(2) {
            assert!(pair[0].score >= pair[1].score);
        }
        assert!(response.hits.iter().all(|hit| hit.score > 0.0));
        assert!(response.hits.iter().all(|hit| hit.kind == "document"));
    }
}
