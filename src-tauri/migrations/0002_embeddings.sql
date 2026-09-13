-- ============================================================================
-- wiki-ya 嵌入表（0002，Phase 6 语义检索）
--
-- 与 0001 同机执行：纯 DDL，不含 PRAGMA，带 IF NOT EXISTS 保证幂等。
--
-- 嵌入是派生数据：chunk 删除即失效，重算即可恢复，因此**不**加外键
-- 级联——避免删除 chunk 时连向量一起清掉导致语义检索静默失效；
-- embedding 由 semantic_search 在查询时按需重算并覆盖写入（PRIMARY KEY 覆盖）。
-- ============================================================================

CREATE TABLE IF NOT EXISTS chunk_embeddings (
  chunk_id  TEXT PRIMARY KEY,
  embedding BLOB,
  dimensions INTEGER,
  model     TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 按模型过滤（多模型共存时可缩小扫描范围）。
CREATE INDEX IF NOT EXISTS idx_chunk_embeddings_model ON chunk_embeddings(model);
