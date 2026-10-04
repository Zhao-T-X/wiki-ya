-- 0013 按真实查询补索引（PERF-04）。
--
-- 纪律：**只为「现在或本 PR 立刻要加的查询」建索引**。索引不是免费的——
-- 每个都会放大写入成本与库体积。以下三条都能对上具体查询：
--
-- 1) idx_candidates_accepted_claim
--    服务 trace_service::get_claim_trace 的
--      WHERE accepted_claim_id = ?1 ORDER BY created_at DESC LIMIT 1
--    该列此前**完全没有索引**——即「Claim 溯源」用得越久越慢的根因。
--
-- 2) idx_candidates_run_cursor
--    服务候选列表的游标分页
--      WHERE run_id = ?1 AND (created_at, id) > (?, ?) ORDER BY created_at, id LIMIT ?
--    它同时是既有 idx_candidates_run 的**严格前缀超集**（run_id 等值查询照样命中），
--    故一并 DROP 掉旧索引，避免同一维度写两遍。
--
-- 3) idx_evidence_claim_level_created
--    服务 claim_repository::CLAIM_SELECT 里挑「主证据」的窗口函数
--      ROW_NUMBER() OVER (PARTITION BY claim_id ORDER BY evidence_level, created_at)
--    有了它，SQLite 可直接沿索引序走，**免掉全表 evidence 的临时排序**。
--    它同样是既有 idx_evidence_claim(claim_id) 的前缀超集，故一并 DROP。

CREATE INDEX IF NOT EXISTS idx_candidates_accepted_claim
  ON candidates(accepted_claim_id);

CREATE INDEX IF NOT EXISTS idx_candidates_run_cursor
  ON candidates(run_id, created_at, id);

CREATE INDEX IF NOT EXISTS idx_evidence_claim_level_created
  ON evidence(claim_id, evidence_level, created_at);

DROP INDEX IF EXISTS idx_candidates_run;
DROP INDEX IF EXISTS idx_evidence_claim;
