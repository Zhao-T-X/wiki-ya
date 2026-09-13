//! Knowledge 用例：实体、Claim、证据的读取，以及**手动录入 Claim**。
//!
//! 手动录入不是"临时方案"，它是 Local-first 的组成部分：没有 API Key 时
//! 用户依然必须能够建立结构化知识（PRD §44）。因此这条路径上的校验
//! 与 AI 抽取走的是**同一套**领域规则——不注册的谓语一律拒绝。

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;

use crate::application::dto::{
    ClaimCard, ClaimDetail, ClaimRelationCard, CreateClaimInput, EntityCard, EntityDetail,
    EvidenceCard, GraphEdge, GraphPayload, GraphNode, RelationCard,
};
use crate::domain::common::ids::{ClaimId, DocumentId, EntityId};
use crate::domain::evidence::evidence::{choose_level, Evidence, DEFAULT_MAX_LEVEL};
use crate::domain::evolution::conflict::ClaimView;
use crate::domain::evolution::temporal::{resolve_current, Lifecycle, Validity};
use crate::domain::graph::graph::{explore, GraphEdge as DomainGraphEdge, NodeSeed};
use crate::domain::knowledge::claim::{
    Claim, ClaimObject, ClaimStatus, ClaimType, Modality, Polarity,
};
use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::domain::ontology::resolution::normalize_name;
use crate::error::{AppError, AppResult};
use crate::infrastructure::db;
use crate::infrastructure::{
    claim_relation_repository, claim_repository, document_repository, entity_repository,
    evidence_repository, relation_repository,
};

// ---------------------------------------------------------------------------
// Claim 映射
// ---------------------------------------------------------------------------

fn to_claim_card(row: &claim_repository::ClaimRow, lifecycle: &str) -> ClaimCard {
    let object_text = match row.claim.object.as_ref() {
        Some(ClaimObject::Literal(text)) => Some(text.clone()),
        Some(ClaimObject::Number(value)) => Some(value.to_string()),
        Some(ClaimObject::Boolean(value)) => Some(value.to_string()),
        Some(ClaimObject::Date(value)) => Some(value.clone()),
        _ => None,
    };

    ClaimCard {
        id: row.claim.id.as_str().to_string(),
        subject_id: row.claim.subject_id.as_str().to_string(),
        subject_name: row.subject_name.clone(),
        predicate: row.claim.predicate.as_str().to_string(),
        object_id: match row.claim.object.as_ref() {
            Some(ClaimObject::Entity(id)) => Some(id.as_str().to_string()),
            _ => None,
        },
        object_name: row.object_name.clone(),
        object_text: row
            .object_name
            .clone()
            .or(object_text),
        content: row.claim.content.clone(),
        claim_type: row.claim.claim_type.as_str().to_string(),
        polarity: row.claim.polarity.as_str().to_string(),
        modality: row.claim.modality.as_str().to_string(),
        condition: row.claim.condition.clone(),
        confidence: row.claim.confidence,
        status: row.claim.status.as_str().to_string(),
        valid_from: row.claim.valid_from.clone(),
        valid_until: row.claim.valid_until.clone(),
        source_document_id: row.source_document_id.as_ref().map(|id| id.as_str().to_string()),
        source_document_title: row.source_document_title.clone(),
        source_quote: row.source_quote.clone(),
        evidence_count: row.evidence_count,
        created_at: row.claim.created_at.clone(),
        lifecycle: lifecycle.to_string(),
        display_text: row.display_text(),
    }
}

