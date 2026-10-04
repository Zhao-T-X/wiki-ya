-- 0011 候选 grounding 支持度（PR-03）。
--
-- 每条抽取候选必须能回答"它锚定在哪段原文"——这是知识可追溯的硬门槛：
--   directly   = source_quote 逐字落在 source_chunk 原文（本地校验，无需 LLM）
--   partially  = 有切片锚点但无逐字引用（LLM 改写 / 缺 quote）
--   unsupported = 无切片锚点，无法验证
-- 只有 directly 进入正常 Review；partially / unsupported 需人工补证据后才可接受。

ALTER TABLE candidates ADD COLUMN support_level TEXT NOT NULL DEFAULT 'unsupported'
  CHECK (support_level IN ('directly', 'partially', 'unsupported'));

CREATE INDEX IF NOT EXISTS idx_candidates_support ON candidates(support_level, status);
