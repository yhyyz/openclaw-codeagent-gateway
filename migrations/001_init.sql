CREATE TABLE IF NOT EXISTS jobs (
    id               TEXT PRIMARY KEY,
    tenant_id        TEXT NOT NULL,
    agent            TEXT NOT NULL,
    session_id       TEXT NOT NULL,
    prompt           TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'pending',
    result           TEXT NOT NULL DEFAULT '',
    error            TEXT NOT NULL DEFAULT '',
    tools            TEXT NOT NULL DEFAULT '[]',
    created_at       INTEGER NOT NULL,
    completed_at     INTEGER NOT NULL DEFAULT 0,
    callback_url     TEXT NOT NULL DEFAULT '',
    callback_routing TEXT NOT NULL DEFAULT '{}',
    webhook_sent     INTEGER NOT NULL DEFAULT 0,
    progress         TEXT NOT NULL DEFAULT '',
    progress_notify  INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_tenant ON jobs(tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_jobs_webhook ON jobs(status, webhook_sent)
    WHERE status IN ('completed', 'failed', 'interrupted') AND webhook_sent = 0;
