//! Agent 工具白名单（Phase 6 骨架，TDD §51）。
//!
//! 铁律：工具只能调用 `application` 层用例，**绝不允许 `execute_sql`**，
//! 也不允许 Agent 直接访问 Repository（TDD §50）。本文件只声明白名单与
//! 一个最小分发入口；具体实现随用例逐步接入。

/// 允许 Agent 调用的工具（TDD §51）。任何其他名字（尤其 `execute_sql`）一律拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolName {
    SearchKnowledge,
    GetKnowledge,
    GetEntities,
    GetEntity,
    GetClaim,
    FindRelated,
    GetEvidence,
    CompareClaims,
    DetectConflict,
    ProposeEvolution,
    RequestReview,
    Research,
}

impl ToolName {
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "search_knowledge" => Some(ToolName::SearchKnowledge),
            "get_knowledge" => Some(ToolName::GetKnowledge),
            "get_entities" => Some(ToolName::GetEntities),
            "get_entity" => Some(ToolName::GetEntity),
            "get_claim" => Some(ToolName::GetClaim),
            "find_related" => Some(ToolName::FindRelated),
            "get_evidence" => Some(ToolName::GetEvidence),
            "compare_claims" => Some(ToolName::CompareClaims),
            "detect_conflict" => Some(ToolName::DetectConflict),
            "propose_evolution" => Some(ToolName::ProposeEvolution),
            "request_review" => Some(ToolName::RequestReview),
            "research" => Some(ToolName::Research),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ToolName::SearchKnowledge => "search_knowledge",
            ToolName::GetKnowledge => "get_knowledge",
            ToolName::GetEntities => "get_entities",
            ToolName::GetEntity => "get_entity",
            ToolName::GetClaim => "get_claim",
            ToolName::FindRelated => "find_related",
            ToolName::GetEvidence => "get_evidence",
            ToolName::CompareClaims => "compare_claims",
            ToolName::DetectConflict => "detect_conflict",
            ToolName::ProposeEvolution => "propose_evolution",
            ToolName::RequestReview => "request_review",
            ToolName::Research => "research",
        }
    }
}

// ---------------------------------------------------------------------------
// 执行实现（Phase 6）
// ---------------------------------------------------------------------------

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::application::dto::{ClaimCard, SearchInput};
use crate::application::{knowledge_service, search_service};
use crate::domain::common::ids::{ClaimId, EntityId};
use crate::domain::evolution::classifier::EvolutionClassification;
use crate::domain::evolution::conflict::ClaimView;
use crate::domain::evolution::engine as evolution_engine;
use crate::domain::knowledge::claim::ClaimObject;
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::domain::review::review::ReviewTarget;
use crate::error::{AppError, AppResult};
use crate::infrastructure::review_repository;

/// 工具输出（TDD §40：short / structured / stable / drillable）。
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub value: Value,
    pub truncated: bool,
    pub hint: Option<String>,
}

impl ToolOutput {
    fn ok(value: Value) -> AppResult<Self> {
        Ok(ToolOutput {
            value,
            truncated: false,
            hint: None,
        })
    }

    /// 渲染成给模型看的紧凑 JSON 文本（含 truncated / hint，TDD §40）。
    pub fn render(&self) -> String {
        let mut body = self.value.clone();
        if let Some(hint) = &self.hint {
            body["hint"] = json!(hint);
        }
        body["truncated"] = json!(self.truncated);
        serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string())
    }
}

/// 单段文本的截断阈值（字符），保证工具输出短小（TDD §39）。
const MAX_TEXT_CHARS: usize = 400;

fn clip(text: &str) -> String {
    let clipped: String = text.chars().take(MAX_TEXT_CHARS).collect();
    if clipped.len() < text.len() {
        format!("{clipped}…")
    } else {
        text.to_string()
    }
}

fn arg_str(args: &Value, key: &str) -> AppResult<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Internal(format!("工具参数 `{key}` 缺失")))
}

fn arg_usize(args: &Value, key: &str, default: usize) -> usize {
    args.get(key)
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(default)
}

