-- 0006 统一 Run 注册表（M1：Run → Skill → Tool → Result → Trace）。
--
-- 目标不是替掉 extraction_runs / agent_runs 的明细，而是给所有类型的
-- Run 一个**统一的登记处**：同一套 id / status 语义，Trace 才能用一条
-- SQL JOIN 把 Source → Run → Candidate → Evidence → Review 串起来。
--
-- 明细留在各自的表里（extraction_runs 的 stage/进度、agent_runs 的
-- step 明细），`runs` 只承载统一身份与生命周期。

CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    parent_run_id TEXT,
    run_type      TEXT NOT NULL,            -- extraction / agent / ask / skill / review
    actor         TEXT NOT NULL DEFAULT '', -- 角色 / Skill 名，如 "KnowledgeAgent"
    status        TEXT NOT NULL,            -- queued / running / completed / failed / cancelled / interrupted
    stage         TEXT NOT NULL DEFAULT '', -- 与类型相关的阶段字面量（可空）
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    error_code    TEXT,
    error_message TEXT,
    metadata      TEXT NOT NULL DEFAULT '{}' -- JSON：类型相关的附加信息
);

CREATE INDEX IF NOT EXISTS idx_runs_started ON runs(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_runs_type ON runs(run_type);

-- run_id 贯穿：Agent/Skill 产生的提案可回溯到"产生它的那次 Run"。
ALTER TABLE reviews ADD COLUMN run_id TEXT;
CREATE INDEX IF NOT EXISTS idx_reviews_run ON reviews(run_id);
