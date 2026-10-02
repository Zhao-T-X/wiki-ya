-- 0009 候选知识持久化（M6：Candidate 一产生就持久化）。
--
-- 纪律二：不再让「结果是否确认」决定数据是否存在——
--   发现 → 留痕（status = pending）
--   确认 → 生效（accepted + accepted_claim_id）
-- 候选与产生它的 Run、来源文档、接受后的 Claim 全链可追溯。

CREATE TABLE IF NOT EXISTS candidates (
    id                 TEXT PRIMARY KEY,
    run_id             TEXT NOT NULL REFERENCES extraction_runs(id),
    document_id        TEXT NOT NULL REFERENCES documents(id),
    subject            TEXT NOT NULL,
    predicate          TEXT NOT NULL,
    object_text        TEXT,
    content            TEXT,
    claim_type         TEXT,
    polarity           TEXT,
    modality           TEXT,
    confidence         REAL,
    source_chunk_index INTEGER,
    source_quote       TEXT,
    sentence           TEXT,
    status             TEXT NOT NULL DEFAULT 'pending'
                       CHECK(status IN ('pending', 'accepted', 'rejected')),
    accepted_claim_id  TEXT REFERENCES claims(id),
    reject_reason      TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_candidates_run ON candidates(run_id);
CREATE INDEX IF NOT EXISTS idx_candidates_status ON candidates(status, created_at DESC);
