//! Review 用例 —— 把待确认的知识变更交给用户，并把用户的判断落库。
//!
//! 这一层是「AI suggests, user decides」的最后一道闸门：
//! 任何会改变「当前知识」的操作（目前只有 `supersedes`）都必须经过这里。

use rusqlite::Connection;

use crate::application::dto::{ClaimRelationCard, DecideRelationInput, ReviewItem};
use crate::application::evolution_service;
use crate::application::knowledge_service::to_relation_card_dto;
use crate::domain::common::ids::ClaimRelationId;
use crate::domain::evolution::classifier::ClaimRelationType;
use crate::domain::evolution::decision::ReviewAction;
use crate::domain::review::review::describe;
use crate::domain::review::rules::review_priority;
use crate::error::{AppError, AppResult};
use crate::infrastructure::{claim_relation_repository, claim_repository};

/// 待审核队列。
///
/// 排序由存储层用 SQL CASE 完成（duplicate → supersedes → contradicts →
/// coexists → unclear），与领域层的 `review_priority` 保持一致；
/// 两处都写是因为一个要排序、一个要展示，但它们用的是同一套顺序定义。
pub fn list_review_items(conn: &Connection, limit: usize) -> AppResult<Vec<ReviewItem>> {
    let rows = claim_relation_repository::list_pending(conn, limit)?;
    let mut items = Vec::with_capacity(rows.len());

    for row in &rows {
        // 引文来自发起方（新知识）的主证据；取不到就如实留空，不编造。
        let quote = claim_repository::get(conn, &row.source_claim_id)?
            .and_then(|claim| claim.source_quote);
        let (what_changed, impact) = describe(row.relationship, &row.source_text, &row.target_text);

        items.push(ReviewItem {
            relation: to_relation_card_dto(row),
            priority: review_priority(row.relationship.as_str()),
            what_changed,
            why: row
                .reason
                .clone()
                .unwrap_or_else(|| "（未记录判定原因）".to_string()),
            evidence_quote: quote,
            impact,
        });
    }

    Ok(items)
}

/// 解析审核决策字符串。
fn parse_action(raw: &str) -> AppResult<ReviewAction> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "accept" => Ok(ReviewAction::Accept),
        "reject" => Ok(ReviewAction::Reject),
        "reset" => Ok(ReviewAction::Reset),
        other => Err(AppError::Invalid(format!(
            "未知的审核决策 {other:?}（可选 accept / reject / reset）"
        ))),
    }
}

