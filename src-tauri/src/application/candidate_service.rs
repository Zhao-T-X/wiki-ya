//! 候选知识决策（M6）。
//!
//! - `list_by_run`：按 Run 列出候选（Review / Trace 用）；
//! - `decide`：用户决策——
//!   - **accept**：复用 `create_claim`（同一套受控词表校验）落库为 Claim，
//!     并触发 `analyze_document` 产出演化提案（进 Review），候选标记
//!     `accepted` + Claim id；
//!   - **reject**：标记 `rejected` + 原因，留痕不删除。
//!
//! 决策只能由人类发起（命令层直连，不经 Agent/Skill 任何路径——M5 闸门）。

use rusqlite::Connection;

use crate::application::dto::{
    CandidateDto, CandidatePageDto, ClaimCard, CreateClaimInput, DecideCandidateInput,
    ListCandidatesInput,
};
use crate::application::evolution_service;
use crate::application::knowledge_service;
use crate::domain::common::ids::DocumentId;
use crate::domain::knowledge::candidate::{Candidate, CandidateStatus};
use crate::error::{AppError, AppResult};
use crate::infrastructure::candidate_repository;
use crate::infrastructure::candidate_repository::CandidateCursor;

/// 按 Run 列出候选（产生顺序）。
pub fn list_by_run(conn: &Connection, run_id: &str) -> AppResult<Vec<CandidateDto>> {
    Ok(candidate_repository::list_by_run(conn, run_id)?
        .iter()
        .map(to_dto)
        .collect())
}

/// 候选列表的默认单页条数（PERF-04）。
///
/// 取 50：够一屏审阅，又不会让前端一次渲染几百张卡片（那正是之前卡顿的来源之一）。
const DEFAULT_PAGE_SIZE: usize = 50;

/// 按 Run **分页**列出候选（PERF-04：游标分页）。
///
/// 相比一次性 `list_by_run` 的好处：
/// - 首屏只传 50 条，载荷与渲染量都与候选总数**无关**；
/// - 游标（而非 OFFSET）保证翻页代价恒定，且并发插入不漏行/不重复。
pub fn list_page(
    conn: &Connection,
    input: &ListCandidatesInput,
) -> AppResult<CandidatePageDto> {
    let limit = input.limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, 500);
    let cursor = input.cursor.as_deref().and_then(CandidateCursor::parse);

    // 多取一条判断"还有更多"，避免额外发一次 COUNT。
    let fetched =
        candidate_repository::list_by_run_page(conn, &input.id, cursor.as_ref(), limit + 1)?;
    let has_more = fetched.len() > limit;
    let items: Vec<CandidateDto> = fetched.iter().take(limit).map(to_dto).collect();
    let next_cursor = if has_more {
        items
            .last()
            .map(|last| CandidateCursor {
                created_at: last.created_at.clone(),
                id: last.id.clone(),
            })
            .map(|c| c.encode())
    } else {
        None
    };

    Ok(CandidatePageDto { items, next_cursor })
}

/// 用户决策一条候选。
pub fn decide(conn: &mut Connection, input: DecideCandidateInput) -> AppResult<CandidateDto> {
    let candidate = candidate_repository::get(conn, &input.candidate_id)?
        .ok_or_else(|| AppError::NotFound(format!("候选 {} 不存在", input.candidate_id)))?;

    if candidate.status != CandidateStatus::Pending {
        return Err(AppError::Domain(format!(
            "候选 {} 已决策（{}），不能重复决策",
            candidate.id,
            candidate.status.as_str()
        )));
    }

    let accept = input.accept;

    // 接受：复用 create_claim（同一套受控词表校验 + Evidence 落库）。
    //
    // 说明：此前的实现试图在外层 `Transaction` 内调用 `create_claim` /
    // `analyze_document`，但这两个函数各自会开自己的事务，且 `Transaction`
    // 不实现 `DerefMut`，因此该写法根本无法编译（main 长期处于半完成态）。
    // 这里改为直接以 `&mut Connection` 调用，每一步各自保证内部原子性；
    // 跨步骤的强一致（避免「Claim 已落库、演化分析失败」的孤儿 Claim）
    // 留待 M15 用 savepoint 形式补回，不在本次 PR-01 范围内。
    if accept {
        let claim_input = CreateClaimInput {
            subject: candidate.subject.clone(),
            predicate: candidate.predicate.clone(),
            object: candidate.object_text.clone(),
            content: candidate.content.clone(),
            claim_type: candidate.claim_type.clone(),
            polarity: candidate.polarity.clone(),
            modality: candidate.modality.clone(),
            condition: None,
            confidence: candidate.confidence,
            document_id: candidate.document_id.clone(),
            chunk_id: None,
            quote: candidate.source_quote.clone(),
            status: None,
            observed_at: None,
        };
        let claim: ClaimCard = knowledge_service::create_claim(conn, claim_input)?;
        // 触发演化分析：与已有知识比对，冲突/新增提案进 Review。
        let analysis = evolution_service::analyze_document(
            conn,
            &DocumentId::from_raw(&candidate.document_id),
        )?;
        candidate_repository::decide(
            conn,
            &candidate.id,
            CandidateStatus::Accepted,
            Some(&claim.id),
            None,
        )?;
        let _ = analysis;
    } else {
        candidate_repository::decide(
            conn,
            &candidate.id,
            CandidateStatus::Rejected,
            None,
            input.reason.as_deref().or(Some("用户拒绝")),
        )?;
    }

    let updated = candidate_repository::get(conn, &candidate.id)?.unwrap_or(candidate);
    Ok(to_dto(&updated))
}

fn to_dto(candidate: &Candidate) -> CandidateDto {
    CandidateDto {
        id: candidate.id.clone(),
        run_id: candidate.run_id.clone(),
        document_id: candidate.document_id.clone(),
        subject: candidate.subject.clone(),
        predicate: candidate.predicate.clone(),
        object_text: candidate.object_text.clone(),
        content: candidate.content.clone(),
        claim_type: candidate.claim_type.clone(),
        polarity: candidate.polarity.clone(),
        modality: candidate.modality.clone(),
        confidence: candidate.confidence,
        source_chunk_index: candidate.source_chunk_index,
        source_quote: candidate.source_quote.clone(),
        sentence: candidate.sentence.clone(),
        support_level: candidate.support_level.as_str().to_string(),
        status: candidate.status.as_str().to_string(),
        accepted_claim_id: candidate.accepted_claim_id.clone(),
        reject_reason: candidate.reject_reason.clone(),
        created_at: candidate.created_at.clone(),
    }
}
