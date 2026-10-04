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

use rusqlite::{Connection, OptionalExtension};
use std::str::FromStr;

use serde_json::{json, Value};

use crate::ai::rig_adapter::RigAdapter;
use crate::application::ai_service;
use crate::application::ask_service;
use crate::application::dto::{AskRequest, SkillDescriptorDto};
use crate::application::evolution_service;
use crate::domain::common::ids::DocumentId;
use crate::domain::extraction::ExtractionRunStatus;
use crate::domain::run::RunType;
use crate::domain::skill::{SkillDefinition, SkillName};
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use crate::infrastructure::{db, document_repository, run_repository};

/// 枚举内置 Skill（前端 Agent/Skill 面板用）。
pub fn list_skills(conn: &Connection) -> AppResult<Vec<SkillDescriptorDto>> {
    let mut stmt = conn.prepare(
        "SELECT s.name, s.description, s.current_version, v.instructions, v.input_hint, \
         v.permissions FROM skills s JOIN skill_versions v \
         ON v.skill_name = s.name AND v.version = s.current_version ORDER BY s.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (name, description, version, instructions, input_hint, permissions) = row?;
        out.push(SkillDescriptorDto {
            name,
            description,
            version,
            instructions,
            permissions: serde_json::from_str(&permissions).unwrap_or_default(),
            input_hint,
        });
    }
    Ok(out)
}

/// 按名字解析当前版本的 Skill 定义（start_skill 与 M4 Agent Profile 共用）。
pub fn resolve(conn: &Connection, name: &str) -> AppResult<SkillDefinition> {
    let row = conn
        .query_row(
            "SELECT s.description, s.current_version, v.instructions, v.input_hint, \
             v.output_hint, v.tools, v.permissions FROM skills s JOIN skill_versions v \
             ON v.skill_name = s.name AND v.version = s.current_version WHERE s.name = ?1",
            rusqlite::params![name],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("Skill `{name}` 未注册")))?;

    let (description, version, instructions, input_hint, output_hint, tools, permissions) = row;
    Ok(SkillDefinition {
        name: name.to_string(),
        version,
        description,
        instructions,
        input_hint,
        output_hint,
        tools: serde_json::from_str(&tools).unwrap_or_default(),
        permissions: serde_json::from_str(&permissions).unwrap_or_default(),
    })
}

/// 启动时把仓库里的内置 SKILL.md 幂等 seed 进 DB（单一事实来源在仓库）。
pub fn ensure_builtin_skills(conn: &Connection) -> AppResult<usize> {
    const BUILTINS: &[&str] = &[
        include_str!("../../skills/knowledge-extraction/SKILL.md"),
        include_str!("../../skills/knowledge-answering/SKILL.md"),
        include_str!("../../skills/knowledge-correction/SKILL.md"),
    ];
    let mut seeded = 0;
    for text in BUILTINS {
        let parsed = parse_skill_md(text)?;
        let permissions =
            serde_json::to_string(&parsed.permissions).unwrap_or_else(|_| "[]".into());
        let tools = serde_json::to_string(&parsed.tools).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO skills(name, description, current_version, is_builtin) \
             VALUES (?1, ?2, ?3, 1) \
             ON CONFLICT(name) DO UPDATE SET description = excluded.description, \
             current_version = excluded.current_version, is_builtin = 1",
            rusqlite::params![parsed.name, parsed.description, parsed.version],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO skill_versions(skill_name, version, instructions, input_hint, \
             output_hint, tools, permissions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                parsed.name,
                parsed.version,
                parsed.instructions,
                parsed.input_hint,
                parsed.output_hint,
                tools,
                permissions
            ],
        )?;
        seeded += 1;
    }
    Ok(seeded)
}

