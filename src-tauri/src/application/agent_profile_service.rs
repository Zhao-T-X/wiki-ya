//! Agent Profile 编排（M4）。
//!
//! Agent Run 的语义：**按 Profile 声明的顺序执行它拥有的 Skill**，
//! 每个 Skill 是一个子 Skill Run（`parent_run_id` 挂到 Agent Run），
//! 结果聚合写进 Agent Run 的 metadata——Trace 链一条 JOIN 串起来：
//!
//! ```text
//! Agent Run (agent)
//!    └── Skill Run #1 (skill, parent=agent)
//!    └── Skill Run #2 (skill, parent=agent)
//! ```
//!
//! 权限闸门雏形（M5 完善）：Skill 的 permissions 必须是 Profile policy
//! 的子集才被执行；超限的 Skill 诚实标记 `skipped: policy`，绝不静默执行。

use std::path::PathBuf;
use std::str::FromStr;

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::domain::agent_profile::AgentProfile;
use crate::domain::policy::Policy;
use crate::domain::extraction::ExtractionRunStatus;
use crate::domain::run::RunType;
use crate::error::{AppError, AppResult};
use crate::events::{RunEvent, RunSink};
use crate::infrastructure::{db, run_repository};
use crate::application::skill_service;

/// 默认 Profile（启动时 seed；用户后续可在设置里调整——M13 UI）。
const DEFAULT_PROFILE_JSON: &str = r#"{
    "name": "knowledge-analyst",
    "display_name": "知识分析师",
    "model": "",
    "skills": ["knowledge-extraction", "knowledge-answering", "knowledge-correction"],
    "policy": ["read", "propose"],
    "system_prompt": ""
}"#;

/// 启动时幂等 seed 默认 Profile（已存在则不覆盖——用户的修改优先）。
pub fn ensure_default_profiles(conn: &Connection) -> AppResult<()> {
    let profile: Value = serde_json::from_str(DEFAULT_PROFILE_JSON)
        .map_err(|err| AppError::Internal(format!("默认 Profile JSON 损坏：{err}")))?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE name = ?1)",
            rusqlite::params![profile["name"].as_str().unwrap_or_default()],
            |r| r.get(0),
        )
        .map_err(AppError::from)?;
    if exists {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO agent_profiles(name, display_name, model, skills, policy, system_prompt) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            profile["name"].as_str().unwrap_or_default(),
            profile["display_name"].as_str().unwrap_or_default(),
            profile["model"].as_str().unwrap_or_default(),
            profile["skills"].to_string(),
            profile["policy"].to_string(),
            profile["system_prompt"].as_str().unwrap_or_default(),
        ],
    )?;
    Ok(())
}

