//! Skill Runtime（M2）。
//!
//! 执行模型：`run_skill` 命令 → 登记 Skill Run（`runs` 表，`run_type='skill'`，
//! 可挂 `parent_run_id`）→ 立即返回 `run_id` → 后台线程同步执行 → 全程发
//! 统一 `RunEvent`。
//!
//! 三个内置 Skill 都是**既有服务的包装**（不推倒重来）：
//! - `knowledge-extraction`  → `ai_service::extract_claims`（预览，不落库）
//! - `knowledge-answering`   → `ask_service::ask`（带引用回答）
//! - `knowledge-correction`  → `evolution_service::analyze_document`（提案进 Review）
//!
//! 诚实边界不变：Skill 产物是候选/答案/提案，落库永远经 Review（M5 起
//! 由 Policy 闸门强制；M2 阶段权限已声明并写入 Run metadata）。

use std::path::PathBuf;

use rusqlite::Connection;
use std::str::FromStr;

use serde_json::{json, Value};

use crate::application::ai_service;
use crate::application::ask_service;
use crate::application::dto::{AskRequest, SkillDescriptorDto};
use crate::application::evolution_service;
use crate::domain::extraction::ExtractionRunStatus;
use crate::domain::run::RunType;
use crate::domain::common::ids::DocumentId;
use crate::domain::skill::{registry, SkillName};
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use crate::infrastructure::{db, run_repository};

/// 枚举内置 Skill（前端 Agent/Skill 面板用）。
pub fn list_skills() -> Vec<SkillDescriptorDto> {
    registry()
        .into_iter()
        .map(|d| SkillDescriptorDto {
            name: d.name.as_str().to_string(),
            description: d.description.to_string(),
            permissions: d
                .permissions
                .iter()
                .map(|p| p.as_str().to_string())
                .collect(),
            input_hint: d.input_hint.to_string(),
        })
        .collect()
}

/// 启动一次 Skill Run：登记 + 立即返回 run_id，执行在后台完成。
pub fn start_skill(
    app: tauri::AppHandle,
    db_path: PathBuf,
    name: &str,
    input: Value,
    parent_run_id: Option<String>,
) -> AppResult<String> {
    let skill = SkillName::from_str(name)
        .map_err(|_| AppError::Domain(format!("未知 Skill `{name}`")))?;
    let descriptor = registry()
        .into_iter()
        .find(|d| d.name == skill)
        .expect("内置 Skill 一定在注册表中");

    let run_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db::open(&db_path)?;
        let metadata = json!({
            "input": input,
            "permissions": descriptor
                .permissions
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
        })
        .to_string();
        run_repository::register(
            &conn,
            &run_id,
            RunType::Skill,
            name,
            parent_run_id.as_deref(),
            &metadata,
        )?;
    }

    // 命令立刻返回；Skill 在后台线程同步执行（内部是阻塞 SQLite + HTTP）。
    let run_id_for_task = run_id.clone();
    let name_for_task = name.to_string();
    let run_events: RunSink = {
        let app = app.clone();
        std::sync::Arc::new(move |event: &RunEvent| {
            let _ = tauri::Emitter::emit(&app, "run-events", event);
        })
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _ = execute_sync(&db_path, &run_id_for_task, &name_for_task, input, &run_events);
    });

    Ok(run_id)
}

/// 同步执行体（`start_skill` 的后台部分；公开以便测试）。
pub fn execute_sync(
    db_path: &PathBuf,
    run_id: &str,
    name: &str,
    input: Value,
    sink: &RunSink,
) -> AppResult<()> {
    let conn = db::open(db_path)?;
    run_repository::set_status(&conn, run_id, ExtractionRunStatus::Running)?;
    sink(&RunEvent::Started {
        run_id: run_id.to_string(),
        run_type: RunType::Skill,
    });

    let mut conn = conn;
    let result = execute_inner(&mut conn, run_id, name, input, sink);

    match result {
        Ok(summary) => {
            run_repository::set_metadata(&conn, run_id, &summary.to_string())?;
            run_repository::finish(&conn, run_id, ExtractionRunStatus::Completed, None, None)?;
            sink(&RunEvent::Completed {
                run_id: run_id.to_string(),
            });
            Ok(())
        }
        Err(err) => {
            let message = err.to_string();
            run_repository::finish(
                &conn,
                run_id,
                ExtractionRunStatus::Failed,
                Some("SKILL_FAILED"),
                Some(&message),
            )?;
            sink(&RunEvent::Failed {
                run_id: run_id.to_string(),
                error: message,
            });
            Err(err)
        }
    }
}

