//! 存量库迁移（Phase 8，TDD §71/§72）：Backup → Probe → Import → Validation。
//!
//! 设计要点：
//! - **原数据只读**：源库以 `SQLITE_OPEN_READ_ONLY` 打开；迁移前先备份目标库。
//! - **复用既有用例**：文档走 `capture_service::create_document`（content_hash
//!   幂等 + 确定性重切片），Claim 走 `knowledge_service::create_claim`
//!   （受控词表校验 + 实体消解）——迁移不绕过任何领域不变量。
//! - **诚实呈现**：跳过与失败全部带原因进 `notes`，绝不静默丢弃数据。

use std::path::Path;
use std::collections::HashMap;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::application::capture_service;
use crate::application::dto::{
    CreateClaimInput, CreateDocumentInput, MigrationProbe, MigrationReport,
};
use crate::application::knowledge_service;
use crate::error::AppError;
use crate::error::AppResult;
use crate::infrastructure::db;
use crate::infrastructure::migration_repository as repo;

/// 只读探测源库：报告可迁移的表与数量。
pub fn probe(source_path: &Path) -> AppResult<MigrationProbe> {
    let source = repo::open_source(source_path)?;
    let tables = repo::sqlite_tables(&source)?;
    let has_documents = repo::table_exists(&source, "documents");
    let has_entities = repo::table_exists(&source, "entities");
    let has_claims = repo::table_exists(&source, "claims");
    let document_count = if has_documents {
        repo::count(&source, "documents")
    } else {
        0
    };
    let claim_count = if has_claims {
        repo::count(&source, "claims")
    } else {
        0
    };

    // 光有表还不够：导入依赖具体列。缺列时 `source_claims` 会中途报错并留下半成品，
    // 因此这里必须把「必需列存在」一并计入 compatible，避免谎报兼容。
    let documents_ok = has_documents
        && repo::column_exists(&source, "documents", "id")
        && repo::column_exists(&source, "documents", "title")
        && repo::column_exists(&source, "documents", "content");
    let entities_ok = has_entities
        && repo::column_exists(&source, "entities", "id")
        && repo::column_exists(&source, "entities", "name");
    let claims_ok = has_claims
        && repo::column_exists(&source, "claims", "subject_id")
        && repo::column_exists(&source, "claims", "predicate")
        && repo::column_exists(&source, "claims", "object_id")
        && repo::column_exists(&source, "claims", "object_text")
        && repo::column_exists(&source, "claims", "content")
        && repo::column_exists(&source, "claims", "document_id");

    Ok(MigrationProbe {
        source_path: source_path.to_string_lossy().to_string(),
        tables,
        has_documents,
        has_entities,
        has_claims,
        document_count,
        claim_count,
        compatible: documents_ok && entities_ok && claims_ok,
    })
}

/// 迁移前备份目标库（TDD §71「先 backup 再 import」）。
pub fn backup(db_path: &Path) -> AppResult<String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let file_name = db_path
        .file_name()
        .map(|name| format!("{}.bak-{stamp}", name.to_string_lossy()))
        .unwrap_or_else(|| format!("wikiya.bak-{stamp}.db"));
    let backup_path = db_path.with_file_name(file_name);
    let target = backup_path.to_string_lossy().to_string();
    // 库运行在 WAL 模式：直接 `fs::copy` 会漏掉尚未 checkpoint 的已提交事务，
    // 备份可能缺失最新数据。用 `VACUUM INTO` 让 SQLite 自己导出一致性快照。
    let conn = db::open(db_path)?;
    conn.execute("VACUUM INTO ?1", rusqlite::params![target])
        .map_err(|err| AppError::Internal(format!("备份目标库失败：{err}")))?;
    Ok(target)
}

/// 导入：documents（幂等 + 自动重切片）→ claims（实体名重建 + 领域校验）。
pub fn import(target: &mut Connection, source_path: &Path) -> AppResult<MigrationReport> {
    let source = repo::open_source(source_path)?;
    let mut report = MigrationReport {
        source_path: source_path.to_string_lossy().to_string(),
        backup_path: None,
        documents_imported: 0,
        documents_skipped: 0,
        claims_imported: 0,
        claims_skipped: 0,
        notes: Vec::new(),
    };

    // 1) documents：建立 source_id → target_id 映射（claims 需要挂靠文档）。
    let mut document_map: HashMap<String, String> = HashMap::new();
    for (source_id, title, content, source_hash) in repo::source_documents(&source)? {
        let content_hash = source_hash.unwrap_or_else(|| sha256_hex(&content));
        match repo::document_exists_by_hash(target, &content_hash) {
            Ok(true) => {
                report.documents_skipped += 1;
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                report.documents_skipped += 1;
                report
                    .notes
                    .push(format!("文档 {source_id} 幂等检查失败：{err}"));
                continue;
            }
        }

        let input = CreateDocumentInput {
            title,
            content,
            source_type: Some("note".into()),
            source_uri: None,
            metadata: Some(json!({ "migratedFrom": source_id })),
        };
        match capture_service::create_document(target, input) {
            Ok(summary) => {
                document_map.insert(source_id, summary.id);
                report.documents_imported += 1;
            }
            Err(err) => {
                report.documents_skipped += 1;
                report.notes.push(format!("文档「{source_id}」导入失败：{err}"));
            }
        }
    }

    // 2) claims：subject/object 经实体名重建；重复 (s, p, o) 幂等跳过。
    for (subject, predicate, object_name, object_text, content, source_document_id) in
        repo::source_claims(&source)?
    {
        let object_display = object_name.clone().or_else(|| object_text.clone());
        match repo::claim_exists(target, &subject, &predicate, object_display.as_deref()) {
            Ok(true) => {
                report.claims_skipped += 1;
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                report.claims_skipped += 1;
                report
                    .notes
                    .push(format!("Claim「{subject} {predicate}」幂等检查失败：{err}"));
                continue;
            }
        }

        // Claim 必须挂靠文档（Evidence 的落点）；源文档未迁移的诚实跳过。
        let document_id = source_document_id
            .as_ref()
            .and_then(|id| document_map.get(id).cloned());
        let Some(document_id) = document_id else {
            report.claims_skipped += 1;
            report.notes.push(format!(
                "Claim「{subject} {predicate}」缺少已迁移的来源文档，已跳过"
            ));
            continue;
        };

        let claim_label = format!("{subject} {predicate}");
        let input = CreateClaimInput {
            subject,
            predicate,
            object: object_display,
            content,
            claim_type: None,
            polarity: None,
            modality: None,
            condition: None,
            confidence: None,
            document_id,
            chunk_id: None,
            quote: None,
            status: None,
        };
        match knowledge_service::create_claim(target, input) {
            Ok(_) => report.claims_imported += 1,
            Err(err) => {
                report.claims_skipped += 1;
                report
                    .notes
                    .push(format!("Claim「{claim_label}」导入失败：{err}"));
            }
        }
    }

    Ok(report)
}

fn sha256_hex(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

use rusqlite::Connection;