/// 列出全部 Agent Profile。
pub fn list_profiles(conn: &Connection) -> AppResult<Vec<AgentProfile>> {
    let mut stmt = conn.prepare(
        "SELECT name, display_name, model, skills, policy, system_prompt \
         FROM agent_profiles ORDER BY name",
    )?;
    let rows = stmt.query_map([], map_profile)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 读取单个 Profile。
pub fn get_profile(conn: &Connection, name: &str) -> AppResult<AgentProfile> {
    let mut stmt = conn.prepare(
        "SELECT name, display_name, model, skills, policy, system_prompt \
         FROM agent_profiles WHERE name = ?1",
    )?;
    let mut rows = stmt.query_map(rusqlite::params![name], map_profile)?;
    match rows.next() {
        Some(row) => row.map_err(AppError::from),
        None => Err(AppError::NotFound(format!("Agent Profile `{name}` 不存在"))),
    }
}

fn map_profile(r: &rusqlite::Row<'_>) -> rusqlite::Result<AgentProfile> {
    let skills_json: String = r.get(3)?;
    let policy_json: String = r.get(4)?;
    Ok(AgentProfile {
        name: r.get(0)?,
        display_name: r.get(1)?,
        model: r.get(2)?,
        skills: serde_json::from_str(&skills_json).unwrap_or_default(),
        policy: serde_json::from_str(&policy_json).unwrap_or_default(),
        system_prompt: r.get(5)?,
    })
}

/// 启动一次 Agent Run：登记 + 立即返回 run_id，编排在后台完成。
pub fn start_agent(
    app: tauri::AppHandle,
    db_path: PathBuf,
    profile_name: &str,
    input: Value,
) -> AppResult<String> {
    let profile = {
        let conn = db::open(&db_path)?;
        let profile = get_profile(&conn, profile_name)?;
        // 前置校验：引用的 Skill 必须都已注册（诚实报错，不留半跑状态）。
        for skill in &profile.skills {
            skill_service::resolve(&conn, skill)?;
        }
        profile
    };

    let run_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db::open(&db_path)?;
        let metadata = json!({
            "profile": profile.name,
            "policy": profile.policy,
            "input": input,
        })
        .to_string();
        run_repository::register(
            &conn,
            &run_id,
            RunType::Agent,
            &profile.display_name,
            None,
            &metadata,
        )?;
    }

    let run_events: RunSink = {
        let app = app.clone();
        std::sync::Arc::new(move |event: &RunEvent| {
            let _ = tauri::Emitter::emit(&app, "run-events", event);
        })
    };
    let profile_for_task = profile.clone();
    let run_id_for_task = run_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = run_agent_sync(
            &db_path,
            &run_id_for_task,
            &profile_for_task,
            input,
            &run_events,
        );
    });

    Ok(run_id)
}

/// 同步编排：顺序执行 Profile 拥有的 Skill（每个都是一个子 Skill Run）。
fn run_agent_sync(
    db_path: &PathBuf,
    run_id: &str,
    profile: &AgentProfile,
    input: Value,
    sink: &RunSink,
) -> AppResult<()> {
    let conn = db::open(db_path)?;
    run_repository::set_status(&conn, run_id, ExtractionRunStatus::Running)?;
    sink(&RunEvent::Started {
        run_id: run_id.to_string(),
        run_type: RunType::Agent,
    });

    let mut results = serde_json::Map::new();
    for skill in &profile.skills {
        // 权限闸门（M5）：Skill 需要的最高权限 ≤ Profile 的 policy 上限。
        let gate = skill_service::resolve(&conn, skill).map(|def| {
            let cap = Policy::highest_of(profile.policy.iter().map(String::as_str));
            let required = def
                .permissions
                .iter()
                .filter_map(|p| Policy::from_str(p.as_str()).ok())
                .max_by_key(|p| p.rank_of());
            let allowed = match (cap, required) {
                (Some(cap), Some(required)) => cap.at_least(&required),
                (Some(_), None) => true,
                _ => false,
            };
            (def, allowed)
        });
        match gate {
            Ok((_def, true)) => match skill_service::run_skill_sync(
                db_path,
                skill,
                input.clone(),
                Some(run_id),
                sink,
            ) {
                Ok((skill_run_id, summary)) => {
                    results.insert(skill.clone(), json!({ "runId": skill_run_id, "summary": summary }));
                }
                Err(err) => {
                    results.insert(skill.clone(), json!({ "error": err.to_string() }));
                }
            },
            Ok((_def, false)) => {
                // 超出 Profile 权限上限：诚实跳过，绝不静默执行。
                results.insert(skill.clone(), json!({ "skipped": "policy" }));
            }
            Err(err) => {
                results.insert(skill.clone(), json!({ "skipped": err.to_string() }));
            }
        }
    }

    let results = Value::Object(results);
    run_repository::set_metadata(&conn, run_id, &json!({ "skills": results }).to_string())?;
    run_repository::finish(&conn, run_id, ExtractionRunStatus::Completed, None, None)?;
    sink(&RunEvent::Completed {
        run_id: run_id.to_string(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::skill_service;

    fn setup_db() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        crate::infrastructure::db::initialize(&db_path).unwrap();
        {
            let mut conn = db::open(&db_path).unwrap();
            conn.execute_batch(
                "INSERT INTO agent_profiles(name, display_name, skills, policy) VALUES \
                 ('read-only-analyst', '只读分析师', '[\"knowledge-extraction\"]', '[\"read\"]');",
            )
            .unwrap();
        }
        // tempfile 目录随返回值移动：把路径泄漏为 'static 不必要——
        // 测试进程生命周期内目录有效即可。
        dir.keep();
        db_path
    }

    #[test]
    fn default_profile_is_seeded_idempotently() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        ensure_default_profiles(&conn).unwrap();
        ensure_default_profiles(&conn).unwrap(); // 幂等
        let profiles = list_profiles(&conn).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "knowledge-analyst");
        assert_eq!(profiles[0].skills.len(), 3);
    }

    #[test]
    fn policy_gate_skips_overprivileged_skill() {
        let db_path = setup_db();
        // ensure builtin skills 注册到这个库
        {
            let conn = db::open(&db_path).unwrap();
            skill_service::ensure_builtin_skills(&conn).unwrap();
        }
        // read-only-analyst 只有 read 权限，但 knowledge-extraction 需要 propose：
        // 应诚实跳过（不执行），Agent Run 仍正常完成。
        let sink: RunSink = std::sync::Arc::new(|_| {});
        let profile = {
            let conn = db::open(&db_path).unwrap();
            get_profile(&conn, "read-only-analyst").unwrap()
        };
        {
            let conn = db::open(&db_path).unwrap();
            run_repository::register(
                &conn,
                "agent-1",
                RunType::Agent,
                &profile.display_name,
                None,
                "{}",
            )
            .unwrap();
        }
        run_agent_sync(&db_path, "agent-1", &profile, json!({}), &sink).unwrap();

        let conn = db::open(&db_path).unwrap();
        let run = run_repository::get(&conn, "agent-1").unwrap().unwrap();
        assert_eq!(run.status, ExtractionRunStatus::Completed);
        let metadata: Value = serde_json::from_str(&run.metadata).unwrap();
        assert_eq!(metadata["skills"]["knowledge-extraction"]["skipped"], json!("policy"));
    }
}

