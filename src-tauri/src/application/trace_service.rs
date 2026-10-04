//! Run Trace 查询（M1：Run → Skill → Tool → Result → Trace）。
//!
//! 把一次 Run 的「统一登记 + 类型相关明细」聚合为一个视图：
//! - `agent` → `agent_runs` 的工具调用步骤（agent_events）；
//! - `extraction` → `extraction_runs` 的抽取明细快照；
//! - review / skill 的明细随对应阶段落地后接入（DTO 已预留扩展位）。
//!
//! 这是 M8 Trace UI 的数据源；先以命令形式暴露，前端随时可画时间线。

use rusqlite::{Connection, OptionalExtension};

use crate::ai::accounting::TokenUsage;
use crate::ai::config::AiConfig;
use crate::application::dto::{
    AgentEventDto, CandidateNodeDto, ClaimTraceDto, EvidenceNodeDto, EvolutionNodeDto, RunTraceDto,
};
use crate::application::extraction_service;
use crate::domain::run::RunType;
use crate::error::{AppError, AppResult};
use crate::infrastructure::{extraction_run_repository, run_repository};

/// 读取一条 Run 的完整 Trace。不存在时返回 NotFound。
pub fn get_trace(conn: &Connection, run_id: &str) -> AppResult<RunTraceDto> {
    let run = run_repository::get(conn, run_id)?
        .ok_or_else(|| AppError::NotFound(format!("Run {run_id} 不存在")))?;

    let agent_steps = if run.run_type == RunType::Agent {
        agent_steps(conn, run_id)?
    } else {
        Vec::new()
    };

    let extraction_run = if run.run_type == RunType::Extraction {
        // 该 Run 一定有明细（同一 id 两处登记）。
        Some(extraction_service::to_dto(
            conn,
            &extraction_run_repository::get(conn, run_id)?,
        )?)
    } else {
        None
    };

    // PR-07：解析真实 token 账本并据当前 AI 配置估算成本（未知模型 → None）。
    let usage = run
        .usage_json
        .as_ref()
        .and_then(|s| serde_json::from_str::<TokenUsage>(s).ok());
    let cfg = AiConfig::from_settings(conn);
    let cost_usd = usage.and_then(|u| u.estimate_cost_usd(&cfg.model, &cfg.embedding_model));

    Ok(RunTraceDto {
        id: run.id,
        parent_run_id: run.parent_run_id,
        run_type: run.run_type.as_str().to_string(),
        actor: run.actor,
        status: run.status.as_str().to_string(),
        stage: run.stage,
        started_at: run.started_at,
        finished_at: run.finished_at,
        error_code: run.error_code,
        error_message: run.error_message,
        metadata: serde_json::from_str(&run.metadata).unwrap_or(serde_json::Value::Null),
        agent_steps,
        extraction_run,
        usage,
        cost_usd,
    })
}

