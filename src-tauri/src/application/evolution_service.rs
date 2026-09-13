//! Evolution 用例 —— 对一份文档做**确定性**的演化分析。
//!
//! 这一步完全不调用 LLM：同主语同谓语的候选检索、关系判定、
//! 落库策略全部由领域规则决定。它的价值在于——**用户导入一份文档后
//! 立刻就能看到「这条知识和已有的哪条冲突」，而不需要配置任何 API Key**。
//!
//! 属于人的那一半（`supersedes` 的确认）留给 Review。

use rusqlite::Connection;

use crate::application::dto::AnalysisReport;
use crate::application::knowledge_service::to_relation_card_dto;
use crate::domain::common::ids::{ClaimRelationId, DocumentId};
use crate::domain::evolution::classifier::{ClaimRelationStatus, ClaimRelationType};
use crate::domain::evolution::conflict::{compare_all, ClaimView};
use crate::domain::evolution::decision::{EvolutionTransition, ReviewAction};
use crate::domain::knowledge::claim::ClaimStatus;
use crate::error::{AppError, AppResult};
use crate::infrastructure::{
    claim_relation_repository, claim_repository, document_repository,
};
use crate::infrastructure::claim_relation_repository::ClaimRelationRow;

/// 每一条 Claim 最多比较多少个候选。
///
/// 上限是必要的：某个谓词可能被高频使用（例如 `uses`），
/// 不设上限会让一次导入变成 O(n²) 的审核队列生成器。
const MAX_CANDIDATES: usize = 20;

/// 分析一份文档贡献的全部 Claim，并写入待确认的演化关系。
pub fn analyze_document(
    conn: &mut Connection,
    document_id: &DocumentId,
) -> AppResult<AnalysisReport> {
    if document_repository::find_by_id(conn, document_id)?.is_none() {
        return Err(AppError::NotFound(format!("文档 {document_id} 不存在")));
    }

    let claims = claim_repository::list_by_document(conn, document_id)?;

    let transaction = conn.transaction()?;
    let mut written = 0i64;
    let mut created_ids = Vec::new();
    let mut summary = (0i64, 0i64, 0i64, 0i64);

    for row in &claims {
        let candidates = claim_repository::find_candidates(
            &transaction,
            &row.claim.subject_id,
            row.claim.predicate,
            Some(&row.claim.id),
            MAX_CANDIDATES,
        )?;
        if candidates.is_empty() {
            continue;
        }

        let candidate_views: Vec<ClaimView> = candidates.iter().map(|c| c.to_view()).collect();
        for verdict in compare_all(&row.to_view(), &candidate_views) {
            // `unclear` 不落库：它不表达任何知识，只会污染审核队列（INV-11 的推论）。
            if !verdict.is_persistable() {
                continue;
            }
            // 同一对 Claim 只留一条关系（INV-07）。
            if claim_relation_repository::pair_exists(
                &transaction,
                &row.claim.id,
                &verdict.related_claim_id,
            )? {
                continue;
            }

            let created = claim_relation_repository::insert(
                &transaction,
                &row.claim.id,
                &verdict.related_claim_id,
                verdict.relationship,
                verdict.initial_status(),
                Some(verdict.confidence),
                Some(verdict.reason.as_str()),
                Some(verdict.suggested_action.as_str()),
            )?;

            match verdict.relationship {
                ClaimRelationType::Duplicate => summary.0 += 1,
                ClaimRelationType::Coexists => summary.1 += 1,
                ClaimRelationType::Contradicts => summary.2 += 1,
                _ => {}
            }
            if verdict.suggested_action == crate::domain::evolution::conflict::SuggestedAction::Review
            {
                summary.3 += 1;
            }

            if let Some(id) = created {
                written += 1;
                created_ids.push(id);
            }
        }
    }

    transaction.commit()?;

    // 回读刚建立的关系，保证返回给 UI 的内容与库里完全一致（含时间戳与 id）。
    let mut verdicts = Vec::with_capacity(created_ids.len());
    for id in created_ids {
        if let Some(row) = claim_relation_repository::get(conn, &id)? {
            verdicts.push(to_relation_card_dto(&row));
        }
    }

    Ok(AnalysisReport {
        document_id: document_id.as_str().to_string(),
        claims_scanned: claims.len() as i64,
        relations_written: written,
        duplicates: summary.0,
        coexists: summary.1,
        contradictions: summary.2,
        needs_review: summary.3,
        verdicts,
    })
}