/// 登记一个 Skill Run（不执行）。版本化 actor = `skill@version`（M3）。
fn register_skill_run(
    conn: &Connection,
    name: &str,
    input: &Value,
    parent_run_id: Option<&str>,
) -> AppResult<String> {
    // 存在性校验：内置与自定义 Skill 一视同仁（M11）。
    resolve(conn, name)?;
    // 版本化标识（M3）：Trace 可回答「这条知识当时是哪个版本的 Skill 产生的」。
    let definition = resolve(conn, name)?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let metadata = json!({
        "input": input,
        "permissions": definition
            .permissions
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
    })
    .to_string();
    run_repository::register(
        conn,
        &run_id,
        RunType::Skill,
        &definition.qualified_name(),
        parent_run_id,
        &metadata,
    )?;
    Ok(run_id)
}

/// 同步运行一个 Skill：登记 + 执行（Agent 编排 M4 复用）。
/// 返回 `(run_id, 结果摘要)`；失败时 Run 已落 Failed 态，错误上抛。
pub fn run_skill_sync(
    db_path: &PathBuf,
    name: &str,
    input: Value,
    parent_run_id: Option<&str>,
    sink: &RunSink,
) -> AppResult<(String, Value)> {
    let conn = db::open(db_path)?;
    let run_id = register_skill_run(&conn, name, &input, parent_run_id)?;
    drop(conn);
    let summary = execute_skill_run(db_path, &run_id, name, input, sink)?;
    Ok((run_id, summary))
}

/// 启动一次 Skill Run：登记 + 立即返回 run_id，执行在后台完成。
pub fn start_skill(
    app: tauri::AppHandle,
    db_path: PathBuf,
    name: &str,
    input: Value,
    parent_run_id: Option<String>,
) -> AppResult<String> {
    let run_id = {
        let conn = db::open(&db_path)?;
        register_skill_run(&conn, name, &input, parent_run_id.as_deref())?
    };

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
        let _ = execute_skill_run(
            &db_path,
            &run_id_for_task,
            &name_for_task,
            input,
            &run_events,
        );
    });

    Ok(run_id)
}

/// 同步执行已登记的 Skill Run，返回结果摘要（公开以便测试与编排）。
pub fn execute_skill_run(
    db_path: &PathBuf,
    run_id: &str,
    name: &str,
    input: Value,
    sink: &RunSink,
) -> AppResult<Value> {
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
            Ok(summary)
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
    // 内置 Skill：确定性服务包装（M2）。
    if let Ok(skill) = SkillName::from_str(name) {
        return match skill {
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
                // M10：提案明细（id/关系/理由）写进 Run metadata——
                // Review 队列可决策，Trace 可回看"当时提了什么、为什么"。
                Ok(json!({
                    "proposals": analysis.relations_written,
                    "scanned": analysis.claims_scanned,
                    "relations": analysis.verdicts.iter().map(|v| json!({
                        "id": v.id,
                        "relationship": v.relationship,
                        "status": v.status,
                        "reason": v.reason,
                        "suggestedAction": v.suggested_action,
                    })).collect::<Vec<_>>(),
                }))
            }
        };
    }

    // 自定义 Skill（M11）：通用 Prompt 执行——**强制只读**，产物只是回答，
    // 不产生候选/提案，Ontology 与 Knowledge Policy 不可能被绕过。
    generic_prompt_execute(conn, name, run_id, &input, sink)
}