/// 提交一次审核决策。
///
/// 只做「解析输入 → 交给 Application 编排」。状态迁移规则与事务都在
/// [`evolution_service::decide_relation`] 与领域层，本函数不写业务逻辑。
pub fn decide_relation(
    conn: &mut Connection,
    input: DecideRelationInput,
) -> AppResult<ClaimRelationCard> {
    let action = parse_action(&input.decision)?;

    let relationship_override = match input
        .relationship
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        None => None,
        Some(raw) => Some(raw.parse::<ClaimRelationType>()?),
    };

    let relation_id = ClaimRelationId::from_raw(input.relation_id.trim());
    if relation_id.as_str().is_empty() {
        return Err(AppError::Invalid("relationId 不能为空".into()));
    }

    let row = evolution_service::decide_relation(conn, &relation_id, action, relationship_override)?;
    Ok(to_relation_card_dto(&row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::capture_service;
    use crate::application::dto::{CreateClaimInput, CreateDocumentInput};
    use crate::application::evolution_service;
    use crate::application::knowledge_service;
    use crate::domain::common::ids::DocumentId;
    use crate::infrastructure::db::tests::memory_db;

    fn seed_conflict(conn: &mut Connection) -> (String, String, String) {
        let first = seed_document(conn, "First", "OpenAI 的 CEO 是 Sam。");
        let old = knowledge_service::create_claim(
            conn,
            claim_input("OpenAI", "is", Some("Sam"), &first),
        )
        .unwrap();

        let second = seed_document(conn, "Second", "OpenAI 的 CEO 是 Alice。");
        let new = knowledge_service::create_claim(
            conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();

        let report = evolution_service::analyze_document(conn, &second).unwrap();
        (old.id, new.id, report.verdicts[0].id.clone())
    }

    fn seed_document(conn: &mut Connection, title: &str, content: &str) -> DocumentId {
        let summary = capture_service::create_document(
            conn,
            CreateDocumentInput {
                title: title.into(),
                content: content.into(),
                source_type: None,
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
        DocumentId::from_raw(summary.id)
    }

    fn claim_input(
        subject: &str,
        predicate: &str,
        object: Option<&str>,
        document: &DocumentId,
    ) -> CreateClaimInput {
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
            quote: Some("OpenAI 的 CEO 是 Sam。".into()),
            status: None,
            observed_at: None,
        }
    }

    fn status_of(conn: &Connection, claim_id: &str) -> String {
        conn.query_row(
            "SELECT status FROM claims WHERE id = ?1",
            rusqlite::params![claim_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn review_items_answer_the_four_questions() {
        let mut conn = memory_db();
        let (_, _, _) = seed_conflict(&mut conn);

        let items = list_review_items(&conn, 20).unwrap();
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert!(item.what_changed.contains("互相矛盾"));
        assert!(!item.why.is_empty());
        assert!(item.evidence_quote.is_some());
        assert!(item.impact.contains("不会改动"));
        // contradicts 的审核优先级为 3（低于 supersedes=2，高于 coexists=4）。
        assert_eq!(item.priority, 3);
    }

    #[test]
    fn accepting_a_supersedes_moves_history_and_keeps_content() {
        let mut conn = memory_db();
        let (old_id, _, relation_id) = seed_conflict(&mut conn);

        let card = decide_relation(
            &mut conn,
            DecideRelationInput {
                relation_id,
                decision: "accept".into(),
                relationship: Some("supersedes".into()),
            },
        )
        .unwrap();

        assert_eq!(card.status, "accepted");
        assert_eq!(card.relationship, "supersedes");
        assert_eq!(card.target_previous_status.as_deref(), Some("candidate"));
        assert_eq!(status_of(&conn, &old_id), "superseded");
    }

    #[test]
    fn resetting_restores_the_previous_status_exactly() {
        let mut conn = memory_db();
        let (old_id, _, relation_id) = seed_conflict(&mut conn);

        decide_relation(
            &mut conn,
            DecideRelationInput {
                relation_id: relation_id.clone(),
                decision: "accept".into(),
                relationship: Some("supersedes".into()),
            },
        )
        .unwrap();
        assert_eq!(status_of(&conn, &old_id), "superseded");

        decide_relation(
            &mut conn,
            DecideRelationInput {
                relation_id,
                decision: "reset".into(),
                relationship: None,
            },
        )
        .unwrap();
        assert_eq!(status_of(&conn, &old_id), "candidate", "必须精确恢复（INV-09）");
    }

    #[test]
    fn rejecting_changes_nothing_and_leaves_the_queue() {
        let mut conn = memory_db();
        let (old_id, new_id, relation_id) = seed_conflict(&mut conn);

        decide_relation(
            &mut conn,
            DecideRelationInput {
                relation_id,
                decision: "reject".into(),
                relationship: None,
            },
        )
        .unwrap();

        assert_eq!(status_of(&conn, &old_id), "candidate");
        assert_eq!(status_of(&conn, &new_id), "candidate");
        assert!(list_review_items(&conn, 20).unwrap().is_empty());
    }

    #[test]
    fn unknown_decisions_and_relationships_are_rejected() {
        let mut conn = memory_db();
        let (_, _, relation_id) = seed_conflict(&mut conn);

        assert_eq!(
            decide_relation(
                &mut conn,
                DecideRelationInput {
                    relation_id: relation_id.clone(),
                    decision: "maybe".into(),
                    relationship: None,
                },
            )
            .unwrap_err()
            .code(),
            "INVALID_INPUT"
        );

        assert_eq!(
            decide_relation(
                &mut conn,
                DecideRelationInput {
                    relation_id,
                    decision: "accept".into(),
                    relationship: Some("vibes_with".into()),
                },
            )
            .unwrap_err()
            .code(),
            "DOMAIN_RULE_VIOLATION"
        );
    }

    #[test]
    fn unknown_relation_ids_are_reported_as_not_found() {
        let mut conn = memory_db();
        let err = decide_relation(
            &mut conn,
            DecideRelationInput {
                relation_id: "missing".into(),
                decision: "accept".into(),
                relationship: None,
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "NOT_FOUND");
    }

    #[test]
    fn empty_queue_is_an_honest_empty_list() {
        let mut conn = memory_db();
        seed_document(&mut conn, "Lonely", "无关内容");
        assert!(list_review_items(&conn, 20).unwrap().is_empty());
    }
}