/// 提交一次审核决策（K-002 的编排入口）。
///
/// 职责边界：
/// ```text
/// 本函数（Application）  ：加载关系 + 开事务 + 调用领域 + 持久化 + 提交
/// EvolutionTransition.plan（Domain）：唯一决定"知识为什么变成什么状态"
/// claim_relation_repository（Infrastructure）：纯持久化，不做业务判断
/// ```
/// 这样以后新增 rollback / batch review / import / AI extraction 都复用它，
/// 而不是各自实现一遍"怎么把 Claim 变成 superseded"。
pub fn decide_relation(
    conn: &mut Connection,
    relation_id: &ClaimRelationId,
    action: ReviewAction,
    relationship_override: Option<ClaimRelationType>,
) -> AppResult<ClaimRelationRow> {
    let transaction = conn.transaction()?;

    let current = claim_relation_repository::get(&transaction, relation_id)?
        .ok_or_else(|| AppError::NotFound(format!("演化关系 {relation_id} 不存在")))?;

    // 允许审核时修正关系类型（用户可能判断"这不是取代而是补充"）。
    let relationship = relationship_override.unwrap_or(current.relationship);
    let target_status =
        claim_relation_repository::claim_status(&transaction, &current.target_claim_id)?;
    let rollback_anchor = current
        .target_previous_status
        .as_deref()
        .map(str::parse::<ClaimStatus>)
        .transpose()?;

    let transition = EvolutionTransition::plan(relationship, action, target_status, rollback_anchor);
    let row = claim_relation_repository::apply_decision(
        &transaction,
        relation_id,
        relationship,
        &transition,
    )?;

    transaction.commit()?;
    Ok(row)
}

/// 回滚一次已接受的演化决策（契约 §29/§30）。
///
/// 语义上等价于「把关系退回待审 + 按锚点精确恢复目标状态」，但保留为**具名
/// 操作**，让审计与 UI 能直接表达「撤销这次知识变更」。它**不会删除**任何
/// 历史事件——回滚本身也会追加一条新事件（CORE-005 / I-007）。
pub fn rollback(
    conn: &mut Connection,
    relation_id: &ClaimRelationId,
) -> AppResult<ClaimRelationRow> {
    decide_relation(conn, relation_id, ReviewAction::Reset, None)
}

/// 直接确认一条关系（把 `candidate` 变成 `accepted`），不改变关系类型。
///
/// 存在的意义：`duplicate` 与 `coexists` 在分析阶段就已经自动 accepted，
/// 但用户可能先拒绝、之后又想接受。这条路径让"恢复"成为可能，
/// 而不需要重新分析整份文档。
pub fn accept_existing(
    conn: &mut Connection,
    relation_id: &ClaimRelationId,
) -> AppResult<crate::application::dto::ClaimRelationCard> {
    let row = decide_relation(conn, relation_id, ReviewAction::Accept, None)?;
    Ok(to_relation_card_dto(&row))
}