#[cfg(test)]
mod policy_gate_tests {
    use super::*;
    use crate::ai::agents::AgentRole;
    use crate::ai::tools::ToolName;
    use crate::domain::policy::Policy;

    /// 结构性保证（行动计划纪律三）：工具白名单里不存在 MUTATE 级工具。
    /// AI / Skill 的任何可达路径最高只能 PROPOSE——改知识永远经人类 Review。
    #[test]
    fn no_tool_requires_mutate() {
        let all = [
            ToolName::SearchKnowledge,
            ToolName::GetKnowledge,
            ToolName::GetEntities,
            ToolName::GetEntity,
            ToolName::GetClaim,
            ToolName::FindRelated,
            ToolName::GetEvidence,
            ToolName::CompareClaims,
            ToolName::DetectConflict,
            ToolName::ProposeEvolution,
            ToolName::RequestReview,
            ToolName::Research,
        ];
        assert!(all
            .iter()
            .all(|t| !t.required_policy().at_least(&Policy::Mutate)));
        // 唯一的"写"路径是往 Review 队列放提案（PROPOSE 级）。
        assert_eq!(ToolName::RequestReview.required_policy(), Policy::Propose);
    }

    /// 角色权限上限：没有任何 Agent 角色能拿到 MUTATE。
    #[test]
    fn no_role_gets_mutate() {
        for role in [
            AgentRole::Auto,
            AgentRole::Personal,
            AgentRole::Knowledge,
            AgentRole::Research,
            AgentRole::Curator,
            AgentRole::Review,
            AgentRole::Extraction,
        ] {
            assert!(!role.policy().at_least(&Policy::Mutate));
        }
    }
}