/// 执行白名单内的工具；`args` 由模型以 JSON 对象提供。
///
/// 只读保证：全部委托 `application` 层用例，**永不直接执行 SQL**
/// （TDD §50/§51，`execute_sql` 被白名单机制结构性排除）。
/// 未接入执行的枚举项诚实拒绝，绝不假装成功。
pub fn execute(conn: &Connection, name: ToolName, args: &Value) -> AppResult<ToolOutput> {
    match name {
        ToolName::SearchKnowledge => search_knowledge(conn, args),
        ToolName::GetKnowledge => get_knowledge(conn, args),
        ToolName::GetEntities => get_entities(conn, args),
        ToolName::GetEntity => get_entity(conn, args),
        ToolName::GetClaim => get_claim(conn, args),
        ToolName::GetEvidence => get_evidence(conn, args),
        ToolName::FindRelated => find_related(conn, args),
        ToolName::CompareClaims => compare_claims(conn, args),
        ToolName::DetectConflict => detect_conflict(conn, args),
        ToolName::ProposeEvolution => propose_evolution(conn, args),
        ToolName::RequestReview => request_review(conn, args),
        // `research` 是递归调用（Agent 内再起研究），无意义，保持诚实未启用。
        ToolName::Research => Err(AppError::Internal(format!(
            "工具 `{}` 尚未启用",
            name.as_str()
        ))),
    }
}

fn search_knowledge(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let query = arg_str(args, "query")?;
    let limit = arg_usize(args, "limit", 8).min(15);
    let response = search_service::search(
        conn,
        SearchInput {
            query,
            limit: Some(limit),
            semantic: Some(false),
            kinds: None,
        },
    )?;

    let mut truncated = false;
    let results: Vec<Value> = response
        .hits
        .iter()
        .take(limit)
        .map(|hit| {
            let snippet = clip(&hit.snippet);
            if snippet.ends_with('…') {
                truncated = true;
            }
            json!({
                "id": hit.id,
                "kind": hit.kind,
                "title": hit.title,
                "score": hit.score,
                "snippet": snippet,
            })
        })
        .collect();

    Ok(ToolOutput {
        value: json!({ "results": results }),
        truncated,
        hint: truncated.then(|| "结果已截断；用 get_claim(id) / get_entity(id) 下钻详情".to_string()),
    })
}

/// 未指定 kind 时先按 Claim 再按 Entity 尝试（幂等只读，成本可忽略）。
fn get_knowledge(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let id = arg_str(args, "id")?;
    if let Ok(output) = claim_output(conn, &id) {
        return Ok(output);
    }
    entity_output(conn, &id)
}

fn get_entity(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let id = arg_str(args, "id")?;
    entity_output(conn, &id)
}

fn get_claim(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let id = arg_str(args, "id")?;
    claim_output(conn, &id)
}

fn get_evidence(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let claim_id = arg_str(args, "claim_id")?;
    let cards = knowledge_service::list_evidence(conn, &ClaimId::from_raw(&claim_id))?;
    let evidence: Vec<Value> = cards
        .iter()
        .take(5)
        .map(|card| {
            json!({
                "id": card.id,
                "document_title": card.document_title,
                "quote": card.quote.as_deref().map(clip),
                "level": card.evidence_level_name,
            })
        })
        .collect();
    ToolOutput::ok(json!({ "claim_id": claim_id, "evidence": evidence }))
}

fn find_related(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let entity_id = arg_str(args, "entity_id")?;
    let detail = knowledge_service::get_entity(conn, &EntityId::from_raw(&entity_id), 1)?;
    let relations: Vec<Value> = detail
        .relations
        .iter()
        .take(10)
        .map(|relation| {
            json!({
                "predicate": relation.predicate,
                "source": relation.source_name,
                "target": relation.target_name,
                "status": relation.status,
                "confidence": relation.confidence,
            })
        })
        .collect();
    ToolOutput::ok(json!({
        "entity": detail.entity.name,
        "relations": relations,
    }))
}

