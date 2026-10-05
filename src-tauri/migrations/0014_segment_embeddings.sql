-- 0014 分段向量化（PERF-07 / 方案 A）。
--
-- 起因：bge-small-zh-v1.5 上下文 512 token，而本库实测 73 个 chunk 中 41 个
-- （56%）超限、最长约 831 token。直接送入会被 tokenizer **静默截断**——
-- 尾部内容永远搜不到（这不是"稍差一点"，是检索不到）。
--
-- 方案 A：超限 chunk 滑动切段，**每段各出一个向量**；检索按 chunk_id 去重、
-- 取分数最高的那一段。短 chunk 仍写单段（part=0），行为与改造前完全等价。
--
-- 三点必须记清：
-- 1. 主键改为 (chunk_id, model, part)：顺带支持**多模型共存**（此前 PK 只有
--    chunk_id，切模型必然全库重算）。
-- 2. segment_text 存**实际送去嵌入的那段原文**。检索返回的证据只能是它——
--    若向量只比较了前半段却展示整块，就是虚报证据范围。
-- 3. **不搬运旧向量**（2026-10-05 修正）。此前这里有一条
--    INSERT ... SELECT ... FROM chunk_embeddings_legacy，按新列名
--    （embedding / dimensions）从旧表搬运，在真实库上直接让**应用无法启动**：
--      no such column: embedding
--    原因是历史库上 chunk_embeddings 的真实结构是**另一套列名**
--    （dims / embedding_blob）——0002 用的是 CREATE TABLE IF NOT EXISTS，
--    它对已存在的表不生效，所以从旧 schema 升上来的库永远停在旧结构上。
--    而静态 SQL 没有两全的写法：SQLite 在 prepare 阶段就解析列名，
--    CASE WHEN 的不可达分支同样报 no such column，无法「按列是否存在」取舍。
--    向量是**派生数据**（见 0001：「chunk 删除即失效，重算即可恢复」），
--    缺失的向量会在下次语义检索时由 chunks_missing_embedding 自动重算。
--    因此宁可空表重建，也不能让启动被一条派生数据的迁移卡死。
--
-- 关于修改已发布迁移：列名不匹配的库停在 version 13，改动后它们才能继续升级；
-- 已成功执行过 0014 的库早已记录 version 14，不受本次改动影响。

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

DROP TABLE chunk_embeddings_legacy;

CREATE INDEX IF NOT EXISTS idx_chunk_embeddings_model ON chunk_embeddings(model);
