//! 检索实现：FTS5（trigram）优先，词法 LIKE 兜底。
//!
//! ## 为什么必须有兜底
//!
//! trigram 索引是**三字符滑窗**：查询短于 3 个字符时它匹配不到任何东西
//! （搜「AI」直接空）。这不是缺陷，而是 trigram 的固有性质。因此
//! 「FTS 无结果」不等于「知识库里没有」，必须再用 LIKE 复核一次，
//! 否则用户会以为自己的知识丢了。
//!
//! 语义检索（向量）属 Phase 6：`semantic: true` 在这里降级为词法，
//! 并如实把它报告成 `lexical`（`docs/IPC契约.md` §1.4）。

use rusqlite::types::Value;
use rusqlite::{params, Connection};

use crate::domain::search::search::SearchHitKind;
use crate::error::AppResult;

/// 一条检索结果。
#[derive(Debug, Clone)]
pub struct SearchHitRow {
    pub kind: SearchHitKind,
    pub id: String,
    pub title: String,
    pub snippet: String,
    /// 越大越相关。
    pub score: f32,
    /// 命中的字段名——让 UI 能解释「为什么这条匹配了」。
    pub matched_in: Vec<String>,
    pub document_id: Option<String>,
    pub claim_id: Option<String>,
    pub entity_id: Option<String>,
}

/// 把用户输入包成 FTS5 短语查询。
///
/// 不这么做的话，`async (fn)` 里的括号会被当成 FTS5 语法，
/// 直接抛错——用户只是搜了个带标点的短语，不该看到语法错误。
/// 内部的双引号按 FTS5 规则转义为两个双引号。
pub fn fts_query(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(format!("\"{}\"", trimmed.replace('"', "\"\"")))
}

