-- 0007 Skill 标准化（M3）：skills / skill_versions 表。
--
-- SKILL.md 是内置 Skill 的单一事实来源（仓库 src-tauri/skills/**），
-- 启动时由 `skill_service::ensure_builtin_skills` 幂等 seed 进这里。
-- 版本化是硬要求：Trace 必须能回答"这条知识当时是哪个版本的 Skill 产生的"。

CREATE TABLE IF NOT EXISTS skills (
    name            TEXT PRIMARY KEY,
    description     TEXT NOT NULL DEFAULT '',
    current_version INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS skill_versions (
    skill_name   TEXT NOT NULL REFERENCES skills(name),
    version      INTEGER NOT NULL,
    instructions TEXT NOT NULL,
    input_hint   TEXT NOT NULL DEFAULT '',
    output_hint  TEXT NOT NULL DEFAULT '',
    tools        TEXT NOT NULL DEFAULT '[]',
    permissions  TEXT NOT NULL DEFAULT '[]',
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (skill_name, version)
);
