-- ============================================================================
-- wiki-ya 迁移 0004（CORE-001 / CORE-005）
--
-- 由 db.rs 的「按版本增量」机制应用：只在 version > current 时执行一次。
-- 因此这里**允许**使用非幂等语句（ALTER TABLE ADD COLUMN）。
-- ============================================================================

-- ---------------------------------------------------------------------------
-- CORE-001：Temporal —— 来源观察时间
-- ---------------------------------------------------------------------------
-- 四个时间字段各自的含义（务必不要混用）：
--   recorded_at : 系统**知道**这条知识的时间（写入即 now，已有）
--   observed_at : 来源材料中**明确记录/观察到**该知识的时间（本次新增）
--   valid_from  : 该知识**实际开始有效**的时间（已有）
--   valid_until : 该知识**实际结束有效**的时间（已有）
--
-- 原则：文本没有明确时间时一律留 NULL，**禁止 AI 猜时间**。
-- 可空、无默认值：NULL 表示"未知"，与"此刻"是两回事。
ALTER TABLE claims ADD COLUMN observed_at TEXT;

-- ---------------------------------------------------------------------------
-- CORE-005：演化事件日志（不可变追加）
-- ---------------------------------------------------------------------------
-- claim_relations 是「当前关系状态」（会被更新），本表是它的**只追加**历史。
-- 每一次 Review 决策（accept / reject / reset）都写入一行，因此：
--   - Timeline 能完整呈现"知识为什么变化"；
--   - Revert（reset）会生成**新的**事件，而不是抹掉历史；
--   - 任何状态迁移都可回放审计。
CREATE TABLE IF NOT EXISTS claim_relation_events (
  id TEXT PRIMARY KEY,
  relation_id TEXT NOT NULL REFERENCES claim_relations(id) ON DELETE CASCADE,
  -- 决策后的关系类型（人工可在审核时改判）。
  relationship TEXT NOT NULL CHECK(relationship IN
    ('duplicate','coexists','supplements','supersedes','contradicts','unclear')),
  -- 决策后的关系状态；`candidate` 即「回到待审 / 撤销」。
  status TEXT NOT NULL CHECK(status IN ('candidate','accepted','rejected')),
  -- 受影响方（supersedes 的 target）与它前后的状态，供精确回滚与审计。
  target_claim_id TEXT REFERENCES claims(id) ON DELETE CASCADE,
  target_status_before TEXT,
  target_status_after TEXT,
  note TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_claim_rel_events_relation
  ON claim_relation_events(relation_id, created_at);
