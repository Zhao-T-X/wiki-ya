-- 0005 Extraction Run（EXTRACTION-001：异步抽取后台任务）
--
-- Run 是"一次文档抽取尝试"的持久化记录，让任务可见、可追踪、可恢复：
-- 用户关掉页面甚至关掉应用，状态都不该蒸发。

CREATE TABLE IF NOT EXISTS extraction_runs (
    id               TEXT PRIMARY KEY,
    document_id      TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    status           TEXT NOT NULL
                       CHECK (status IN ('queued','running','completed','failed','cancelled','interrupted')),
    stage            TEXT NOT NULL
                       CHECK (stage IN ('preparing','chunking','extracting','validating','comparing','finalizing')),
    total_chunks     INTEGER NOT NULL DEFAULT 0,
    processed_chunks INTEGER NOT NULL DEFAULT 0,
    candidates_found INTEGER NOT NULL DEFAULT 0,
    changes_found    INTEGER NOT NULL DEFAULT 0,
    -- 完成后的结果（序列化的 ExtractionReport），用于历史回看与重新展示。
    result_json      TEXT,
    started_at       TEXT NOT NULL,
    finished_at      TEXT,
    error_code       TEXT,
    error_message    TEXT
);

CREATE INDEX IF NOT EXISTS idx_extraction_runs_document ON extraction_runs(document_id);
CREATE INDEX IF NOT EXISTS idx_extraction_runs_started ON extraction_runs(started_at);