/// 自定义 Skill 的通用执行：instructions 作系统提示，输入文本作用户消息。
fn generic_prompt_execute(
    conn: &mut Connection,
    name: &str,
    run_id: &str,
    input: &Value,
    sink: &RunSink,
) -> AppResult<Value> {
    let definition = resolve(conn, name)?;
    let config = crate::ai::config::AiConfig::from_settings(conn);
    if !config.enabled {
        // 诚实降级：不伪造回答。
        return Ok(json!({ "enabled": false, "answer": "" }));
    }

    // 输入：直接文本，或引用一篇文档的原文。
    let text = match input
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(text) => text,
        None => {
            let document_id = input
                .get("documentId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AppError::Domain("自定义 Skill 输入缺少 `text` 或 `documentId`".into())
                })?;
            let document =
                document_repository::find_by_id(conn, &DocumentId::from_raw(document_id))?
                    .ok_or_else(|| AppError::NotFound(format!("文档 {document_id} 不存在")))?;
            document.content
        }
    };

    // M14 PR4：自定义 Skill 的模型调用走 RigAdapter（含流式专用端点兜底）。
    let adapter = RigAdapter::new(config.clone());
    let result = adapter.run_blocking(crate::ai::rig_adapter::AgentRequest {
        goal: text.clone(),
        system: definition.instructions.clone(),
        run_id: run_id.to_string(),
    })?;

    // M14 PR8：PROPOSE 型自定义 Skill——产出候选必须通过与 AI 抽取
    // **完全相同**的抢救解析 + 受控词表校验，才能进 candidates 表（Review）。
    // Ontology 不可能被绕过：词表校验是唯一的落库闸门。
    if definition.has_permission(crate::domain::skill::SkillPermission::Propose) {
        let document_id = input
            .get("documentId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Domain("PROPOSE 型自定义 Skill 输入需要 `documentId`".into())
            })?;
        let system = format!(
            "{}\n\n只输出一个 JSON 对象，形如 {{\"claims\":[ ... ]}}，不要任何解释或 Markdown。",
            definition.instructions
        );
        let adapter2 = RigAdapter::new(config.clone());
        let raw_answer = adapter2.run_blocking(crate::ai::rig_adapter::AgentRequest {
            goal: text,
            system,
            run_id: run_id.to_string(),
        })?;
        let claims = crate::application::ai_service::parse_and_validate_claims(&raw_answer.answer)?;
        let mut accepted = 0usize;
        for claim in &claims {
            let candidate = crate::domain::knowledge::candidate::Candidate {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                document_id: document_id.to_string(),
                subject: claim.subject.clone(),
                predicate: claim.predicate.clone(),
                object_text: claim.object_text.clone(),
                content: claim.content.clone(),
                claim_type: claim.claim_type.clone(),
                polarity: claim.polarity.clone(),
                modality: claim.modality.clone(),
                confidence: claim.confidence,
                source_chunk_index: None,
                source_quote: claim.source_quote.clone(),
                sentence: claim.sentence.clone(),
                support_level: crate::domain::knowledge::candidate::SupportLevel::Unsupported,
                status: crate::domain::knowledge::candidate::CandidateStatus::Pending,
                accepted_claim_id: None,
                reject_reason: claim.reject_reason.clone(),
                created_at: String::new(),
            };
            crate::infrastructure::candidate_repository::insert(conn, &candidate)?;
            accepted += 1;
        }
        sink(&crate::events::RunEvent::CandidateCreated {
            run_id: run_id.to_string(),
            count: accepted,
        });
        return Ok(json!({
            "enabled": true,
            "candidates": accepted,
            "answer": format!("已产出 {accepted} 条候选（进 Review 确认）。"),
        }));
    }

    Ok(json!({ "enabled": true, "answer": result.answer }))
}

fn input_string(input: &Value, key: &str) -> AppResult<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Domain(format!("Skill 输入缺少字段 `{key}`")))
}

/// 解析 SKILL.md：frontmatter（`---` 之间）+ 正文 instructions。
///
/// 只支持本仓库用到的子集：`key: value`、`key: [a, b]` 列表、
/// `key: |` 多行块（后续缩进行归入该字段，直到下一个 key）。
fn parse_skill_md(text: &str) -> AppResult<ParsedSkillMd> {
    let mut parts = text.trim().splitn(3, "---");
    if parts.next().map(str::trim) != Some("") {
        return Err(AppError::Domain(
            "SKILL.md 缺少 frontmatter 开始标记".into(),
        ));
    }
    let fm = parts
        .next()
        .ok_or_else(|| AppError::Domain("SKILL.md 缺少 frontmatter".into()))?;
    let instructions = parts.next().unwrap_or_default().trim().to_string();

    let mut fields: std::collections::HashMap<String, String> = Default::default();
    let mut current_block: Option<String> = None;
    for line in fm.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        if let Some(key) = &current_block {
            if trimmed.starts_with(char::is_whitespace) {
                let entry = fields.get_mut(key).unwrap();
                entry.push('\n');
                entry.push_str(trimmed.trim());
                continue;
            }
            current_block = None;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_string();
            let value = value.trim();
            if value == "|" {
                fields.insert(key.clone(), String::new());
                current_block = Some(key);
            } else if value.starts_with('[') && value.ends_with(']') {
                let list: Vec<String> = value[1..value.len() - 1]
                    .split(',')
                    .map(|item| item.trim().to_string())
                    .filter(|item| !item.is_empty())
                    .collect();
                fields.insert(
                    key,
                    serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()),
                );
            } else {
                fields.insert(key, value.to_string());
            }
        }
    }

    let name = fields
        .get("name")
        .cloned()
        .ok_or_else(|| AppError::Domain("SKILL.md 缺少 name".into()))?;
    let version = fields
        .get("version")
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| AppError::Domain("SKILL.md 缺少有效的 version".into()))?;

    let list_of = |key: &str| -> Vec<String> {
        fields
            .get(key)
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default()
    };

    Ok(ParsedSkillMd {
        name,
        version,
        description: fields.get("description").cloned().unwrap_or_default(),
        permissions: list_of("permissions"),
        tools: list_of("tools"),
        input_hint: fields.get("input").cloned().unwrap_or_default(),
        output_hint: fields.get("output").cloned().unwrap_or_default(),
        instructions,
    })
}