/// 批量把 Claim 行转成卡片，并**用领域规则派生 lifecycle**。
///
/// 派生而不是直接照抄 `status`：被取代但没有任何当前知识覆盖的 Claim
/// 依然要能被回答（「前任 CEO 是谁」），因此它的 lifecycle 虽为
/// `superseded`，却仍然出现在结果里（领域文档 §4.7）。
pub fn to_claim_cards_with_lifecycle(
    conn: &Connection,
    rows: &[claim_repository::ClaimRow],
) -> AppResult<Vec<ClaimCard>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let views: Vec<ClaimView> = rows.iter().map(|row| row.to_view()).collect();
    let validities: Vec<Validity<'_>> = rows
        .iter()
        .map(|row| Validity {
            valid_from: row.claim.valid_from.as_deref(),
            valid_until: row.claim.valid_until.as_deref(),
        })
        .collect();

    let now = db::now(conn)?;
    let derived: HashMap<String, Lifecycle> = resolve_current(&views, &validities, &now)
        .into_iter()
        .map(|resolved| (resolved.id.into_string(), resolved.lifecycle))
        .collect();

    Ok(rows
        .iter()
        .map(|row| {
            // 不在派生结果里只有两种可能，必须区分（领域文档 §2.1：历史 ≠ 错误）：
            // - status = superseded：被当前知识覆盖，确属「历史」；
            // - rejected / archived / draft：不参与检索，是「被排除」，
            //   **绝不能误标成 superseded**（否则 UI 会给错误知识打上「历史」标签）。
            let lifecycle = match derived.get(row.claim.id.as_str()) {
                Some(lifecycle) => lifecycle.as_str(),
                None if row.claim.status == ClaimStatus::Superseded => "superseded",
                None => "excluded",
            };
            to_claim_card(row, lifecycle)
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

fn to_entity_card(row: &entity_repository::EntityRow) -> EntityCard {
    EntityCard {
        id: row.entity.id.as_str().to_string(),
        name: row.entity.name.clone(),
        primary_type: row.entity.primary_type.as_str().to_string(),
        types: row
            .entity
            .types
            .iter()
            .map(|t| t.as_str().to_string())
            .collect(),
        description: row.entity.description.clone(),
        status: row.entity.status.as_str().to_string(),
        alias_count: row.alias_count,
        claim_count: row.claim_count,
    }
}

fn to_relation_card(row: &relation_repository::RelationRow) -> RelationCard {
    RelationCard {
        id: row.id.as_str().to_string(),
        source_id: row.source_id.as_str().to_string(),
        source_name: row.source_name.clone(),
        predicate: row.predicate.as_str().to_string(),
        inverse_label: crate::domain::ontology::registry::registry()
            .relation_spec(row.predicate)
            .map(|spec| spec.inverse_label.clone()),
        target_id: row.target_id.as_str().to_string(),
        target_name: row.target_name.clone(),
        confidence: row.confidence,
        status: row.status.as_str().to_string(),
    }
}

/// 实体列表。
pub fn list_entities(
    conn: &Connection,
    query: Option<&str>,
    entity_type: Option<&str>,
    limit: usize,
) -> AppResult<Vec<EntityCard>> {
    let parsed_type = match entity_type.map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(raw) => Some(EntityType::canonical(raw)?),
    };
    let rows = entity_repository::list(conn, query, parsed_type, limit)?;
    Ok(rows.iter().map(to_entity_card).collect())
}

/// 实体详情：别名 + 相关 Claim + 关系 + 邻域图。
pub fn get_entity(conn: &Connection, id: &EntityId, depth: usize) -> AppResult<EntityDetail> {
    let row = entity_repository::get_row(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("实体 {id} 不存在")))?;

    let aliases = entity_repository::list_aliases(conn, id)?;

    let claim_rows = claim_repository::list(
        conn,
        &claim_repository::ClaimFilter {
            subject_id: Some(id.clone()),
            limit: 200,
            ..Default::default()
        },
    )?;
    let claims = to_claim_cards_with_lifecycle(conn, &claim_rows)?;

    let relations: Vec<RelationCard> = relation_repository::list_for_entity(conn, id)?
        .iter()
        .map(to_relation_card)
        .collect();

    let graph = build_graph(conn, id, depth)?;

    Ok(EntityDetail {
        entity: to_entity_card(&row),
        aliases,
        claims,
        relations,
        graph,
    })
}

/// 从根实体出发逐层收集邻域（每层一次 SQL，不加载全图）。
fn build_graph(conn: &Connection, root: &EntityId, depth: usize) -> AppResult<GraphPayload> {
    let depth = depth.clamp(1, crate::domain::graph::graph::MAX_DEPTH);

    let mut collected: HashSet<String> = HashSet::new();
    collected.insert(root.as_str().to_string());
    let mut edges: Vec<DomainGraphEdge> = Vec::new();
    let mut frontier: Vec<EntityId> = vec![root.clone()];

    for _ in 0..depth {
        let mut next: Vec<EntityId> = Vec::new();
        for current in &frontier {
            for relation in relation_repository::list_for_entity(conn, current)? {
                let edge = DomainGraphEdge {
                    source: relation.source_id.as_str().to_string(),
                    target: relation.target_id.as_str().to_string(),
                    predicate: relation.predicate.as_str().to_string(),
                };
                let duplicated = edges.iter().any(|existing| {
                    existing.source == edge.source
                        && existing.target == edge.target
                        && existing.predicate == edge.predicate
                });
                if !duplicated {
                    edges.push(edge);
                }
                for candidate in [relation.source_id.clone(), relation.target_id.clone()] {
                    if collected.insert(candidate.as_str().to_string()) {
                        next.push(candidate);
                    }
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    let mut seeds: Vec<NodeSeed> = Vec::new();
    for raw_id in &collected {
        if let Some(row) = entity_repository::get_row(conn, &EntityId::from_raw(raw_id.clone()))? {
            seeds.push(NodeSeed {
                id: row.entity.id.as_str().to_string(),
                name: row.entity.name.clone(),
                type_name: row.entity.primary_type.as_str().to_string(),
                status: row.entity.status.as_str().to_string(),
            });
        }
    }

    // 布局与截断规则交给领域层，保证与 MAX_NODES 等约束同源。
    let neighborhood = explore(root.as_str(), &seeds, &edges, depth, None);

    Ok(GraphPayload {
        nodes: neighborhood
            .nodes
            .into_iter()
            .map(|node| GraphNode {
                id: node.id,
                name: node.name,
                type_name: node.type_name,
                status: node.status,
                depth: node.depth as i64,
            })
            .collect(),
        edges: neighborhood
            .edges
            .into_iter()
            .map(|edge| GraphEdge {
                source: edge.source,
                target: edge.target,
                predicate: edge.predicate,
            })
            .collect(),
        truncated: neighborhood.truncated,
    })
}

// ---------------------------------------------------------------------------
// Claim
// ---------------------------------------------------------------------------

/// Claim 列表。
pub fn list_claims(
    conn: &Connection,
    subject_id: Option<&str>,
    predicate: Option<&str>,
    status: Option<&str>,
    document_id: Option<&str>,
    limit: usize,
) -> AppResult<Vec<ClaimCard>> {
    let parsed_predicate = match predicate.map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(raw) => Some(ClaimPredicate::canonical(raw)?),
    };
    let parsed_status = match status.map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(raw) => Some(raw.parse::<ClaimStatus>()?),
    };

    let rows = claim_repository::list(
        conn,
        &claim_repository::ClaimFilter {
            subject_id: subject_id
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(EntityId::from_raw),
            predicate: parsed_predicate,
            status: parsed_status,
            document_id: document_id
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(DocumentId::from_raw),
            limit,
        },
    )?;
    to_claim_cards_with_lifecycle(conn, &rows)
}

/// Claim 详情：证据 + 演化关系 + 全部历史。
pub fn get_claim(conn: &Connection, id: &ClaimId) -> AppResult<ClaimDetail> {
    let row = claim_repository::get(conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("Claim {id} 不存在")))?;

    let cards = to_claim_cards_with_lifecycle(conn, std::slice::from_ref(&row))?;
    let card = cards
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("Claim 卡片构造失败".into()))?;

    let evidence = list_evidence(conn, id)?;
    let history = get_claim_history(conn, id)?;
    let relations: Vec<ClaimRelationCard> = history
        .iter()
        .filter(|relation| relation.status == "accepted")
        .cloned()
        .collect();

    Ok(ClaimDetail {
        claim: card,
        evidence,
        relations,
        history,
    })
}

/// 关系行 → IPC 卡片。供本模块与 review / evolution 用例共用，
/// 避免同一份映射逻辑出现三份（三份就会漂移）。
pub fn to_relation_card_dto(row: &claim_relation_repository::ClaimRelationRow) -> ClaimRelationCard {
    ClaimRelationCard {
        id: row.id.as_str().to_string(),
        source_claim_id: row.source_claim_id.as_str().to_string(),
        source_text: row.source_text.clone(),
        target_claim_id: row.target_claim_id.as_str().to_string(),
        target_text: row.target_text.clone(),
        relationship: row.relationship.as_str().to_string(),
        status: row.status.as_str().to_string(),
        confidence: row.confidence,
        reason: row.reason.clone(),
        suggested_action: row.suggested_action.clone(),
        target_previous_status: row.target_previous_status.clone(),
        created_at: row.created_at.clone(),
    }
}

/// 与某条 Claim 相关的全部演化关系。
pub fn get_claim_history(conn: &Connection, id: &ClaimId) -> AppResult<Vec<ClaimRelationCard>> {
    // 先确认 Claim 存在，避免把"不存在"渲染成"没有历史"。
    if claim_repository::get(conn, id)?.is_none() {
        return Err(AppError::NotFound(format!("Claim {id} 不存在")));
    }
    Ok(claim_relation_repository::list_for_claim(conn, id)?
        .iter()
        .map(to_relation_card_dto)
        .collect())
}

/// 某条 Claim 的证据。
pub fn list_evidence(conn: &Connection, claim_id: &ClaimId) -> AppResult<Vec<EvidenceCard>> {
    let rows = evidence_repository::list_for_claim(conn, claim_id)?;
    let mut cards = Vec::with_capacity(rows.len());
    for evidence in rows {
        let document_title = document_repository::find_by_id(conn, &evidence.document_id)?
            .map(|document| document.title);
        cards.push(EvidenceCard {
            id: evidence.id.as_str().to_string(),
            document_id: evidence.document_id.as_str().to_string(),
            document_title,
            chunk_id: evidence.chunk_id.as_ref().map(|id| id.as_str().to_string()),
            start_offset: evidence.start_offset.map(|v| v as i64),
            end_offset: evidence.end_offset.map(|v| v as i64),
            quote: evidence.quote.clone(),
            evidence_level: evidence.evidence_level.as_u8(),
            evidence_level_name: evidence.evidence_level.name().to_string(),
        });
    }
    Ok(cards)
}

/// 手动录入一条 Claim（AI 关闭时的降级路径）。
///
/// 全部领域校验都在这里执行，与 LLM 抽取路径共用同一套规则：
/// 谓语 / 类型 / 极性 / 语气 / 状态越界一律拒绝，confidence 必须落在 `[0,1]`。
///
/// 宾语的处理是 **保守** 的：只有当它已经能匹配到某个已存在的实体时才落成
/// 实体引用，否则存为自由文本。这样可以避免用户随手输入的一句话
/// 变成一个新的"实体"，把实体表污染成垃圾场；同时它会被
/// `unresolved_objects` 指标统计出来，提醒用户去归类。
pub fn create_claim(conn: &mut Connection, input: CreateClaimInput) -> AppResult<ClaimCard> {
    let predicate = ClaimPredicate::canonical(&input.predicate)?;
    let claim_type = match input.claim_type.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => ClaimType::DEFAULT,
        Some(raw) => ClaimType::canonical(raw)?,
    };
    let polarity = match input.polarity.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => Polarity::DEFAULT,
        Some(raw) => Polarity::canonical(raw)?,
    };
    let modality = match input.modality.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => Modality::DEFAULT,
        Some(raw) => Modality::canonical(raw)?,
    };
    let status = match input.status.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => ClaimStatus::DEFAULT,
        Some(raw) => raw.parse::<ClaimStatus>()?,
    };
    let confidence = Claim::validate_confidence(input.confidence)?;

    let document_id = DocumentId::from_raw(input.document_id.trim());
    let document = document_repository::find_by_id(conn, &document_id)?
        .ok_or_else(|| AppError::NotFound(format!("文档 {document_id} 不存在，证据必须指向真实来源")))?;

    let transaction = conn.transaction()?;

    let subject = entity_repository::resolve_or_create(&transaction, &input.subject)?;

    let object = match input.object.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        None => None,
        Some(raw) => {
            // 只在**已存在**时才建立实体引用；否则存自由文本（见函数文档）。
            let existing = entity_repository::find_by_name(&transaction, raw)?
                .or(entity_repository::find_by_alias(&transaction, raw)?);
            Some(match existing {
                Some(entity) => ClaimObject::Entity(entity.id),
                None => ClaimObject::Literal(raw.to_string()),
            })
        }
    };

    let claim = Claim {
        id: ClaimId::new(),
        subject_id: subject.id,
        predicate,
        object,
        content: input.content.clone().filter(|v| !v.trim().is_empty()),
        context: serde_json::json!({}),
        claim_type,
        polarity,
        modality,
        condition: input.condition.clone().filter(|v| !v.trim().is_empty()),
        confidence,
        status,
        // 时间有效性来源尚未确定（决策 D2）：一律留空，表示"始终有效"。
        valid_from: None,
        valid_until: None,
        recorded_at: String::new(),
        created_at: String::new(),
    };

    claim_repository::insert(&transaction, &claim)?;

    // 证据必须指向真实切片：找不到就用该文档的第一块，
    // 并如实把层级降到段落级（不伪造精确引文）。
    let quote = input.quote.clone().filter(|v| !v.trim().is_empty());
    let chunk = match input
        .chunk_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(raw) => Some(
            document_repository::list_chunks(&transaction, &document_id)?
                .into_iter()
                .find(|chunk| chunk.id.as_str() == raw)
                .ok_or_else(|| AppError::NotFound(format!("切片 {raw} 不属于该文档")))?,
        ),
        None => pick_chunk_for(&transaction, &document_id, quote.as_deref())?,
    };

    let (start_offset, end_offset) = match (quote.as_deref(), &chunk) {
        (Some(text), Some(chunk)) => match chunk.content.find(text) {
            Some(offset) => (
                Some(chunk.start_offset + offset),
                Some(chunk.start_offset + offset + text.len()),
            ),
            None => (Some(chunk.start_offset), Some(chunk.end_offset)),
        },
        (None, Some(chunk)) => (Some(chunk.start_offset), Some(chunk.end_offset)),
        (_, None) => (None, None),
    };

    let decision = choose_level(
        quote.is_some(),
        claim.condition.is_some(),
        claim.confidence,
        false,
        false,
        DEFAULT_MAX_LEVEL,
    );

    evidence_repository::insert(
        &transaction,
        &Evidence {
            id: crate::domain::common::ids::EvidenceId::new(),
            claim_id: claim.id.clone(),
            document_id: document.id.clone(),
            chunk_id: chunk.as_ref().map(|c| c.id.clone()),
            start_offset,
            end_offset,
            quote,
            evidence_level: decision.level,
            source_type: document.source_type,
            confidence: claim.confidence,
            created_at: String::new(),
        },
    )?;

    transaction.commit()?;

    let stored = claim_repository::get(conn, &claim.id)?
        .ok_or_else(|| AppError::Internal("Claim 刚写入却读不到".into()))?;
    let cards = to_claim_cards_with_lifecycle(conn, std::slice::from_ref(&stored))?;
    cards
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("Claim 卡片构造失败".into()))
}