/// 查询分词（中英文混合）。
///
/// 中文没有空格，按空白切会把整个问题当成一个词，因此需要按字符脚本切分：
/// 连续汉字成一段，连续的字母数字成一段。
pub fn terms(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_is_cjk = false;

    for ch in raw.chars() {
        let is_cjk = ('\u{4e00}'..='\u{9fff}').contains(&ch);
        let is_word = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-');
        if is_cjk || is_word {
            if !current.is_empty() && is_cjk != current_is_cjk {
                out.push(std::mem::take(&mut current));
            }
            current_is_cjk = is_cjk;
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.retain(|term| !term.is_empty());
    out
}

/// 转义 LIKE 通配符，配合 `ESCAPE '\'` 使用。
///
/// 不转义的话，用户搜 `%` 会退化成「匹配一切」，搜 `_` 会匹配任意单字符。
fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for ch in term.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 把单个 term 包成 `%term%`（已转义）。
fn like_pattern(term: &str) -> String {
    format!("%{}%", escape_like(term))
}

/// 命中的字段名列表（按固定顺序，便于前端稳定展示）。
fn matched_fields(term_list: &[String], fields: &[(&str, &str)]) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for (name, value) in fields {
        let haystack = value.to_lowercase();
        if !haystack.is_empty() && term_list.iter().any(|t| haystack.contains(&t.to_lowercase())) {
            hits.push((*name).to_string());
        }
    }
    hits
}

fn score_from(hits: &[String], term_count: usize) -> f32 {
    if term_count == 0 {
        return 0.0;
    }
    (hits.len() as f32 / term_count as f32).min(1.0)
}

/// 检索文档（FTS5 → LIKE 兜底）。
pub fn search_documents(
    conn: &Connection,
    raw_query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHitRow>> {
    let limit = limit.clamp(1, 200) as i64;
    let term_list = terms(raw_query);

    if let Some(match_query) = fts_query(raw_query) {
        let sql = "SELECT d.id,
                          d.title,
                          snippet(documents_fts, 1, '「', '」', '…', 12) AS snip,
                          -bm25(documents_fts) AS score,
                          substr(d.content, 1, 400) AS body
                   FROM documents_fts
                   JOIN documents d ON d.rowid = documents_fts.rowid
                   WHERE documents_fts MATCH ?1
                   ORDER BY bm25(documents_fts)
                   LIMIT ?2";
        // FTS 语法错误（例如查询被用户写成奇怪的形态）不应让整个搜索失败，
        // 交给 LIKE 兜底即可。
        if let Ok(mut statement) = conn.prepare(sql) {
            let rows = statement.query_map(params![match_query, limit], |row| {
                let title: String = row.get(1)?;
                let snippet: String = row.get(2)?;
                let score: f64 = row.get(3)?;
                let body: String = row.get(4)?;
                let matched = matched_fields(
                    &term_list,
                    &[("title", title.as_str()), ("content", body.as_str())],
                );
                Ok(SearchHitRow {
                    kind: SearchHitKind::Document,
                    id: row.get(0)?,
                    title,
                    snippet,
                    score: if score.is_finite() { score as f32 } else { 0.0 },
                    matched_in: matched,
                    document_id: None,
                    claim_id: None,
                    entity_id: None,
                })
            });
            if let Ok(collected) = rows {
                let collected: Vec<SearchHitRow> = collected.filter_map(Result::ok).collect();
                if !collected.is_empty() {
                    return Ok(collected
                        .into_iter()
                        .map(|mut hit| {
                            hit.document_id = Some(hit.id.clone());
                            let hits = hit.matched_in.clone();
                            hit.score = score_from(&hits, term_list.len().max(1));
                            hit
                        })
                        .collect());
                }
            }
        }
    }

    search_documents_like(conn, raw_query, limit as usize)
}

/// LIKE 兜底：短查询、trigram 无法覆盖的形态、FTS 出错时都走这里。
pub fn search_documents_like(
    conn: &Connection,
    raw_query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHitRow>> {
    let term_list = terms(raw_query);
    if term_list.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 200) as i64;

    // 每个 term **单独绑定一个占位符**，且都必须命中 title 或 content（AND 语义）。
    // 之前所有子句共用 ?1 且绑定整句，使 AND 实际退化为「整句连续子串匹配」。
    let mut clauses: Vec<String> = Vec::with_capacity(term_list.len());
    let mut bound: Vec<Value> = Vec::with_capacity(term_list.len() + 1);
    for term in &term_list {
        let idx = bound.len() + 1;
        clauses.push(format!(
            "(d.title LIKE ?{idx} ESCAPE '\\' OR d.content LIKE ?{idx} ESCAPE '\\')"
        ));
        bound.push(Value::Text(like_pattern(term)));
    }
    let clause = clauses.join(" AND ");
    let limit_idx = bound.len() + 1;
    bound.push(Value::Integer(limit));

    let sql = format!(
        "SELECT d.id, d.title, substr(d.content, 1, 400)
         FROM documents d
         WHERE {clause}
         ORDER BY d.updated_at DESC
         LIMIT ?{limit_idx}"
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(bound), |row| {
        let title: String = row.get(1)?;
        let body: String = row.get(2)?;
        let matched = matched_fields(
            &term_list,
            &[("title", title.as_str()), ("content", body.as_str())],
        );
        Ok((row.get::<_, String>(0)?, title, body, matched))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (id, title, body, matched) = row?;
        let score = score_from(&matched, term_list.len().max(1));
        hits.push(SearchHitRow {
            kind: SearchHitKind::Document,
            document_id: Some(id.clone()),
            id,
            title,
            snippet: body,
            score,
            matched_in: matched,
            claim_id: None,
            entity_id: None,
        });
    }
    Ok(hits)
}

/// 检索 Claim。
///
/// Claim 没有独立的 FTS 索引（它们比文档短得多，LIKE 已经够快），
/// 因此这里只用词法匹配，并在 `matched_in` 里说明命中了哪个字段。
pub fn search_claims(
    conn: &Connection,
    raw_query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHitRow>> {
    let term_list = terms(raw_query);
    if term_list.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 200) as i64;

    // 同样：每个 term 独立占位符 + AND（每个词都要出现在某个字段里）+ ESCAPE。
    let mut clauses: Vec<String> = Vec::with_capacity(term_list.len());
    let mut bound: Vec<Value> = Vec::with_capacity(term_list.len() + 1);
    for term in &term_list {
        let idx = bound.len() + 1;
        clauses.push(format!(
            "(c.content LIKE ?{idx} ESCAPE '\\' OR c.object_text LIKE ?{idx} ESCAPE '\\' \
             OR s.name LIKE ?{idx} ESCAPE '\\' OR o.name LIKE ?{idx} ESCAPE '\\' \
             OR c.predicate LIKE ?{idx} ESCAPE '\\')"
        ));
        bound.push(Value::Text(like_pattern(term)));
    }
    let clause = clauses.join(" AND ");
    let limit_idx = bound.len() + 1;
    bound.push(Value::Integer(limit));

    let sql = format!(
        "SELECT c.id,
               COALESCE(NULLIF(trim(c.content), ''),
                        trim(s.name || ' ' || c.predicate || ' ' || COALESCE(o.name, c.object_text, ''))) AS title,
               COALESCE(o.name, c.object_text, '') AS object_label,
               s.name AS subject_name,
               c.predicate,
               c.content
        FROM claims c
        JOIN entities s ON s.id = c.subject_id
        LEFT JOIN entities o ON o.id = c.object_id
        WHERE c.status NOT IN ('rejected','archived')
          AND ({clause})
        ORDER BY c.created_at DESC
        LIMIT ?{limit_idx}"
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(bound), |row| {
        let title: String = row.get(1)?;
        let object_label: String = row.get(2)?;
        let subject_name: String = row.get(3)?;
        let predicate: String = row.get(4)?;
        let content: Option<String> = row.get(5)?;
        let matched = matched_fields(
            &term_list,
            &[
                ("subject", subject_name.as_str()),
                ("predicate", predicate.as_str()),
                ("object", object_label.as_str()),
                ("content", content.as_deref().unwrap_or("")),
            ],
        );
        Ok((row.get::<_, String>(0)?, title, matched))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (id, title, matched) = row?;
        let score = score_from(&matched, term_list.len().max(1));
        hits.push(SearchHitRow {
            kind: SearchHitKind::Claim,
            claim_id: Some(id.clone()),
            id,
            title,
            snippet: String::new(),
            score,
            matched_in: matched,
            document_id: None,
            entity_id: None,
        });
    }
    Ok(hits)
}

/// 检索实体（含别名）。
pub fn search_entities(
    conn: &Connection,
    raw_query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHitRow>> {
    let term_list = terms(raw_query);
    if term_list.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.clamp(1, 200) as i64;

    // 每个 term 一个（名称/描述）占位符 + 一个（别名归一化）占位符；term 之间 AND。
    let mut clauses: Vec<String> = Vec::with_capacity(term_list.len());
    let mut bound: Vec<Value> = Vec::with_capacity(term_list.len() * 2 + 1);
    for term in &term_list {
        let idx = bound.len() + 1;
        clauses.push(format!(
            "(e.name LIKE ?{idx} ESCAPE '\\' OR e.description LIKE ?{idx} ESCAPE '\\' \
             OR EXISTS (SELECT 1 FROM entity_aliases a \
                        WHERE a.entity_id = e.id AND a.alias_normalized LIKE ?{} ESCAPE '\\'))",
            idx + 1
        ));
        bound.push(Value::Text(like_pattern(term)));
        bound.push(Value::Text(like_pattern(
            &crate::domain::ontology::resolution::normalize_name(term),
        )));
    }
    let clause = clauses.join(" AND ");
    let limit_idx = bound.len() + 1;
    bound.push(Value::Integer(limit));

    let sql = format!(
        "SELECT e.id, e.name, e.primary_type, e.description,
               (SELECT group_concat(a.alias, ' / ') FROM entity_aliases a WHERE a.entity_id = e.id) AS aliases
        FROM entities e
        WHERE e.status NOT IN ('rejected','archived')
          AND ({clause})
        ORDER BY e.name
        LIMIT ?{limit_idx}"
    );

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(bound), |row| {
        let name: String = row.get(1)?;
        let type_name: String = row.get(2)?;
        let description: Option<String> = row.get(3)?;
        let aliases: Option<String> = row.get(4)?;
        let matched = matched_fields(
            &term_list,
            &[
                ("name", name.as_str()),
                ("type", type_name.as_str()),
                ("description", description.as_deref().unwrap_or("")),
                ("alias", aliases.as_deref().unwrap_or("")),
            ],
        );
        Ok((row.get::<_, String>(0)?, name, type_name, matched))
    })?;

    let mut hits = Vec::new();
    for row in rows {
        let (id, name, type_name, matched) = row?;
        let score = score_from(&matched, term_list.len().max(1));
        hits.push(SearchHitRow {
            kind: SearchHitKind::Entity,
            entity_id: Some(id.clone()),
            id,
            title: name,
            snippet: type_name,
            score,
            matched_in: matched,
            document_id: None,
            claim_id: None,
        });
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::knowledge::document::{Document, SourceType};
    use crate::infrastructure::db::tests::memory_db;
    use crate::infrastructure::{claim_repository, entity_repository};
    use crate::domain::knowledge::claim::{Claim, ClaimObject, ClaimStatus, ClaimType, Modality, Polarity};
    use crate::domain::ontology::predicate::ClaimPredicate;

    fn seed_document(conn: &Connection, title: &str, content: &str) {
        let document = Document {
            id: crate::domain::common::ids::DocumentId::new(),
            title: title.into(),
            content: content.into(),
            content_hash: Document::content_hash(content),
            source_type: SourceType::Note,
            source_uri: None,
            metadata: serde_json::json!({}),
            created_at: String::new(),
            updated_at: String::new(),
        };
        crate::infrastructure::document_repository::insert(conn, &document).unwrap();
    }

    #[test]
    fn query_is_wrapped_as_a_phrase_so_punctuation_cannot_break_fts() {
        assert_eq!(fts_query("async fn").as_deref(), Some("\"async fn\""));
        assert_eq!(fts_query("a \"quoted\" bit").as_deref(), Some("\"a \"\"quoted\"\" bit\""));
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn terms_split_by_script() {
        assert_eq!(terms("Rust 的 async fn"), vec!["Rust", "的", "async", "fn"]);
        assert_eq!(terms("苹果的SEO"), vec!["苹果的", "SEO"]);
        assert!(terms("，。！").is_empty());
    }

    #[test]
    fn english_substring_search_hits_through_trigram() {
        let conn = memory_db();
        seed_document(&conn, "Rust note", "Rust supports async fn in trait.");
        let hits = search_documents(&conn, "async", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].matched_in.contains(&"content".to_string()));
    }

    #[test]
    fn chinese_search_works_and_reports_matched_fields() {
        let conn = memory_db();
        seed_document(&conn, "苹果的SEO是乔布斯", "正文与关键词无关");
        let hits = search_documents(&conn, "苹果", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].matched_in.contains(&"title".to_string()));
    }

    #[test]
    fn short_queries_are_served_by_the_like_fallback() {
        let conn = memory_db();
        seed_document(&conn, "AI 笔记", "AI 与知识管理");
        // "AI" 只有 2 个字符，trigram 无法匹配 → 必须由 LIKE 兜底
        let hits = search_documents(&conn, "AI", 10).unwrap();
        assert_eq!(hits.len(), 1, "短查询不能因为 trigram 的限制而返回空");
    }

    #[test]
    fn like_fallback_requires_every_term_across_fields() {
        let conn = memory_db();
        seed_document(&conn, "note", "Rust and async are mentioned far apart here.");
        // 「Rust」与「async」不连续：整句子串匹配不到，但逐词 AND 应命中。
        let hits = search_documents_like(&conn, "Rust async", 10).unwrap();
        assert_eq!(hits.len(), 1, "多词应逐词匹配（AND），而非要求整句连续");
    }

    #[test]
    fn like_fallback_treats_underscore_literally() {
        let conn = memory_db();
        seed_document(&conn, "note", "snake_case identifier");
        seed_document(&conn, "other", "snakeXcase identifier");
        // 未转义时 `_` 是「任意单字符」通配符，会误命中 snakeXcase。
        let hits = search_documents_like(&conn, "snake_case", 10).unwrap();
        assert_eq!(hits.len(), 1, "下划线必须按字面量匹配");
        assert_eq!(hits[0].title, "note");
    }

    #[test]
    fn blank_queries_return_nothing_instead_of_everything() {
        let conn = memory_db();
        seed_document(&conn, "anything", "body");
        assert!(search_documents(&conn, "   ", 10).unwrap().is_empty());
        assert!(search_claims(&conn, "", 10).unwrap().is_empty());
        assert!(search_entities(&conn, "  ", 10).unwrap().is_empty());
    }

    #[test]
    fn claims_are_searchable_by_subject_predicate_and_object() {
        let conn = memory_db();
        let subject = entity_repository::resolve_or_create(&conn, "Rust").unwrap();
        let object = entity_repository::resolve_or_create(&conn, "SQLite").unwrap();
        let claim = Claim {
            id: crate::domain::common::ids::ClaimId::new(),
            subject_id: subject.id,
            predicate: ClaimPredicate::Uses,
            object: Some(ClaimObject::Entity(object.id)),
            content: None,
            context: serde_json::json!({}),
            claim_type: ClaimType::Factual,
            polarity: Polarity::Positive,
            modality: Modality::Asserted,
            condition: None,
            confidence: None,
            status: ClaimStatus::Candidate,
            valid_from: None,
            valid_until: None,
            recorded_at: String::new(),
            created_at: String::new(),
        };
        claim_repository::insert(&conn, &claim).unwrap();

        let by_object = search_claims(&conn, "SQLite", 10).unwrap();
        assert_eq!(by_object.len(), 1);
        assert!(by_object[0].matched_in.contains(&"object".to_string()));

        let by_subject = search_claims(&conn, "Rust", 10).unwrap();
        assert_eq!(by_subject.len(), 1);
    }

    #[test]
    fn entities_are_searchable_by_alias() {
        let conn = memory_db();
        let entity = entity_repository::resolve_or_create(&conn, "OpenAI").unwrap();
        entity_repository::insert_alias(&conn, &entity.id, "OpenAI Inc.").unwrap();

        let hits = search_entities(&conn, "OpenAI Inc.", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].matched_in.contains(&"alias".to_string()));
    }

    #[test]
    fn limit_is_respected() {
        let conn = memory_db();
        for index in 0..5 {
            seed_document(&conn, &format!("note {index}"), &format!("shared keyword {index}"));
        }
        assert_eq!(search_documents(&conn, "shared", 2).unwrap().len(), 2);
    }
}