struct ParsedSkillMd {
    name: String,
    version: i64,
    description: String,
    permissions: Vec<String>,
    tools: Vec<String>,
    input_hint: String,
    output_hint: String,
    instructions: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::skill::{registry, SkillPermission};

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

        let err = execute_skill_run(&db_path, run_id, "knowledge-extraction", json!({}), &sink)
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

        execute_skill_run(
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

    #[test]
    fn builtin_skill_md_parses_with_versions() {
        for text in [
            include_str!("../../skills/knowledge-extraction/SKILL.md"),
            include_str!("../../skills/knowledge-answering/SKILL.md"),
            include_str!("../../skills/knowledge-correction/SKILL.md"),
        ] {
            let parsed = parse_skill_md(text).unwrap();
            assert_eq!(parsed.version, 1);
            assert!(parsed.permissions.iter().any(|p| p == "read"));
            assert!(parsed.instructions.contains("Instructions"));
        }
        let extraction =
            parse_skill_md(include_str!("../../skills/knowledge-extraction/SKILL.md")).unwrap();
        assert_eq!(extraction.name, "knowledge-extraction");
        assert!(extraction.permissions.contains(&"propose".into()));
    }

    #[test]
    fn ensure_builtin_skills_is_idempotent_and_resolvable() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();

        assert_eq!(ensure_builtin_skills(&conn).unwrap(), 3);
        // 第二次 seed 不产生重复版本行。
        ensure_builtin_skills(&conn).unwrap();

        let definition = resolve(&conn, "knowledge-extraction").unwrap();
        assert_eq!(definition.version, 1);
        assert_eq!(definition.qualified_name(), "knowledge-extraction@1");
        assert!(definition.has_permission(SkillPermission::Propose));

        let skills = list_skills(&conn).unwrap();
        assert_eq!(skills.len(), 3);
        assert!(skills.iter().all(|d| d.version == 1));

        assert!(resolve(&conn, "no-such-skill").is_err());
    }

    #[test]
    fn custom_skill_crud_versioning_and_builtin_protection() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();

        // 创建：v1，强制只读。
        create_skill(&conn, "paper-extractor", "论文抽取", "抽取论文要点", false).unwrap();
        let def = resolve(&conn, "paper-extractor").unwrap();
        assert_eq!(def.version, 1);
        assert!(!def.has_permission(SkillPermission::Propose));

        // 内置名不可占用。
        assert!(create_skill(&conn, "knowledge-extraction", "x", "y", false).is_err());
        // 名称规则：大写/空格拒绝。
        assert!(create_skill(&conn, "Bad Name", "x", "y", false).is_err());

        // 更新 → 新版本（propose=true）；resolve 拿到 v2 且具备 PROPOSE。
        let v = update_skill(&conn, "paper-extractor", "论文抽取 v2", "新指令", true).unwrap();
        assert_eq!(v, 2);
        let def = resolve(&conn, "paper-extractor").unwrap();
        assert_eq!(def.version, 2);
        assert!(def.has_permission(SkillPermission::Propose));