/// 三个内置 Skill 的分派：全部委托既有 Application 服务。
fn execute_inner(
    conn: &mut Connection,
    run_id: &str,
    name: &str,
    input: Value,
    sink: &RunSink,
) -> AppResult<Value> {
    let skill = SkillName::from_str(name)
        .map_err(|_| AppError::Domain(format!("未知 Skill `{name}`")))?;

    match skill {
        SkillName::KnowledgeExtraction => {
            let document_id = input_string(&input, "documentId")?;
            let report = ai_service::extract_claims(conn, &document_id)?;
            let accepted = report.extracted.iter().filter(|c| c.accepted).count();
            sink(&RunEvent::CandidateCreated {
                run_id: run_id.to_string(),
                count: accepted,
            });
            Ok(json!({
                "candidates": accepted,
                "total": report.extracted.len(),
                "enabled": report.enabled,
            }))
        }
        SkillName::KnowledgeAnswering => {
            let question = input_string(&input, "question")?;
            let role = input
                .get("role")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let request = AskRequest {
                question,
                role,
                // 复用 skill run_id 作为流式事件 id：前端订阅 run-events 即可看到增量。
                run_id: Some(run_id.to_string()),
            };
            let response = ask_service::ask(conn, request, Some(sink))?;
            Ok(json!({
                "answer": response.answer,
                "sources": response.sources.len(),
                "enabled": response.enabled,
            }))
        }
        SkillName::KnowledgeCorrection => {
            let document_id = input_string(&input, "documentId")?;
            let analysis =
                evolution_service::analyze_document(conn, &DocumentId::from_raw(&document_id))?;
            sink(&RunEvent::ProposalCreated {
                run_id: run_id.to_string(),
                count: analysis.relations_written as usize,
            });
            Ok(json!({
                "proposals": analysis.relations_written,
                "scanned": analysis.claims_scanned,
            }))
        }
    }
}

fn input_string(input: &Value, key: &str) -> AppResult<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Domain(format!("Skill 输入缺少字段 `{key}`")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skill::SkillPermission;

    #[test]
    fn registry_declares_three_skills_with_read() {
        let skills = registry();
        assert_eq!(skills.len(), 3);
        for d in &skills {
            assert!(
                d.has_permission(SkillPermission::Read),
                "{} 必须至少声明 READ",
                d.name
            );
        }
        // 产知识的 Skill 必须声明 PROPOSE（提案进 Review，而不是直接改库）。
        let extraction = skills
            .iter()
            .find(|d| d.name == SkillName::KnowledgeExtraction)
            .unwrap();
        assert!(extraction.has_permission(SkillPermission::Propose));
        // 纯问答 Skill 不允许 PROPOSE。
        let answering = skills
            .iter()
            .find(|d| d.name == SkillName::KnowledgeAnswering)
            .unwrap();
        assert!(!answering.has_permission(SkillPermission::Propose));
    }

    #[test]
    fn unknown_skill_is_rejected() {
        assert!(SkillName::from_str("make-coffee").is_err());
    }

    #[test]
    fn execute_sync_marks_failed_on_missing_input() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        // execute_sync 自开连接，需要真实文件库。
        drop(conn);
        crate::infrastructure::db::initialize(&db_path).unwrap();

        let sink: RunSink = std::sync::Arc::new(|_| {});
        let run_id = "run-test-1";
        {
            let conn = db::open(&db_path).unwrap();
            run_repository::register(
                &conn,
                run_id,
                RunType::Skill,
                "knowledge-extraction",
                None,
                "{}",
            )
            .unwrap();
        }

        let err = execute_sync(
            &db_path,
            run_id,
            "knowledge-extraction",
            json!({}),
            &sink,
        )
        .unwrap_err();
        assert!(err.to_string().contains("documentId"));

        let conn = db::open(&db_path).unwrap();
        let run = run_repository::get(&conn, run_id).unwrap().unwrap();
        assert_eq!(run.status, ExtractionRunStatus::Failed);
    }

    #[test]
    fn execute_sync_answering_honestly_completes_without_ai() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        crate::infrastructure::db::initialize(&db_path).unwrap();

        let sink: RunSink = std::sync::Arc::new(|_| {});
        let run_id = "run-test-2";
        {
            let conn = db::open(&db_path).unwrap();
            run_repository::register(
                &conn,
                run_id,
                RunType::Skill,
                "knowledge-answering",
                None,
                "{}",
            )
            .unwrap();
        }

        execute_sync(
            &db_path,
            run_id,
            "knowledge-answering",
            json!({ "question": "测试问题" }),
            &sink,
        )
        .unwrap();

        let conn = db::open(&db_path).unwrap();
        let run = run_repository::get(&conn, run_id).unwrap().unwrap();
        assert_eq!(run.status, ExtractionRunStatus::Completed);
        // 无 AI 时诚实完成：metadata 里 enabled=false，不伪造答案。
        let metadata: Value = serde_json::from_str(&run.metadata).unwrap();
        assert_eq!(metadata["enabled"], json!(false));
    }
}
