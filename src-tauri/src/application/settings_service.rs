//! 元信息与受控词表的对外出口。

use std::path::Path;

use rusqlite::Connection;

use crate::ai::config::AiConfig;
use crate::application::dto::{
    AiSettings, AppInfo, EntityTypeOption, HealthReport, Registries, RelationPredicateOption,
    UpdateAiSettings,
};
use crate::infrastructure::settings_repository;
use crate::domain::evidence::evidence::EvidenceLevel;
use crate::domain::knowledge::claim::{ClaimStatus, ClaimType, Modality, Polarity};
use crate::domain::knowledge::document::SourceType;
use crate::domain::knowledge::idea::IdeaStatus;
use crate::domain::knowledge::question::{QuestionStatus, QuestionType};
use crate::domain::ontology::entity::EntityStatus;
use crate::domain::ontology::entity_type::EntityType;
use crate::domain::ontology::event::{EventStatus, EventTimePrecision, EventType};
use crate::domain::ontology::normalization::NormalizationOutcome;
use crate::domain::ontology::predicate::ClaimPredicate;
use crate::domain::ontology::registry;
use crate::domain::ontology::relation::RelationStatus;
use crate::domain::ontology::resolution::EntityResolutionStep;
use crate::domain::evolution::classifier::{ClaimRelationStatus, ClaimRelationType};
use crate::domain::review::review::ReviewTarget;
use crate::domain::search::search::{SearchHitKind, SearchMethod};
use crate::error::AppResult;
use crate::infrastructure::db::{self, count_rows};
use crate::infrastructure::{
    claim_relation_repository, claim_repository, document_repository, entity_repository,
    evidence_repository,
};

/// 应用是否启用了 AI。
///
/// 由持久化设置（settings 表）或环境变量中的 API Key 是否存在决定
/// （见 `ai::config::AiConfig::from_settings`）。UI 必须据此如实展示
/// "AI 未启用"或可用状态，**不得**伪造任何 AI 输出——
/// 这是 PRD「AI suggests, user decides」与 Local-first 承诺的最低要求。
fn ai_enabled(conn: &Connection) -> bool {
    AiConfig::from_settings(conn).enabled
}

pub fn app_info(db_path: &Path, conn: &Connection) -> AppResult<AppInfo> {
    Ok(AppInfo {
        name: "wiki-ya".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        schema_version: db::schema_version(conn)?,
        registry_version: registry::version().to_string(),
        ai_enabled: ai_enabled(conn),
    })
}

/// 读取 AI 运行时设置（不含明文 API Key，仅告知是否已配置）。
///
/// 优先级：持久化设置 > 环境变量 > 默认值（见 `AiConfig::from_settings`）。
pub fn get_ai_settings(conn: &Connection) -> AppResult<AiSettings> {
    let config = AiConfig::from_settings(conn);
    Ok(AiSettings {
        api_key_set: config.api_key.is_some(),
        base_url: config.base_url,
        model: config.model,
        embedding_model: config.embedding_model,
        token_budget: config.token_budget,
    })
}

/// 更新 AI 运行时设置并回读。
///
/// `apiKey` 传空字符串表示显式清除；其余字段仅当提供时才覆盖，便于部分更新。
pub fn update_ai_settings(conn: &mut Connection, req: UpdateAiSettings) -> AppResult<AiSettings> {
    // 整组更新放在一个事务里：避免中途失败留下「一半新一半旧」的 AI 配置。
    let tx = conn.transaction()?;
    if let Some(api_key) = req.api_key {
        settings_repository::set_setting(&tx, "ai.api_key", api_key.trim())?;
    }
    if let Some(base_url) = req.base_url {
        settings_repository::set_setting(&tx, "ai.base_url", base_url.trim())?;
    }
    if let Some(model) = req.model {
        settings_repository::set_setting(&tx, "ai.model", model.trim())?;
    }
    if let Some(embedding_model) = req.embedding_model {
        settings_repository::set_setting(&tx, "ai.embedding_model", embedding_model.trim())?;
    }
    if let Some(token_budget) = req.token_budget {
        settings_repository::set_setting(&tx, "ai.token_budget", &token_budget.to_string())?;
    }
    tx.commit()?;
    get_ai_settings(conn)
}