        // 内置不可改 / 不可删。
        assert!(update_skill(&conn, "knowledge-extraction", "x", "y", false).is_err());
        assert!(delete_skill(&conn, "knowledge-extraction").is_err());

        // 删除自定义。
        delete_skill(&conn, "paper-extractor").unwrap();
        assert!(resolve(&conn, "paper-extractor").is_err());
    }
}

// ---------------------------------------------------------------------------
// Skill 自定义（M11）
// ---------------------------------------------------------------------------

/// 创建自定义 Skill——**强制只读**（permissions 恒为 ["read"]）。
///
/// 域边界（M11）：用户可自定义「怎么处理文本」，但产不出候选/提案，
/// Ontology 与 Knowledge Policy 不可能被绕过。
pub fn create_skill(
    conn: &Connection,
    name: &str,
    description: &str,
    instructions: &str,
    propose: bool,
) -> AppResult<()> {
    crate::domain::skill::validate_custom_name(name).map_err(AppError::Domain)?;
    if description.trim().is_empty() || instructions.trim().is_empty() {
        return Err(AppError::Domain(
            "description 与 instructions 不能为空".into(),
        ));
    }
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM skills WHERE name = ?1)",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .map_err(AppError::from)?;
    if exists {
        return Err(AppError::Domain(format!("Skill `{name}` 已存在")));
    }
    conn.execute(
        "INSERT INTO skills(name, description, current_version, is_builtin) \
         VALUES (?1, ?2, 1, 0)",
        rusqlite::params![name, description.trim()],
    )?;
    conn.execute(
        "INSERT INTO skill_versions(skill_name, version, instructions, permissions) \
         VALUES (?1, 1, ?2, ?3)",
        rusqlite::params![
            name,
            instructions.trim(),
            if propose {
                "[\"read\",\"propose\"]"
            } else {
                "[\"read\"]"
            }
        ],
    )?;
    Ok(())
}

/// 更新自定义 Skill：**产生新版本**（version + 1）——版本化保证 Trace
/// 可回答「当时用的是哪个版本」。内置 Skill 不可修改。
pub fn update_skill(
    conn: &Connection,
    name: &str,
    description: &str,
    instructions: &str,
    propose: bool,
) -> AppResult<i64> {
    if crate::domain::skill::BUILTIN_NAMES.contains(&name) {
        return Err(AppError::Domain("内置 Skill 不可修改".into()));
    }
    if description.trim().is_empty() || instructions.trim().is_empty() {
        return Err(AppError::Domain(
            "description 与 instructions 不能为空".into(),
        ));
    }
    let current: i64 = conn
        .query_row(
            "SELECT current_version FROM skills WHERE name = ?1 AND is_builtin = 0",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("自定义 Skill `{name}` 不存在")))?;
    let new_version = current + 1;
    conn.execute(
        "UPDATE skills SET description = ?2, current_version = ?3 WHERE name = ?1",
        rusqlite::params![name, description.trim(), new_version],
    )?;
    conn.execute(
        "INSERT INTO skill_versions(skill_name, version, instructions, permissions) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            name,
            new_version,
            instructions.trim(),
            if propose {
                "[\"read\",\"propose\"]"
            } else {
                "[\"read\"]"
            }
        ],
    )?;
    Ok(new_version)
}

/// 删除自定义 Skill（内置不可删除；历史 Run 的 metadata 留痕不受影响）。
pub fn delete_skill(conn: &Connection, name: &str) -> AppResult<()> {
    if crate::domain::skill::BUILTIN_NAMES.contains(&name) {
        return Err(AppError::Domain("内置 Skill 不可删除".into()));
    }
    // 先删子行（版本），再删主行——外键顺序不能反。
    conn.execute(
        "DELETE FROM skill_versions WHERE skill_name = ?1",
        rusqlite::params![name],
    )?;
    let changed = conn.execute(
        "DELETE FROM skills WHERE name = ?1 AND is_builtin = 0",
        rusqlite::params![name],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("自定义 Skill `{name}` 不存在")));
    }
    Ok(())
}