/// 选出承载引文的切片；引文不在任何切片里时退回第一块。
fn pick_chunk_for(
    conn: &Connection,
    document_id: &DocumentId,
    quote: Option<&str>,
) -> AppResult<Option<crate::domain::knowledge::chunk::Chunk>> {
    let chunks = document_repository::list_chunks(conn, document_id)?;
    if chunks.is_empty() {
        return Ok(None);
    }
    if let Some(text) = quote {
        if let Some(found) = chunks.iter().find(|chunk| chunk.content.contains(text)) {
            return Ok(Some(found.clone()));
        }
    }
    Ok(chunks.into_iter().next())
}

/// 归一化一个名称（供前端预览"这个名字会挂到哪个实体上"）。
pub fn preview_normalized_name(raw: &str) -> String {
    normalize_name(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::capture_service;
    use crate::application::dto::CreateDocumentInput;
    use crate::domain::evidence::evidence::EvidenceLevel;
    use crate::domain::knowledge::document::SourceType;
    use crate::infrastructure::db::tests::memory_db;

    fn seed_document(conn: &mut Connection, content: &str) -> DocumentId {
        let summary = capture_service::create_document(
            conn,
            CreateDocumentInput {
                title: "Note".into(),
                content: content.into(),
                source_type: None,
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
        DocumentId::from_raw(summary.id)
    }

    fn manual(subject: &str, predicate: &str, object: Option<&str>, document: &DocumentId) -> CreateClaimInput {
        CreateClaimInput {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.map(str::to_string),
            content: None,
            claim_type: None,
            polarity: None,
            modality: None,
            condition: None,
            confidence: None,
            document_id: document.as_str().to_string(),
            chunk_id: None,
            quote: None,
            status: None,
        }
    }

    #[test]
    fn manual_capture_creates_subject_claim_and_evidence_in_one_transaction() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "Rust 支持 async fn in trait。");

        let mut input = manual("Rust", "supports", Some("async fn in trait"), &document);
        input.quote = Some("Rust 支持 async fn in trait。".into());
        input.confidence = Some(0.8);

        let card = create_claim(&mut conn, input).unwrap();
        assert_eq!(card.predicate, "supports");
        assert_eq!(card.subject_name, "Rust");
        assert_eq!(card.evidence_count, 1);
        assert_eq!(card.lifecycle, "current");
        assert_eq!(card.status, "candidate");
        assert_eq!(card.display_text, "Rust supports async fn in trait");

        let detail = get_claim(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(detail.evidence.len(), 1);
        assert!(detail.evidence[0].quote.is_some());
        assert_eq!(detail.evidence[0].document_id, document.as_str());
    }

    #[test]
    fn unregistered_predicates_are_rejected_on_manual_entry_too() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let err = create_claim(&mut conn, manual("Rust", "vibes_with", None, &document)).unwrap_err();
        assert_eq!(err.code(), "DOMAIN_RULE_VIOLATION");
    }

    #[test]
    fn unknown_enums_and_out_of_range_confidence_are_rejected() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");

        let mut bad_polarity = manual("Rust", "uses", None, &document);
        bad_polarity.polarity = Some("sideways".into());
        assert_eq!(
            create_claim(&mut conn, bad_polarity).unwrap_err().code(),
            "DOMAIN_RULE_VIOLATION"
        );

        let mut bad_confidence = manual("Rust", "uses", None, &document);
        bad_confidence.confidence = Some(2.0);
        assert_eq!(
            create_claim(&mut conn, bad_confidence).unwrap_err().code(),
            "INVALID_INPUT"
        );
    }

    #[test]
    fn evidence_must_point_at_a_real_document() {
        let mut conn = memory_db();
        let mut input = manual("Rust", "uses", None, &DocumentId::from_raw("missing"));
        input.document_id = "missing".into();
        assert_eq!(
            create_claim(&mut conn, input).unwrap_err().code(),
            "NOT_FOUND"
        );
    }

    #[test]
    fn objects_only_become_entities_when_already_known() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "wiki-ya 使用 SQLite。");

        // 先建立一个已知实体
        entity_repository::resolve_or_create(&conn, "SQLite").unwrap();

        let known = create_claim(
            &mut conn,
            manual("wiki-ya", "uses", Some("SQLite"), &document),
        )
        .unwrap();
        assert!(known.object_id.is_some(), "已存在的宾语应落成实体引用");

        let unknown = create_claim(
            &mut conn,
            manual("wiki-ya", "supports", Some("整体架构清晰"), &document),
        )
        .unwrap();
        assert!(unknown.object_id.is_none(), "未知宾语不应污染实体表");
        assert_eq!(unknown.object_text.as_deref(), Some("整体架构清晰"));
        assert_eq!(
            claim_repository::count_unresolved_objects(&conn).unwrap(),
            1
        );
    }

    #[test]
    fn resolution_prefers_existing_entities_and_reuses_them() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        create_claim(&mut conn, manual("Rust", "uses", None, &document)).unwrap();
        create_claim(&mut conn, manual("rust", "supports", None, &document)).unwrap();

        let entities = list_entities(&conn, None, None, 10).unwrap();
        assert_eq!(entities.len(), 1, "大小写不同应解析到同一个实体");
        assert_eq!(entities[0].claim_count, 2);
    }

    #[test]
    fn claim_detail_lifecycle_is_derived_not_copied() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let card = create_claim(&mut conn, manual("Rust", "uses", Some("SQLite"), &document)).unwrap();
        assert_eq!(card.lifecycle, "current");

        conn.execute(
            "UPDATE claims SET status = 'superseded' WHERE id = ?1",
            rusqlite::params![card.id],
        )
        .unwrap();
        let detail = get_claim(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(detail.claim.lifecycle, "superseded");
    }

    #[test]
    fn excluded_claims_are_not_reported_as_history() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let card = create_claim(&mut conn, manual("Rust", "uses", Some("SQLite"), &document)).unwrap();

        // rejected 不参与检索，但它是「错误」而非「历史」。
        conn.execute(
            "UPDATE claims SET status = 'rejected' WHERE id = ?1",
            rusqlite::params![card.id],
        )
        .unwrap();

        let detail = get_claim(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(detail.claim.lifecycle, "excluded", "错误知识不能被标成历史");
        assert_eq!(detail.claim.status, "rejected");
    }

    #[test]
    fn entity_detail_includes_aliases_claims_relations_and_graph() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let card = create_claim(&mut conn, manual("wiki-ya", "uses", Some("SQLite"), &document)).unwrap();
        let entity_id = EntityId::from_raw(&card.subject_id);
        entity_repository::insert_alias(&conn, &entity_id, "wiki ya").unwrap();

        // 造一条实体关系（直接写表，因为 Phase 1 还没有抽取路径）。
        // 注意：未知宾语不会自动落成实体，所以这里先显式把 SQLite 建成实体。
        entity_repository::resolve_or_create(&conn, "SQLite").unwrap();
        let sqlite = entity_repository::find_by_name(&conn, "SQLite").unwrap().unwrap();
        relation_repository::insert(
            &conn,
            &entity_id,
            RelationPredicateForTest::USES,
            &sqlite.id,
            Some(0.9),
            None,
            None,
            None,
        )
        .unwrap();

        let detail = get_entity(&conn, &entity_id, 1).unwrap();
        assert_eq!(detail.entity.name, "wiki-ya");
        assert_eq!(detail.aliases, vec!["wiki ya".to_string()]);
        assert_eq!(detail.claims.len(), 1);
        assert_eq!(detail.relations.len(), 1);
        assert_eq!(detail.graph.nodes.len(), 2);
        assert_eq!(detail.graph.edges.len(), 1);
        assert!(!detail.graph.truncated);
    }

    #[test]
    fn unknown_ids_are_reported_as_not_found() {
        let mut conn = memory_db();
        let _ = seed_document(&mut conn, "body");
        assert_eq!(
            get_entity(&conn, &EntityId::from_raw("nope"), 1).unwrap_err().code(),
            "NOT_FOUND"
        );
        assert_eq!(
            get_claim(&conn, &ClaimId::from_raw("nope")).unwrap_err().code(),
            "NOT_FOUND"
        );
        assert_eq!(
            get_claim_history(&conn, &ClaimId::from_raw("nope")).unwrap_err().code(),
            "NOT_FOUND"
        );
    }

    #[test]
    fn list_claims_rejects_unknown_filters() {
        let mut conn = memory_db();
        let _ = seed_document(&mut conn, "body");
        assert!(list_claims(&conn, None, Some("vibes_with"), None, None, 10).is_err());
        assert!(list_claims(&conn, None, None, Some("nonsense"), None, 10).is_err());
    }

    #[test]
    fn name_preview_is_deterministic() {
        assert_eq!(preview_normalized_name("  OpenAI   Inc. "), "openai inc.");
    }

    /// 测试里用到的关系谓语别名，避免在测试内部再引入一次注册表查找。
    struct RelationPredicateForTest;
    impl RelationPredicateForTest {
        const USES: crate::domain::ontology::predicate::RelationPredicate =
            crate::domain::ontology::predicate::RelationPredicate::Uses;
    }

    #[test]
    fn evidence_level_follows_the_escalation_policy() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "Rust 支持 async fn in trait。");

        // 有引文 → 停在最便宜的 L1
        let mut with_quote = manual("Rust", "supports", None, &document);
        with_quote.quote = Some("Rust 支持 async fn in trait。".into());
        let card = create_claim(&mut conn, with_quote).unwrap();
        let evidence = list_evidence(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(evidence[0].evidence_level, 1);
        assert_eq!(evidence[0].evidence_level_name, "quote");

        // 无引文 → 退回段落级
        let without_quote = create_claim(&mut conn, manual("Rust", "uses", None, &document)).unwrap();
        let evidence = list_evidence(&conn, &ClaimId::from_raw(&without_quote.id)).unwrap();
        assert_eq!(evidence[0].evidence_level, 3);
        assert_eq!(evidence[0].evidence_level_name, "paragraph");
    }

    #[test]
    fn duplicate_evidence_ids_are_idempotent() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let card = create_claim(&mut conn, manual("Rust", "uses", None, &document)).unwrap();
        let claim_id = ClaimId::from_raw(&card.id);

        let existing = evidence_repository::list_for_claim(&conn, &claim_id).unwrap();
        assert_eq!(existing.len(), 1);
        evidence_repository::insert(&conn, &existing[0]).unwrap();
        assert_eq!(
            evidence_repository::list_for_claim(&conn, &claim_id).unwrap().len(),
            1,
            "重复写入同一证据应被忽略"
        );
    }

    #[test]
    fn evidence_pointing_at_a_missing_chunk_is_rejected() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "body");
        let mut input = manual("Rust", "uses", None, &document);
        input.chunk_id = Some("not-a-chunk".into());
        assert_eq!(create_claim(&mut conn, input).unwrap_err().code(), "NOT_FOUND");
    }

    #[test]
    fn source_type_is_carried_onto_the_evidence_row() {
        let mut conn = memory_db();
        let summary = capture_service::create_document(
            &mut conn,
            CreateDocumentInput {
                title: "md".into(),
                content: "内容".into(),
                source_type: Some("markdown".into()),
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
        let card = create_claim(
            &mut conn,
            manual("Rust", "uses", None, &DocumentId::from_raw(summary.id)),
        )
        .unwrap();
        let evidence = evidence_repository::list_for_claim(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(evidence[0].source_type, SourceType::Markdown);
    }

    #[test]
    fn evidence_level_enum_is_used_from_the_domain_layer() {
        assert_eq!(EvidenceLevel::Quote.as_u8(), 1);
        assert_eq!(ClaimStatus::ALL.len(), 6);
    }

    #[test]
    fn condition_is_stored_and_escalates_evidence() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "内容");
        let mut input = manual("Rust", "supports", None, &document);
        input.condition = Some("在 nightly 上".into());
        input.quote = Some("内容".into());
        let card = create_claim(&mut conn, input).unwrap();
        assert_eq!(card.condition.as_deref(), Some("在 nightly 上"));
        let evidence = list_evidence(&conn, &ClaimId::from_raw(&card.id)).unwrap();
        assert_eq!(evidence[0].evidence_level, 2, "条件式断言应升到 L2");
    }
}