/// 导出全部受控词表。
///
/// 前端**不允许**硬编码任何枚举值：下拉、校验提示、状态色映射
/// 都要从这里取。这样注册表升级时前端不需要跟着改。
pub fn list_registries() -> AppResult<Registries> {
    let reg = registry::registry();

    Ok(Registries {
        entity_types: reg
            .entity_types
            .iter()
            .map(|entry| EntityTypeOption {
                value: entry.type_name.clone(),
                description: entry.description.clone(),
            })
            .collect(),
        claim_predicates: strings(ClaimPredicate::ALL.iter().map(|v| v.as_str())),
        relation_predicates: reg
            .relation_specs
            .iter()
            .map(|spec| RelationPredicateOption {
                predicate: spec.predicate.clone(),
                inverse_label: spec.inverse_label.clone(),
                symmetric: spec.symmetric,
                transitive: spec.transitive,
                source_types: spec.source_types.clone(),
                target_types: spec.target_types.clone(),
            })
            .collect(),
        claim_types: strings(ClaimType::ALL.iter().map(|v| v.as_str())),
        polarities: strings(Polarity::ALL.iter().map(|v| v.as_str())),
        modalities: strings(Modality::ALL.iter().map(|v| v.as_str())),
        claim_statuses: strings(ClaimStatus::ALL.iter().map(|v| v.as_str())),
        entity_statuses: strings(EntityStatus::ALL.iter().map(|v| v.as_str())),
        relation_statuses: strings(RelationStatus::ALL.iter().map(|v| v.as_str())),
        idea_statuses: strings(IdeaStatus::ALL.iter().map(|v| v.as_str())),
        question_statuses: strings(QuestionStatus::ALL.iter().map(|v| v.as_str())),
        question_types: strings(QuestionType::ALL.iter().map(|v| v.as_str())),
        event_types: strings(EventType::ALL.iter().map(|v| v.as_str())),
        event_statuses: strings(EventStatus::ALL.iter().map(|v| v.as_str())),
        event_time_precisions: strings(EventTimePrecision::ALL.iter().map(|v| v.as_str())),
        research_task_statuses: strings(
            ["open", "running", "completed", "failed"].into_iter(),
        ),
        source_types: strings(SourceType::ALL.iter().map(|v| v.as_str())),
        claim_relation_types: strings(ClaimRelationType::ALL.iter().map(|v| v.as_str())),
        claim_relation_statuses: strings(ClaimRelationStatus::ALL.iter().map(|v| v.as_str())),
        normalization_outcomes: strings(NormalizationOutcome::ALL.iter().map(|v| v.as_str())),
        entity_resolution_steps: strings(EntityResolutionStep::ALL.iter().map(|v| v.as_str())),
        load_strategies: strings(["LOAD", "SUMMARIZE", "RETRIEVE_LATER", "NEVER_LOAD"].into_iter()),
        agent_roles: strings(
            ["personal", "knowledge", "research", "curator", "review", "extractor"].into_iter(),
        ),
        registry_version: registry::version().to_string(),
    })
}

/// 词表浏览器与自检用的补充信息（当前未接入 IPC，保留为内部能力）。
pub fn auxiliary_vocabularies() -> Vec<(&'static str, Vec<String>)> {
    let _ = (EntityType::ALL, EvidenceLevel::ALL, SearchMethod::ALL);
    vec![
        (
            "search_hit_kinds",
            strings(SearchHitKind::ALL.iter().map(|v| v.as_str())),
        ),
        (
            "review_targets",
            strings(ReviewTarget::ALL.iter().map(|v| v.as_str())),
        ),
    ]
}