/// 关系状态的枚举在本模块的公开签名里出现，保证调用方不会自己拼字符串。
pub const CANDIDATE_STATUS: ClaimRelationStatus = ClaimRelationStatus::Candidate;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::capture_service;
    use crate::application::dto::{CreateClaimInput, CreateDocumentInput};
    use crate::application::knowledge_service;
    use crate::infrastructure::db::tests::memory_db;

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
            quote: None,
            status: None,
            observed_at: None,
        }
    }

    /// 造出"同一主语同一谓语、宾语不同"的两份文档，制造一次冲突。
    fn seed_conflict(conn: &mut Connection) -> DocumentId {
        let first = seed_document(conn, "First", "OpenAI 的 CEO 是 Sam。");
        knowledge_service::create_claim(
            conn,
            claim_input("OpenAI", "is", Some("Sam"), &first),
        )
        .unwrap();

        let second = seed_document(conn, "Second", "OpenAI 的 CEO 是 Alice。");
        second
    }

    #[test]
    fn analysis_of_a_document_without_related_claims_writes_nothing() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "Lonely", "全新主题的内容。");
        let report = analyze_document(&mut conn, &document).unwrap();
        assert_eq!(report.claims_scanned, 0);
        assert_eq!(report.relations_written, 0);
        assert!(report.verdicts.is_empty());
    }

    #[test]
    fn a_single_valued_change_becomes_a_pending_contradiction() {
        let mut conn = memory_db();
        let second = seed_conflict(&mut conn);
        knowledge_service::create_claim(
            &mut conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(report.claims_scanned, 1);
        assert_eq!(report.relations_written, 1);
        assert_eq!(report.contradictions, 1);
        assert_eq!(report.needs_review, 1);

        let verdict = &report.verdicts[0];
        assert_eq!(verdict.relationship, "contradicts");
        assert_eq!(verdict.status, "candidate", "冲突必须等人工确认（INV-10）");
        assert!(verdict.reason.as_deref().unwrap().contains("一个取值"));
    }

    #[test]
    fn analysis_is_idempotent() {
        let mut conn = memory_db();
        let second = seed_conflict(&mut conn);
        knowledge_service::create_claim(
            &mut conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();

        let first = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(first.relations_written, 1);
        let again = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(again.relations_written, 0, "同一对 Claim 不应重复上报（INV-07）");
    }

    #[test]
    fn identical_statements_are_auto_accepted_as_duplicates() {
        let mut conn = memory_db();
        let first = seed_document(&mut conn, "First", "SQLite 被本项目使用。");
        knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &first),
        )
        .unwrap();

        let second = seed_document(&mut conn, "Second", "本项目使用了 SQLite。");
        knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(report.duplicates, 1);
        assert_eq!(report.needs_review, 0, "重复不改变知识，无需打扰用户");
        assert_eq!(report.verdicts[0].status, "accepted");
        assert_eq!(report.verdicts[0].suggested_action.as_deref(), Some("link_evidence"));
    }

    #[test]
    fn supersedes_is_never_produced_by_analysis() {
        let mut conn = memory_db();
        let second = seed_conflict(&mut conn);
        knowledge_service::create_claim(
            &mut conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert!(report
            .verdicts
            .iter()
            .all(|verdict| verdict.relationship != "supersedes"));
    }

    #[test]
    fn analysis_never_changes_any_claim_status() {
        let mut conn = memory_db();
        let second = seed_conflict(&mut conn);
        knowledge_service::create_claim(
            &mut conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();

        analyze_document(&mut conn, &second).unwrap();

        let superseded = claim_repository::count_superseded(&conn).unwrap();
        assert_eq!(superseded, 0, "分析阶段绝不能动 claims.status（INV-08）");
    }

    #[test]
    fn missing_documents_are_reported_as_not_found() {
        let mut conn = memory_db();
        let err = analyze_document(&mut conn, &DocumentId::from_raw("missing")).unwrap_err();
        assert_eq!(err.code(), "NOT_FOUND");
    }

    #[test]
    fn explicitly_accepting_an_existing_relation_works() {
        let mut conn = memory_db();
        let second = seed_conflict(&mut conn);
        knowledge_service::create_claim(
            &mut conn,
            claim_input("OpenAI", "is", Some("Alice"), &second),
        )
        .unwrap();
        let report = analyze_document(&mut conn, &second).unwrap();

        let relation_id =
            crate::domain::common::ids::ClaimRelationId::from_raw(&report.verdicts[0].id);
        let accepted = accept_existing(&mut conn, &relation_id).unwrap();
        assert_eq!(accepted.status, "accepted");
        assert_eq!(CANDIDATE_STATUS.as_str(), "candidate");
    }

    // -----------------------------------------------------------------------
    // REL-001 / K-005：Evolution 端到端（经统一 Resolver 校验当前知识）
    // -----------------------------------------------------------------------

    use crate::domain::common::ids::ClaimId;

    /// 经统一 Resolver 派生某条 Claim 的 lifecycle（ARCH-001 的真实调用路径）。
    fn lifecycle_of(conn: &Connection, id: &str) -> String {
        knowledge_service::get_claim(conn, &ClaimId::from_raw(id))
            .unwrap()
            .claim
            .lifecycle
    }

    /// 造出 "OpenAI is Sam" / "OpenAI is Alice" 的冲突，并完成一次演化分析。
    fn seed_supersede_case(conn: &mut Connection) -> (String, String, ClaimRelationId) {
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

        let report = analyze_document(conn, &second).unwrap();
        let relation_id = ClaimRelationId::from_raw(&report.verdicts[0].id);
        (old.id, new.id, relation_id)
    }

    /// 最完整的那条链：接受取代 → 当前知识切换 → 回滚 → 恢复。
    #[test]
    fn e2e_supersede_then_rollback_moves_current_knowledge() {
        let mut conn = memory_db();
        let (old_id, new_id, relation_id) = seed_supersede_case(&mut conn);

        // 接受取代：旧知识变为历史，新知识成为当前。
        decide_relation(
            &mut conn,
            &relation_id,
            ReviewAction::Accept,
            Some(ClaimRelationType::Supersedes),
        )
        .unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "superseded", "旧知识应成为历史");
        assert_eq!(lifecycle_of(&conn, &new_id), "current", "新知识应成为当前");

        // 回滚：精确恢复，当前知识回到旧知识。
        decide_relation(&mut conn, &relation_id, ReviewAction::Reset, None).unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "current", "回滚后必须恢复为当前");
        assert_eq!(
            claim_relation_repository::list_events(&conn, &relation_id)
                .unwrap()
                .len(),
            2,
            "回滚必须留下新事件（K-009）"
        );
    }

    /// contradicts 被接受：不改变任何 Claim 的当前状态（K-007）。
    #[test]
    fn e2e_accepting_contradicts_keeps_both_current() {
        let mut conn = memory_db();
        let (old_id, new_id, relation_id) = seed_supersede_case(&mut conn);

        decide_relation(&mut conn, &relation_id, ReviewAction::Accept, None).unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "current");
        assert_eq!(lifecycle_of(&conn, &new_id), "current");
        assert_eq!(claim_repository::count_superseded(&conn).unwrap(), 0);
    }

    /// 拒绝：什么都不改。
    #[test]
    fn e2e_rejecting_changes_nothing() {
        let mut conn = memory_db();
        let (old_id, new_id, relation_id) = seed_supersede_case(&mut conn);

        decide_relation(&mut conn, &relation_id, ReviewAction::Reject, None).unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "current");
        assert_eq!(lifecycle_of(&conn, &new_id), "current");
    }

    /// duplicates 自动接受，且不产生历史（分析阶段的确定性结论）。
    #[test]
    fn e2e_duplicate_is_auto_accepted_without_creating_history() {
        let mut conn = memory_db();
        let first = seed_document(&mut conn, "First", "SQLite 被本项目使用。");
        let old = knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &first),
        )
        .unwrap();
        let second = seed_document(&mut conn, "Second", "本项目使用了 SQLite。");
        let new = knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(report.duplicates, 1);
        assert_eq!(report.verdicts[0].status, "accepted");
        assert_eq!(lifecycle_of(&conn, &old.id), "current");
        assert_eq!(lifecycle_of(&conn, &new.id), "current");
        assert_eq!(claim_repository::count_superseded(&conn).unwrap(), 0);
    }

    /// §50：coexists（多值谓语、宾语不同）自动接受，双方都是当前知识。
    #[test]
    fn e2e_coexists_keeps_both_current() {
        let mut conn = memory_db();
        let first = seed_document(&mut conn, "First", "wiki-ya 使用 SQLite。");
        let a = knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &first),
        )
        .unwrap();

        let second = seed_document(&mut conn, "Second", "wiki-ya 使用 Vite。");
        let b = knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("Vite"), &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(report.verdicts[0].relationship, "coexists");
        assert_eq!(report.verdicts[0].status, "accepted", "并存无需打扰用户");
        assert_eq!(lifecycle_of(&conn, &a.id), "current");
        assert_eq!(lifecycle_of(&conn, &b.id), "current");
    }

    /// §11 / §50：supplements 不改变当前知识；接受后双方都保持当前。
    #[test]
    fn e2e_supplements_keeps_both_current() {
        let mut conn = memory_db();
        let (old_id, new_id, _) = seed_supersede_case(&mut conn);

        // supplements 不由确定性规则产生（需 LLM / 人工），因此直接建关系。
        let relation_id = claim_relation_repository::insert(
            &conn,
            &ClaimId::from_raw(&new_id),
            &ClaimId::from_raw(&old_id),
            ClaimRelationType::Supplements,
            ClaimRelationStatus::Candidate,
            Some(0.6),
            Some("补充信息"),
            Some("review"),
        )
        .unwrap()
        .unwrap();

        let card = decide_relation(&mut conn, &relation_id, ReviewAction::Accept, None).unwrap();
        assert_eq!(card.relationship.as_str(), "supplements");
        assert_eq!(lifecycle_of(&conn, &old_id), "current");
        assert_eq!(lifecycle_of(&conn, &new_id), "current");
        assert_eq!(claim_repository::count_superseded(&conn).unwrap(), 0);
    }

    /// §15 / §50：用户选「Keep both」= 接受为 coexists，而不是取代。
    #[test]
    fn e2e_keep_both_maps_to_accepted_coexists() {
        let mut conn = memory_db();
        let (old_id, new_id, relation_id) = seed_supersede_case(&mut conn);

        let card = decide_relation(
            &mut conn,
            &relation_id,
            ReviewAction::Accept,
            Some(ClaimRelationType::Coexists),
        )
        .unwrap();
        assert_eq!(card.relationship.as_str(), "coexists");
        assert_eq!(card.status.as_str(), "accepted");
        assert_eq!(lifecycle_of(&conn, &old_id), "current");
        assert_eq!(lifecycle_of(&conn, &new_id), "current");
    }

    /// §14 / §50：unclear 不入库，也不改变任何状态。
    #[test]
    fn e2e_unclear_is_never_persisted() {
        let mut conn = memory_db();
        let first = seed_document(&mut conn, "First", "wiki-ya 使用 SQLite。");
        knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("SQLite"), &first),
        )
        .unwrap();

        let second = seed_document(&mut conn, "Second", "一条宾语缺失的含糊陈述。");
        knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", None, &second),
        )
        .unwrap();

        let report = analyze_document(&mut conn, &second).unwrap();
        assert_eq!(report.relations_written, 0, "unclear 不应污染审核队列");
        assert!(report.verdicts.is_empty());
    }

    /// §29 / §30：具名 rollback 保留历史事件并精确恢复。
    #[test]
    fn e2e_named_rollback_restores_and_keeps_history() {
        let mut conn = memory_db();
        let (old_id, _new_id, relation_id) = seed_supersede_case(&mut conn);

        decide_relation(
            &mut conn,
            &relation_id,
            ReviewAction::Accept,
            Some(ClaimRelationType::Supersedes),
        )
        .unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "superseded");

        rollback(&mut conn, &relation_id).unwrap();
        assert_eq!(lifecycle_of(&conn, &old_id), "current");
        assert_eq!(
            claim_relation_repository::list_events(&conn, &relation_id)
                .unwrap()
                .len(),
            2,
            "回滚是新增事件，不是删除历史（I-007）"
        );
    }

    /// 时间有效性到期：不靠任何演化关系，也要被判为历史（Temporal Contract）。
    #[test]
    fn e2e_temporally_expired_claim_becomes_history_without_evolution() {
        let mut conn = memory_db();
        let document = seed_document(&mut conn, "Doc", "旧知识。");
        let card = knowledge_service::create_claim(
            &mut conn,
            claim_input("wiki-ya", "uses", Some("Vite 5"), &document),
        )
        .unwrap();

        conn.execute(
            "UPDATE claims SET valid_until = '2020-01-01 00:00:00' WHERE id = ?1",
            rusqlite::params![card.id],
        )
        .unwrap();

        assert_eq!(
            lifecycle_of(&conn, &card.id),
            "superseded",
            "已过期且无当前知识覆盖时应保留为历史"
        );
        assert_eq!(
            claim_repository::count_superseded(&conn).unwrap(),
            0,
            "时间过期不是 supersedes，不得改写 status"
        );
    }
}
