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

use crate::application::ai_service;
use crate::application::ask_service;
use crate::application::dto::{AskRequest, SkillDescriptorDto};
use crate::application::evolution_service;
use crate::domain::extraction::ExtractionRunStatus;
use crate::domain::run::RunType;
use crate::domain::common::ids::DocumentId;
use crate::domain::skill::{registry, SkillDefinition, SkillName};
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use crate::infrastructure::{db, run_repository};

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
        name: SkillName::from_str(name)
            .map_err(|_| AppError::Domain(format!("未知 Skill `{name}`")))?,
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
            "INSERT INTO skills(name, description, current_version) VALUES (?1, ?2, ?3) \
             ON CONFLICT(name) DO UPDATE SET description = excluded.description, \
             current_version = excluded.current_version",
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
    SkillName::from_str(name)
        .map_err(|_| AppError::Domain(format!("未知 Skill `{name}`")))?;
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
        let _ = execute_skill_run(&db_path, &run_id_for_task, &name_for_task, input, &run_events);
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

/// 解析 SKILL.md：frontmatter（`---` 之间）+ 正文 instructions。
///
/// 只支持本仓库用到的子集：`key: value`、`key: [a, b]` 列表、
/// `key: |` 多行块（后续缩进行归入该字段，直到下一个 key）。
fn parse_skill_md(text: &str) -> AppResult<ParsedSkillMd> {
    let mut parts = text.trim().splitn(3, "---");
    if parts.next().map(str::trim) != Some("") {
        return Err(AppError::Domain("SKILL.md 缺少 frontmatter 开始标记".into()));
    }
    let fm = parts.next().ok_or_else(|| AppError::Domain("SKILL.md 缺少 frontmatter".into()))?;
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
                fields.insert(key, serde_json::to_string(&list).unwrap_or_else(|_| "[]".into()));
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

        let err = execute_skill_run(
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
}