/// Knowledge Health —— 「我的知识库健康吗」的唯一回答处。
///
/// 所有指标都是**真实计数**，不是估算：一个编造出来的健康分数
/// 比没有分数更糟，因为它会让用户以为系统知道自己在说什么。
pub fn knowledge_health(conn: &Connection) -> AppResult<HealthReport> {
    Ok(HealthReport {
        total_documents: document_repository::count(conn)?,
        total_chunks: document_repository::count_chunks(conn)?,
        total_entities: entity_repository::count(conn)?,
        total_claims: claim_repository::count(conn)?,
        total_evidence: evidence_repository::count(conn)?,
        potential_duplicates: claim_relation_repository::count_potential_duplicates(conn)?,
        unresolved_conflicts: claim_relation_repository::count_unresolved_conflicts(conn)?,
        claims_without_evidence: claim_repository::count_without_evidence(conn)?,
        unresolved_entities: entity_repository::count_unresolved(conn)?,
        superseded_claims: claim_repository::count_superseded(conn)?,
    })
}

/// 全库计数（供测试与诊断使用）。
pub fn total_rows(conn: &Connection) -> AppResult<Vec<(&'static str, i64)>> {
    let mut rows = Vec::new();
    for table in ["documents", "chunks", "entities", "claims", "evidence", "relations"] {
        rows.push((table, count_rows(conn, table)?));
    }
    Ok(rows)
}

fn strings<'a, I: Iterator<Item = &'a str>>(values: I) -> Vec<String> {
    values.map(|value| value.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db::tests::memory_db;

    #[test]
    fn registries_expose_every_controlled_vocabulary() {
        let registries = list_registries().unwrap();
        assert_eq!(registries.entity_types.len(), 14);
        assert_eq!(registries.claim_predicates.len(), 46);
        assert_eq!(registries.relation_predicates.len(), 19);
        assert_eq!(registries.claim_relation_types.len(), 6);
        assert!(!registries.registry_version.is_empty());
        // 每个实体类型都要有描述，否则词表浏览器会显示空白
        assert!(registries
            .entity_types
            .iter()
            .all(|option| !option.description.is_empty()));
    }

    #[test]
    fn relation_predicate_options_carry_their_constraints() {
        let registries = list_registries().unwrap();
        let trained_on = registries
            .relation_predicates
            .iter()
            .find(|spec| spec.predicate == "trained_on")
            .unwrap();
        assert_eq!(trained_on.source_types, vec!["Model".to_string()]);
        assert!(!trained_on.symmetric);
    }

    #[test]
    fn health_report_starts_empty_and_tracks_real_counts() {
        let conn = memory_db();
        let empty = knowledge_health(&conn).unwrap();
        assert_eq!(empty.total_documents, 0);
        assert_eq!(empty.claims_without_evidence, 0);

        conn.execute(
            "INSERT INTO documents(id,title,content,content_hash) VALUES('d1','t','c','h1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO entities(id,name,primary_type) VALUES('e1','Rust','Technology')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO claims(id,subject_id,predicate) VALUES('c1','e1','supports')",
            [],
        )
        .unwrap();

        let report = knowledge_health(&conn).unwrap();
        assert_eq!(report.total_documents, 1);
        assert_eq!(report.total_entities, 1);
        assert_eq!(report.total_claims, 1);
        assert_eq!(report.claims_without_evidence, 1, "没有证据必须被计入");
    }

    #[test]
    fn app_info_reports_the_registry_fingerprint_and_schema_version() {
        let conn = memory_db();
        let info = app_info(std::path::Path::new("/tmp/test.db"), &conn).unwrap();
        assert_eq!(info.name, "wiki-ya");
        assert_eq!(info.registry_version, registry::version());
        assert!(!info.ai_enabled, "未配置 API Key 时 AI 必须报告为未启用");
        assert!(info.db_path.ends_with("test.db"));
    }

    #[test]
    fn helper_vocabularies_are_available_for_the_settings_page() {
        let vocabularies = auxiliary_vocabularies();
        assert_eq!(vocabularies.len(), 2);
        assert_eq!(vocabularies[0].1.len(), 4);
        assert_eq!(vocabularies[1].1.len(), 7);
    }
}