fn entity_output(conn: &Connection, id: &str) -> AppResult<ToolOutput> {
    let detail = knowledge_service::get_entity(conn, &EntityId::from_raw(id), 1)?;
    let claims: Vec<Value> = detail
        .claims
        .iter()
        .take(10)
        .map(|claim| {
            json!({
                "id": claim.id,
                "statement": claim.display_text,
                "status": claim.status,
                "lifecycle": claim.lifecycle,
            })
        })
        .collect();
    let relations: Vec<Value> = detail
        .relations
        .iter()
        .take(10)
        .map(|relation| {
            json!({
                "predicate": relation.predicate,
                "source": relation.source_name,
                "target": relation.target_name,
                "status": relation.status,
            })
        })
        .collect();
    ToolOutput::ok(json!({
        "entity": {
            "id": detail.entity.id,
            "name": detail.entity.name,
            "primary_type": detail.entity.primary_type,
            "status": detail.entity.status,
            "description": detail.entity.description,
        },
        "aliases": detail.aliases,
        "claims": claims,
        "relations": relations,
    }))
}

fn claim_output(conn: &Connection, id: &str) -> AppResult<ToolOutput> {
    let detail = knowledge_service::get_claim(conn, &ClaimId::from_raw(id))?;
    let claim = &detail.claim;
    let evidence: Vec<Value> = detail
        .evidence
        .iter()
        .take(5)
        .map(|card| {
            json!({
                "quote": card.quote.as_deref().map(clip),
                "document_title": card.document_title,
                "level": card.evidence_level_name,
            })
        })
        .collect();
    ToolOutput::ok(json!({
        "claim": {
            "id": claim.id,
            "statement": claim.display_text,
            "status": claim.status,
            "lifecycle": claim.lifecycle,
            "confidence": claim.confidence,
            "evidence_count": claim.evidence_count,
        },
        "evidence": evidence,
    }))
}

// ---------------------------------------------------------------------------
// 演化分析 / 审核提案 / 批量（TDD §41/§51）
// ---------------------------------------------------------------------------

/// 把 Claim 卡片收窄成演化判定所需的最小视图。
///
/// 判定只依赖这几个字段（`ClaimView` 的设计意图），不把整张卡塞进引擎。
fn claim_view_from_card(card: &ClaimCard) -> AppResult<ClaimView> {
    Ok(ClaimView {
        id: ClaimId::from_raw(&card.id),
        subject_id: EntityId::from_raw(&card.subject_id),
        predicate: ClaimPredicate::canonical(&card.predicate)?,
        object: card
            .object_id
            .as_ref()
            .map(|id| ClaimObject::Entity(EntityId::from_raw(id)))
            .or_else(|| card.object_text.clone().map(ClaimObject::Literal)),
        polarity: card.polarity.parse()?,
        status: card.status.parse()?,
        created_at: card.created_at.clone(),
    })
}

/// 确定性比较两条 Claim（TDD §51：compare_claims）。
fn compare_claims(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let a_id = arg_str(args, "claim_a")?;
    let b_id = arg_str(args, "claim_b")?;
    let a = knowledge_service::get_claim(conn, &ClaimId::from_raw(&a_id))?;
    let b = knowledge_service::get_claim(conn, &ClaimId::from_raw(&b_id))?;
    let a_view = claim_view_from_card(&a.claim)?;
    let b_view = claim_view_from_card(&b.claim)?;

    let classification = evolution_engine::classify(&a_view, std::slice::from_ref(&b_view));
    let comparisons: Vec<Value> = evolution_engine::analyze(&a_view, std::slice::from_ref(&b_view))
        .iter()
        .map(|verdict| {
            json!({
                "relationship": verdict.relationship.as_str(),
                "suggested_action": format!("{:?}", verdict.suggested_action),
            })
        })
        .collect();

    ToolOutput::ok(json!({
        "claim_a": a.claim.display_text,
        "claim_b": b.claim.display_text,
        "classification": format!("{classification:?}"),
        "comparisons": comparisons,
    }))
}

