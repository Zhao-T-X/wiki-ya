//! Agent Profile CRUD（M12：自定义 Agent）。
//!
//! 校验边界：
//! - `name`：小写 slug，可与内置默认重名（覆盖语义走 update）；
//! - `skills`：必须全部已在 `skills` 表注册（内置或自定义）；
//! - `policy`：上限 **propose**——用户创建不了 MUTATE 级 Agent，
//!   「改知识永远经人类 Review」不因自定义而失效。
//!
//! 默认 Profile（`knowledge-analyst`）由启动 seed 保证存在：
//! 删除后重启会恢复（出厂预置语义，用户的修改不被覆盖）。

use rusqlite::Connection;

use crate::application::agent_profile_service::{ensure_default_profiles, get_profile, list_profiles};
use crate::domain::agent_profile::AgentProfile;
use crate::domain::policy::Policy;
use crate::error::{AppError, AppResult};

fn validate_name(name: &str) -> AppResult<()> {
    let ok = name.len() >= 2
        && name.len() <= 40
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(AppError::Domain(
            "Agent 名必须为 2-40 个字符的小写字母/数字/连字符，且以字母开头".into(),
        ))
    }
}

fn validate_policy(policy: &[String]) -> AppResult<()> {
    let cap = Policy::highest_of(policy.iter().map(String::as_str)).ok_or_else(|| {
        AppError::Domain("policy 不能为空（至少包含 read）".into())
    })?;
    if cap.at_least(&Policy::Mutate) {
        // 结构边界（M5 测试固化的同一纪律）：自定义 Agent 拿不到 MUTATE。
        return Err(AppError::Domain(
            "Agent 的 policy 上限是 propose——改知识永远经人类 Review".into(),
        ));
    }
    Ok(())
}

fn validate_skills(conn: &Connection, skills: &[String]) -> AppResult<()> {
    if skills.is_empty() {
        return Err(AppError::Domain("Agent 至少要拥有一个 Skill".into()));
    }
    for skill in skills {
        let registered: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM skills WHERE name = ?1)",
                rusqlite::params![skill],
                |r| r.get(0),
            )
            .map_err(AppError::from)?;
        if !registered {
            return Err(AppError::Domain(format!("Skill `{skill}` 未注册")));
        }
    }
    Ok(())
}

/// 创建 Agent Profile。
pub fn create_profile(conn: &mut Connection, profile: &AgentProfile) -> AppResult<()> {
    validate_name(&profile.name)?;
    validate_policy(&profile.policy)?;
    validate_skills(conn, &profile.skills)?;
    if profile.name == "knowledge-analyst" {
        return Err(AppError::Domain(
            "`knowledge-analyst` 是默认 Profile，如需调整请直接编辑".into(),
        ));
    }
    conn.execute(
        "INSERT INTO agent_profiles(name, display_name, model, skills, policy, system_prompt) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            profile.name,
            profile.display_name,
            profile.model,
            serde_json::to_string(&profile.skills).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&profile.policy).unwrap_or_else(|_| "[]".into()),
            profile.system_prompt
        ],
    )?;
    Ok(())
}

/// 更新 Agent Profile（整体替换字段）。
pub fn update_profile(conn: &mut Connection, profile: &AgentProfile) -> AppResult<()> {
    validate_policy(&profile.policy)?;
    validate_skills(conn, &profile.skills)?;
    // 更新默认 Profile 时不受 name 规则限制（保持稳定标识）。
    let changed = conn.execute(
        "UPDATE agent_profiles SET display_name = ?2, model = ?3, skills = ?4, policy = ?5, \
         system_prompt = ?6, updated_at = datetime('now') WHERE name = ?1",
        rusqlite::params![
            profile.name,
            profile.display_name,
            profile.model,
            serde_json::to_string(&profile.skills).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&profile.policy).unwrap_or_else(|_| "[]".into()),
            profile.system_prompt
        ],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!(
            "Agent Profile `{}` 不存在",
            profile.name
        )));
    }
    Ok(())
}

/// 删除 Agent Profile。默认 Profile 删除后会在下次启动恢复（出厂预置）。
pub fn delete_profile(conn: &Connection, name: &str) -> AppResult<()> {
    if name == "knowledge-analyst" {
        return Err(AppError::Domain(
            "`knowledge-analyst` 是默认 Profile，不可删除（可编辑其 skills/policy）".into(),
        ));
    }
    let changed = conn.execute(
        "DELETE FROM agent_profiles WHERE name = ?1",
        rusqlite::params![name],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("Agent Profile `{name}` 不存在")));
    }
    Ok(())
}

/// 便捷聚合：列表（给前端一次性拿到全部 Profile）。
pub fn list_or_seed(conn: &Connection) -> AppResult<Vec<AgentProfile>> {
    if list_profiles(conn)?.is_empty() {
        ensure_default_profiles(conn)?;
    }
    list_profiles(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;

    fn profile(name: &str, policy: &[&str], skills: &[&str]) -> AgentProfile {
        AgentProfile {
            name: name.into(),
            display_name: name.into(),
            model: String::new(),
            skills: skills.iter().map(|s| s.to_string()).collect(),
            policy: policy.iter().map(|s| s.to_string()).collect(),
            system_prompt: String::new(),
        }
    }

    #[test]
    fn crud_rejects_mutate_and_unregistered_skills() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::apply_migrations(&mut conn).unwrap();
        crate::application::skill_service::ensure_builtin_skills(&conn).unwrap();

        // 正常创建。
        create_profile(
            &mut conn,
            &profile("tech-researcher", &["read", "propose"], &["knowledge-extraction"]),
        )
        .unwrap();

        // MUTATE 上限拒绝（结构边界：自定义 Agent 拿不到写权限）。
        assert!(create_profile(
            &mut conn,
            &profile("god-agent", &["read", "propose", "mutate"], &["knowledge-answering"]),
        )
        .is_err());

        // 未注册 Skill 拒绝。
        assert!(create_profile(&mut conn, &profile("x", &["read"], &["no-such"])).is_err());
        // 空 Skill 列表拒绝。
        assert!(create_profile(&mut conn, &profile("x2", &["read"], &[])).is_err());
        // 默认名不可占用。
        assert!(create_profile(&mut conn, &profile("knowledge-analyst", &["read"], &["knowledge-answering"])).is_err());

        // 更新（缩权：只保留 read）。
        update_profile(
            &mut conn,
            &profile("tech-researcher", &["read"], &["knowledge-answering"]),
        )
        .unwrap();

        // 默认 Profile 不可删除；自定义可删。
        ensure_default_profiles(&conn).unwrap();
        assert!(delete_profile(&conn, "knowledge-analyst").is_err());
        delete_profile(&conn, "tech-researcher").unwrap();
        assert!(get_profile(&conn, "tech-researcher").is_err());
    }
}