/// 读取一次 Agent Run 的步骤明细（按 step_index 升序）。
fn agent_steps(conn: &Connection, run_id: &str) -> AppResult<Vec<AgentEventDto>> {
    let mut stmt = conn.prepare(
        "SELECT step_index, name, status, input_summary, output_text, error_message, created_at \
         FROM agent_events WHERE run_id = ?1 ORDER BY step_index",
    )?;
    let rows = stmt.query_map(rusqlite::params![run_id], |r| {
        Ok(AgentEventDto {
            step_index: r.get(0)?,
            name: r.get(1)?,
            status: r.get(2)?,
            input_summary: r.get(3)?,
            output_text: r.get(4)?,
            error_message: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------------------
// Claim Trace（M8）：Current Knowledge 反向追溯到 Source
// ---------------------------------------------------------------------------

/// 读取一条 Claim 的完整溯源：证据（原文/切片）、产生它的候选与 Run
/// （含 skill@version）、参与的演化关系。四条链一次查询聚合。
pub fn get_claim_trace(conn: &Connection, claim_id: &str) -> AppResult<ClaimTraceDto> {
    // 1) 证据链：evidence → documents / chunks。
    let mut stmt = conn.prepare(
        "SELECT e.id, e.document_id, COALESCE(d.title, ''), e.chunk_id, \
         c.chunk_index, e.quote, c.content FROM evidence e \
         LEFT JOIN documents d ON d.id = e.document_id \
         LEFT JOIN chunks c ON c.id = e.chunk_id \
         WHERE e.claim_id = ?1 ORDER BY e.created_at",
    )?;
    let evidences = rows_to(&mut stmt, rusqlite::params![claim_id], |r| {
        Ok(EvidenceNodeDto {
            evidence_id: r.get(0)?,
            document_id: r.get(1)?,
            document_title: r.get(2)?,
            chunk_id: r.get(3)?,
            chunk_index: r.get(4)?,
            quote: r.get(5)?,
            chunk_text: r.get(6)?,
        })
    })?;

    // 2) 候选链：哪次抽取产生的这条知识。
    let candidate: Option<CandidateNodeDto> = conn
        .query_row(
            "SELECT id, run_id, status, support_level, created_at FROM candidates \
             WHERE accepted_claim_id = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![claim_id],
            |r| {
                Ok(CandidateNodeDto {
                    candidate_id: r.get(0)?,
                    run_id: r.get(1)?,
                    status: r.get(2)?,
                    support_level: r.get(3)?,
                    created_at: r.get(4)?,
                })
            },
        )
        .optional()?;

    // 3) Run 链：候选的产生过程（阶段/步骤/actor = skill@version）。
    let run = match &candidate {
        Some(node) => Some(get_trace(conn, &node.run_id)?),
        None => None,
    };

    // 4) 演化链：该 Claim 参与的关系（新→旧）。
    let mut stmt = conn.prepare(
        "SELECT id, relationship, status, source_claim_id, target_claim_id, reason, created_at \
         FROM claim_relations WHERE source_claim_id = ?1 OR target_claim_id = ?1 \
         ORDER BY created_at DESC",
    )?;
    let evolutions = rows_to(&mut stmt, rusqlite::params![claim_id], |r| {
        Ok(EvolutionNodeDto {
            relation_id: r.get(0)?,
            relationship: r.get(1)?,
            status: r.get(2)?,
            source_claim_id: r.get(3)?,
            target_claim_id: r.get(4)?,
            reason: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;

    Ok(ClaimTraceDto {
        claim_id: claim_id.to_string(),
        evidences,
        candidate,
        run,
        evolutions,
    })
}

/// query_map → Vec 的小帮手（ rusqlite 错误自动转换）。
fn rows_to<T>(
    stmt: &mut rusqlite::Statement<'_>,
    params: impl rusqlite::Params,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> AppResult<Vec<T>> {
    let rows = stmt.query_map(params, map)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::capture_service;
    use crate::application::dto::{CreateClaimInput, CreateDocumentInput};
    use crate::application::knowledge_service;
    use crate::infrastructure::db::tests::memory_db;

    /// 证据绑定了来源切片时，溯源必须把 chunk 原文带回（Chunk 层可见）。
    #[test]
    fn claim_trace_returns_chunk_text_for_evidence() {
        let mut conn = memory_db();
        let doc = capture_service::create_document(
            &mut conn,
            CreateDocumentInput {
                title: "Note".into(),
                content: "wiki-ya 使用 React 19。".into(),
                source_type: None,
                source_uri: None,
                metadata: None,
            },
        )
        .unwrap();
        // 显式插一个 chunk（避免与 create_document 自带切片冲突），作为证据坐标。
        conn.execute(
            "INSERT INTO chunks(id, document_id, chunk_index, start_offset, end_offset, content, char_count) \
             VALUES ('ck-trace', ?1, 99, 0, 12, 'wiki-ya 使用 React 19。', 12)",
            rusqlite::params![doc.id.as_str()],
        )
        .unwrap();

        let input = CreateClaimInput {
            subject: "wiki-ya".into(),
            predicate: "uses".into(),
            object: Some("React 19".into()),
            content: None,
            claim_type: None,
            polarity: None,
            modality: None,
            condition: None,
            confidence: None,
            document_id: doc.id.clone(),
            chunk_id: Some("ck-trace".into()),
            quote: Some("React 19".into()),
            status: None,
            observed_at: None,
        };
        let card = knowledge_service::create_claim(&mut conn, input).unwrap();

        let trace = get_claim_trace(&conn, &card.id).unwrap();
        assert_eq!(trace.evidences.len(), 1, "应有一条证据");
        let evidence = &trace.evidences[0];
        assert_eq!(
            evidence.chunk_text.as_deref(),
            Some("wiki-ya 使用 React 19。"),
            "chunk 原文应被带回"
        );
        assert_eq!(evidence.chunk_index, Some(99));
    }
}

