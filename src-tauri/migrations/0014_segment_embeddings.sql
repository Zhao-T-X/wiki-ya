-- 0014 分段向量化（PERF-07 / 方案 A）。
--
-- 起因：bge-small-zh-v1.5 上下文 512 token，而本库实测 73 个 chunk 中 41 个
-- （56%）超限、最长约 831 token。直接送入会�� tokenizer **静默截断**——
-- 尾部内容永远搜不到（这不是"稍差一点"，是检索不到）。
--
-- 方案 A：超限 chunk 滑动切段，**每段各出一个向量**；检索按 chunk_id 去重、
-- 取分数最高的那一段。短 chunk 仍写单段（part=0），行为与改造前完全一致。
--
-- 三点必须记清：
-- 1. 主键改为 (chunk_id, model, part)：顺带支持**多模型共存**（此前 PK 只有
--    chunk_id，切模型必然全库重算）。
-- 2. segment_text 存**实际送去嵌入的那段原文**。检索返回的证据只能是它——
--    若向量只比较了前半段却展示整块，就是虚报证据范围。
-- 3. 本库当前 0 向量，重建无成本；将来迁移旧数据按 part=0 落位。

DROP INDEX IF EXISTS idx_chunk_embeddings_model;
ALTER TABLE chunk_embeddings RENAME TO chunk_embeddings_legacy;

CREATE TABLE chunk_embeddings (
  chunk_id    TEXT NOT NULL,
  model       TEXT NOT NULL,
  part        INTEGER NOT NULL DEFAULT 0,  -- 第几段；0 = 未切分
  embedding   BLOB,
  dimensions  INTEGER,
  segment_text TEXT,                        -- 实际参与嵌入的文本（诚实性关键）
  char_start  INTEGER,                      -- 该段在 chunk 内的字符区间
  char_end    INTEGER,
  created_at  TEXT NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (chunk_id, model, part)
);

INSERT INTO chunk_embeddings(chunk_id, model, part, embedding, dimensions, segment_text, char_start, char_end, created_at)
  SELECT chunk_id, model, 0, embedding, dimensions, NULL, NULL, NULL, created_at
  FROM chunk_embeddings_legacy;

DROP TABLE chunk_embeddings_legacy;

CREATE INDEX IF NOT EXISTS idx_chunk_embeddings_model ON chunk_embeddings(model);
