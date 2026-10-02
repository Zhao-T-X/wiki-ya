-- 0008 Agent Profile（M4：Agent 配置化）。
--
-- Agent 不再定义「我会干什么」（硬编码角色提示词），而是声明
-- 「我拥有哪些 Skill、被允许做什么」：
--   profile.skills ⊆ 已注册 Skill；profile.policy 是该 Agent 的权限上限，
--   Skill 的 permissions 必须是 policy 的子集才会被执行（M5 闸门雏形）。

CREATE TABLE IF NOT EXISTS agent_profiles (
    name          TEXT PRIMARY KEY,
    display_name  TEXT NOT NULL DEFAULT '',
    model         TEXT NOT NULL DEFAULT '', -- 空 = 跟随全局 AI 设置
    skills        TEXT NOT NULL DEFAULT '[]',
    policy        TEXT NOT NULL DEFAULT '[]',
    system_prompt TEXT NOT NULL DEFAULT '', -- 预留给 ReAct 型 Agent（M4+）
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);
