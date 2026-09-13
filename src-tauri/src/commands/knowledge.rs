//! Knowledge（Entity / Claim / Evidence）相关命令。

use tauri::State;

use crate::application::dto::{
    ClaimCard, ClaimDetail, ClaimRelationCard, CreateClaimInput, EntityCard, EntityDetail,
    EvidenceCard, GetEntityInput, IdInput, ListClaimsInput, ListEntitiesInput, ListEvidenceInput,
};
use crate::application::knowledge_service;
use crate::domain::common::ids::{ClaimId, EntityId};
use crate::error::AppError;
use crate::AppState;

/// 实体列表。
#[tauri::command]
pub fn list_entities(
    state: State<'_, AppState>,
    input: ListEntitiesInput,
) -> Result<Vec<EntityCard>, AppError> {
    let conn = state.open()?;
    knowledge_service::list_entities(
        &conn,
        input.query.as_deref(),
        input.entity_type.as_deref(),
        input.limit.unwrap_or(100),
    )
}

/// 实体详情（别名 + Claim + 关系 + 邻域图）。
#[tauri::command]
pub fn get_entity(
    state: State<'_, AppState>,
    input: GetEntityInput,
) -> Result<EntityDetail, AppError> {
    let conn = state.open()?;
    knowledge_service::get_entity(
        &conn,
        &EntityId::from_raw(input.id.trim()),
        input.depth.unwrap_or(1),
    )
}

/// Claim 列表。
#[tauri::command]
pub fn list_claims(
    state: State<'_, AppState>,
    input: ListClaimsInput,
) -> Result<Vec<ClaimCard>, AppError> {
    let conn = state.open()?;
    knowledge_service::list_claims(
        &conn,
        input.subject_id.as_deref(),
        input.predicate.as_deref(),
        input.status.as_deref(),
        input.document_id.as_deref(),
        input.limit.unwrap_or(100),
    )
}

/// Claim 详情（证据 + 演化关系 + 历史）。
#[tauri::command]
pub fn get_claim(state: State<'_, AppState>, input: IdInput) -> Result<ClaimDetail, AppError> {
    let conn = state.open()?;
    knowledge_service::get_claim(&conn, &ClaimId::from_raw(input.id.trim()))
}

/// 某条 Claim 的演化历史。
#[tauri::command]
pub fn get_claim_history(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<Vec<ClaimRelationCard>, AppError> {
    let conn = state.open()?;
    knowledge_service::get_claim_history(&conn, &ClaimId::from_raw(input.id.trim()))
}

/// 手动录入 Claim（AI 关闭时的降级路径，领域校验与抽取路径同源）。
#[tauri::command]
pub fn create_claim(
    state: State<'_, AppState>,
    input: CreateClaimInput,
) -> Result<ClaimCard, AppError> {
    let mut conn = state.open()?;
    knowledge_service::create_claim(&mut conn, input)
}

/// 某条 Claim 的证据。
#[tauri::command]
pub fn list_evidence(
    state: State<'_, AppState>,
    input: ListEvidenceInput,
) -> Result<Vec<EvidenceCard>, AppError> {
    let conn = state.open()?;
    knowledge_service::list_evidence(&conn, &ClaimId::from_raw(input.claim_id.trim()))
}