/// 检测某实体当前知识中的矛盾对（TDD §51：detect_conflict）。
fn detect_conflict(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let entity_id = arg_str(args, "entity_id")?;
    let detail = knowledge_service::get_entity(conn, &EntityId::from_raw(&entity_id), 1)?;
    let current: Vec<ClaimCard> = detail
        .claims
        .iter()
        .filter(|card| card.lifecycle == "current")
        .cloned()
        .collect();

    let mut conflicts: Vec<Value> = Vec::new();
    for i in 0..current.len() {
        for j in (i + 1)..current.len() {
            let (Ok(a), Ok(b)) = (
                claim_view_from_card(&current[i]),
                claim_view_from_card(&current[j]),
            ) else {
                continue;
            };
            if a.subject_id != b.subject_id || a.predicate != b.predicate {
                continue;
            }
            if evolution_engine::classify(&a, &[b]) == EvolutionClassification::Contradicts {
                conflicts.push(json!({
                    "claim_a": { "id": current[i].id, "statement": current[i].display_text },
                    "claim_b": { "id": current[j].id, "statement": current[j].display_text },
                }));
            }
        }
    }

    ToolOutput::ok(json!({
        "entity": detail.entity.name,
        "checked": current.len(),
        "conflicts": conflicts,
    }))
}

/// 生成演化建议（只读，不落库；落库走 request_review，TDD §51）。
fn propose_evolution(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let claim_id = arg_str(args, "claim_id")?;
    let detail = knowledge_service::get_claim(conn, &ClaimId::from_raw(&claim_id))?;
    let incoming = claim_view_from_card(&detail.claim)?;

    // 同主体候选：经实体邻域取同 (subject, predicate) 的 current claims。
    let entity_detail = knowledge_service::get_entity(conn, &incoming.subject_id, 1)?;
    let mut proposals: Vec<Value> = Vec::new();
    for card in &entity_detail.claims {
        if card.id == detail.claim.id || card.lifecycle != "current" {
            continue;
        }
        let Ok(existing) = claim_view_from_card(card) else {
            continue;
        };
        if existing.subject_id != incoming.subject_id || existing.predicate != incoming.predicate {
            continue;
        }
        for verdict in evolution_engine::analyze(&incoming, &[existing]) {
            if verdict.is_persistable() {
                proposals.push(json!({
                    "against_claim": card.id,
                    "statement": card.display_text,
                    "relationship": verdict.relationship.as_str(),
                    "suggested_action": format!("{:?}", verdict.suggested_action),
                }));
            }
        }
    }

    ToolOutput::ok(json!({
        "claim": detail.claim.display_text,
        "proposals": proposals,
        "note": "建议不会自动落库；确认后用 request_review 提交到审核队列。",
    }))
}

/// 把一条提案登记进审核队列（Agent 唯一的写路径，且只写 reviews）。
fn request_review(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let target_id = arg_str(args, "target_id")?;
    let reason = args
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    // Agent 提案统一记为 agent_proposal：真正改知识的关系/状态迁移
    // 只能由用户在 Review 中走既有入口（INV-08/INV-10）。
    let review_id = review_repository::insert_pending(
        conn,
        ReviewTarget::AgentProposal,
        &target_id,
        json!({ "reason": reason }),
    )?;
    ToolOutput::ok(json!({
        "review_id": review_id.as_str(),
        "status": "pending",
    }))
}

/// 批量取实体摘要（TDD §41：get_entities(ids[])）。
fn get_entities(conn: &Connection, args: &Value) -> AppResult<ToolOutput> {
    let ids: Vec<String> = args
        .get("ids")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str())
                .map(|s| s.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        return Err(AppError::Internal("工具参数 `ids` 缺失".into()));
    }

    let mut results: Vec<Value> = Vec::new();
    for id in ids.iter().take(10) {
        match knowledge_service::get_entity(conn, &EntityId::from_raw(id), 1) {
            Ok(detail) => results.push(json!({
                "id": detail.entity.id,
                "name": detail.entity.name,
                "primary_type": detail.entity.primary_type,
                "status": detail.entity.status,
                "claim_count": detail.claims.len(),
            })),
            Err(err) => results.push(json!({ "id": id, "error": err.to_string() })),
        }
    }

    let truncated = ids.len() > 10;
    Ok(ToolOutput {
        value: json!({ "results": results }),
        truncated,
        hint: truncated.then(|| "一次最多 10 个实体，请分批调用".to_string()),
    })
}
