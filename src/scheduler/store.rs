//! Persistent job storage (SQLite).

use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::Error;
use crate::scheduler::job::{Job, JobStatus};

#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub session_id: String,
    pub session_name: String,
    pub tenant_id: String,
    pub agent: String,
    pub acp_session_id: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub prompt_count: i64,
}

pub struct JobStore {
    conn: Mutex<Connection>,
}

impl JobStore {
    pub fn open(path: &str) -> Result<Self, Error> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch(include_str!("../../migrations/001_init.sql"))?;
        let _ = conn.execute_batch("ALTER TABLE jobs ADD COLUMN progress TEXT NOT NULL DEFAULT ''");
        let _ = conn.execute_batch("ALTER TABLE jobs ADD COLUMN progress_notify INTEGER NOT NULL DEFAULT 1");
        let _ = conn.execute_batch("ALTER TABLE jobs ADD COLUMN session_name TEXT NOT NULL DEFAULT ''");
        // Ensure sessions table exists for existing DBs (new DBs get it from 001_init.sql)
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                session_id     TEXT PRIMARY KEY,
                session_name   TEXT NOT NULL,
                tenant_id      TEXT NOT NULL,
                agent          TEXT NOT NULL,
                acp_session_id TEXT NOT NULL DEFAULT '',
                created_at     INTEGER NOT NULL,
                last_used_at   INTEGER NOT NULL,
                prompt_count   INTEGER NOT NULL DEFAULT 0
            )"
        );
        let _ = conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS idx_session_name ON sessions(tenant_id, agent, session_name)");
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_session_recent ON sessions(tenant_id, agent, last_used_at DESC)");
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", "5000")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, job: &Job) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let tools_json = serde_json::to_string(&job.tools).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO jobs (id, tenant_id, agent, session_id, prompt, status, result, error, tools, created_at, completed_at, callback_url, callback_routing, webhook_sent, progress, progress_notify, session_name) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
            params![
                job.id,
                job.tenant_id,
                job.agent,
                job.session_id,
                job.prompt,
                job.status.as_str(),
                job.result,
                job.error,
                tools_json,
                job.created_at,
                job.completed_at,
                job.callback_url,
                job.callback_routing,
                job.webhook_sent as i32,
                job.progress,
                job.progress_notify as i32,
                job.session_name,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent, session_id, prompt, status, result, error, tools, created_at, completed_at, callback_url, callback_routing, webhook_sent, progress, progress_notify, session_name FROM jobs WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_job(row)?)),
            None => Ok(None),
        }
    }

    pub fn update_status(
        &self,
        id: &str,
        status: &JobStatus,
        result: &str,
        error: &str,
    ) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let completed_at = match status {
            JobStatus::Completed | JobStatus::Failed | JobStatus::Interrupted => now,
            _ => 0,
        };
        conn.execute(
            "UPDATE jobs SET status = ?1, result = ?2, error = ?3, completed_at = ?4 WHERE id = ?5",
            params![status.as_str(), result, error, completed_at, id],
        )?;
        Ok(())
    }

    pub fn mark_completed(&self, id: &str, result: &str, tools: &[String]) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        let tools_json = serde_json::to_string(tools).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "UPDATE jobs SET status = 'completed', result = ?1, tools = ?2, completed_at = ?3 WHERE id = ?4",
            params![result, tools_json, now, id],
        )?;
        Ok(())
    }

    pub fn mark_failed(&self, id: &str, error: &str) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE jobs SET status = 'failed', error = ?1, completed_at = ?2 WHERE id = ?3",
            params![error, now, id],
        )?;
        Ok(())
    }

    pub fn mark_webhook_sent(&self, id: &str) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET webhook_sent = 1 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn update_progress(&self, id: &str, progress: &str) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET progress = ?1 WHERE id = ?2",
            params![progress, id],
        )?;
        Ok(())
    }

    pub fn list_by_tenant(&self, tenant_id: &str, limit: usize) -> Result<Vec<Job>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent, session_id, prompt, status, result, error, tools, created_at, completed_at, callback_url, callback_routing, webhook_sent, progress, progress_notify, session_name FROM jobs WHERE tenant_id = ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![tenant_id, limit as i64], |row| {
            row_to_job(row).map_err(|e| match e {
                Error::Db(db_err) => db_err,
                _ => rusqlite::Error::InvalidQuery,
            })
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn list_pending_webhooks(&self) -> Result<Vec<Job>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent, session_id, prompt, status, result, error, tools, created_at, completed_at, callback_url, callback_routing, webhook_sent, progress, progress_notify, session_name FROM jobs WHERE status IN ('completed', 'failed', 'interrupted') AND webhook_sent = 0 AND callback_url != ''",
        )?;
        let rows = stmt.query_map([], |row| {
            row_to_job(row).map_err(|e| match e {
                Error::Db(db_err) => db_err,
                _ => rusqlite::Error::InvalidQuery,
            })
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn recover_stale(&self) -> Result<Vec<Job>, Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE jobs SET status = 'interrupted', error = 'recovered after restart', completed_at = ?1 WHERE status IN ('pending', 'running')",
            params![now],
        )?;
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent, session_id, prompt, status, result, error, tools, created_at, completed_at, callback_url, callback_routing, webhook_sent, progress, progress_notify, session_name FROM jobs WHERE status = 'interrupted' AND error = 'recovered after restart'",
        )?;
        let rows = stmt.query_map([], |row| {
            row_to_job(row).map_err(|e| match e {
                Error::Db(db_err) => db_err,
                _ => rusqlite::Error::InvalidQuery,
            })
        })?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn mark_stuck_jobs(&self, timeout_secs: i64) -> Result<usize, Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - timeout_secs;
        let count = conn.execute(
            "UPDATE jobs SET status = 'failed', error = 'timeout: job stuck', completed_at = ?1 WHERE status = 'running' AND created_at < ?2",
            rusqlite::params![chrono::Utc::now().timestamp(), cutoff],
        )?;
        Ok(count)
    }

    pub fn cleanup(&self, retention_secs: i64) -> Result<usize, Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff = chrono::Utc::now().timestamp() - retention_secs;
        let deleted = conn.execute(
            "DELETE FROM jobs WHERE status IN ('completed', 'failed', 'interrupted') AND completed_at > 0 AND completed_at < ?1",
            params![cutoff],
        )?;
        Ok(deleted)
    }

    pub fn insert_session(
        &self,
        session_id: &str,
        session_name: &str,
        tenant_id: &str,
        agent: &str,
        acp_session_id: &str,
    ) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO sessions (session_id, session_name, tenant_id, agent, acp_session_id, created_at, last_used_at, prompt_count) VALUES (?1,?2,?3,?4,?5,?6,?7,0)",
            params![session_id, session_name, tenant_id, agent, acp_session_id, now, now],
        )?;
        Ok(())
    }

    pub fn update_session_acp_id(
        &self,
        session_id: &str,
        acp_session_id: &str,
    ) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET acp_session_id = ?1 WHERE session_id = ?2",
            params![acp_session_id, session_id],
        )?;
        Ok(())
    }

    pub fn touch_session(&self, session_id: &str) -> Result<(), Error> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "UPDATE sessions SET last_used_at = ?1, prompt_count = prompt_count + 1 WHERE session_id = ?2",
            params![now, session_id],
        )?;
        Ok(())
    }

    pub fn get_session_by_name(
        &self,
        tenant_id: &str,
        agent: &str,
        session_name: &str,
    ) -> Result<Option<SessionRecord>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, session_name, tenant_id, agent, acp_session_id, created_at, last_used_at, prompt_count FROM sessions WHERE tenant_id = ?1 AND agent = ?2 AND session_name = ?3",
        )?;
        let mut rows = stmt.query(params![tenant_id, agent, session_name])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_session(row)?)),
            None => Ok(None),
        }
    }

    pub fn get_latest_session(
        &self,
        tenant_id: &str,
        agent: &str,
    ) -> Result<Option<SessionRecord>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, session_name, tenant_id, agent, acp_session_id, created_at, last_used_at, prompt_count FROM sessions WHERE tenant_id = ?1 AND agent = ?2 ORDER BY last_used_at DESC LIMIT 1",
        )?;
        let mut rows = stmt.query(params![tenant_id, agent])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_session(row)?)),
            None => Ok(None),
        }
    }

    pub fn list_sessions(
        &self,
        tenant_id: &str,
        agent: &str,
        limit: usize,
    ) -> Result<Vec<SessionRecord>, Error> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT session_id, session_name, tenant_id, agent, acp_session_id, created_at, last_used_at, prompt_count FROM sessions WHERE tenant_id = ?1 AND agent = ?2 ORDER BY last_used_at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![tenant_id, agent, limit as i64], |row| {
            row_to_session(row).map_err(|e| match e {
                Error::Db(db_err) => db_err,
                _ => rusqlite::Error::InvalidQuery,
            })
        })?;
        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row?);
        }
        Ok(sessions)
    }
}

fn row_to_job(row: &rusqlite::Row) -> Result<Job, Error> {
    let status_str: String = row.get(5)?;
    let status = match status_str.as_str() {
        "pending" => JobStatus::Pending,
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "interrupted" => JobStatus::Interrupted,
        _ => JobStatus::Failed,
    };
    let tools_json: String = row.get(8)?;
    let tools: Vec<String> =
        serde_json::from_str(&tools_json).unwrap_or_default();
    let webhook_sent_int: i32 = row.get(13)?;
    let progress: String = row.get(14)?;
    let progress_notify_int: i32 = row.get(15)?;
    let session_name: String = row.get(16)?;

    Ok(Job {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        agent: row.get(2)?,
        session_id: row.get(3)?,
        prompt: row.get(4)?,
        status,
        result: row.get(6)?,
        error: row.get(7)?,
        tools,
        created_at: row.get(9)?,
        completed_at: row.get(10)?,
        callback_url: row.get(11)?,
        callback_routing: row.get(12)?,
        webhook_sent: webhook_sent_int != 0,
        progress,
        progress_notify: progress_notify_int != 0,
        session_name,
    })
}

fn row_to_session(row: &rusqlite::Row) -> Result<SessionRecord, Error> {
    Ok(SessionRecord {
        session_id: row.get(0)?,
        session_name: row.get(1)?,
        tenant_id: row.get(2)?,
        agent: row.get(3)?,
        acp_session_id: row.get(4)?,
        created_at: row.get(5)?,
        last_used_at: row.get(6)?,
        prompt_count: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_mem() -> JobStore {
        JobStore::open(":memory:").unwrap()
    }

    fn make_job(tenant: &str, agent: &str) -> Job {
        Job::new(tenant, agent, "sess-1", "do something")
    }

    #[test]
    fn insert_and_get_round_trip() {
        let store = open_mem();
        let job = make_job("t1", "kiro");
        store.insert(&job).unwrap();
        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.id, job.id);
        assert_eq!(fetched.tenant_id, "t1");
        assert_eq!(fetched.agent, "kiro");
        assert_eq!(fetched.prompt, "do something");
        assert_eq!(fetched.status, JobStatus::Pending);
        assert!(!fetched.webhook_sent);
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let store = open_mem();
        assert!(store.get("nonexistent").unwrap().is_none());
    }

    #[test]
    fn update_status_sets_fields() {
        let store = open_mem();
        let job = make_job("t1", "kiro");
        store.insert(&job).unwrap();

        store
            .update_status(&job.id, &JobStatus::Running, "", "")
            .unwrap();
        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Running);
        assert_eq!(fetched.completed_at, 0);

        store
            .update_status(&job.id, &JobStatus::Completed, "done", "")
            .unwrap();
        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Completed);
        assert_eq!(fetched.result, "done");
        assert!(fetched.completed_at > 0);
    }

    #[test]
    fn mark_completed_with_tools() {
        let store = open_mem();
        let job = make_job("t1", "agent");
        store.insert(&job).unwrap();

        let tools = vec!["read_file".to_string(), "write_file".to_string()];
        store.mark_completed(&job.id, "all done", &tools).unwrap();

        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Completed);
        assert_eq!(fetched.result, "all done");
        assert_eq!(fetched.tools, vec!["read_file", "write_file"]);
        assert!(fetched.completed_at > 0);
    }

    #[test]
    fn mark_failed_sets_error() {
        let store = open_mem();
        let job = make_job("t1", "agent");
        store.insert(&job).unwrap();

        store.mark_failed(&job.id, "OOM killed").unwrap();

        let fetched = store.get(&job.id).unwrap().unwrap();
        assert_eq!(fetched.status, JobStatus::Failed);
        assert_eq!(fetched.error, "OOM killed");
        assert!(fetched.completed_at > 0);
    }

    #[test]
    fn mark_webhook_sent_flips_flag() {
        let store = open_mem();
        let mut job = make_job("t1", "agent");
        job.callback_url = "https://example.com/hook".into();
        store.insert(&job).unwrap();

        assert!(!store.get(&job.id).unwrap().unwrap().webhook_sent);

        store.mark_webhook_sent(&job.id).unwrap();
        assert!(store.get(&job.id).unwrap().unwrap().webhook_sent);
    }

    #[test]
    fn list_by_tenant_only_own_jobs() {
        let store = open_mem();
        let j1 = make_job("t1", "kiro");
        let j2 = make_job("t2", "kiro");
        let j3 = make_job("t1", "codex");
        store.insert(&j1).unwrap();
        store.insert(&j2).unwrap();
        store.insert(&j3).unwrap();

        let t1_jobs = store.list_by_tenant("t1", 100).unwrap();
        assert_eq!(t1_jobs.len(), 2);
        assert!(t1_jobs.iter().all(|j| j.tenant_id == "t1"));

        let t2_jobs = store.list_by_tenant("t2", 100).unwrap();
        assert_eq!(t2_jobs.len(), 1);
        assert_eq!(t2_jobs[0].tenant_id, "t2");
    }

    #[test]
    fn list_by_tenant_respects_limit() {
        let store = open_mem();
        for _ in 0..5 {
            store.insert(&make_job("t1", "a")).unwrap();
        }
        let jobs = store.list_by_tenant("t1", 3).unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn list_pending_webhooks() {
        let store = open_mem();
        let mut j1 = make_job("t1", "a");
        j1.callback_url = "https://hook.example.com".into();
        store.insert(&j1).unwrap();
        store.mark_completed(&j1.id, "ok", &[]).unwrap();

        let mut j2 = make_job("t1", "b");
        j2.callback_url = "https://hook.example.com".into();
        store.insert(&j2).unwrap();
        // j2 still pending — should NOT appear

        let pending = store.list_pending_webhooks().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, j1.id);
    }

    #[test]
    fn pending_webhooks_excludes_already_sent() {
        let store = open_mem();
        let mut j1 = make_job("t1", "a");
        j1.callback_url = "https://hook.example.com".into();
        store.insert(&j1).unwrap();
        store.mark_completed(&j1.id, "ok", &[]).unwrap();
        store.mark_webhook_sent(&j1.id).unwrap();

        let pending = store.list_pending_webhooks().unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn pending_webhooks_excludes_no_url() {
        let store = open_mem();
        let j1 = make_job("t1", "a");
        store.insert(&j1).unwrap();
        store.mark_completed(&j1.id, "ok", &[]).unwrap();

        let pending = store.list_pending_webhooks().unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn recover_stale_interrupts_running_jobs() {
        let store = open_mem();
        let j1 = make_job("t1", "a");
        let j2 = make_job("t1", "b");
        store.insert(&j1).unwrap();
        store.insert(&j2).unwrap();
        store
            .update_status(&j1.id, &JobStatus::Running, "", "")
            .unwrap();

        let recovered = store.recover_stale().unwrap();
        assert_eq!(recovered.len(), 2);
        for j in &recovered {
            assert_eq!(j.status, JobStatus::Interrupted);
            assert_eq!(j.error, "recovered after restart");
        }
    }

    #[test]
    fn cleanup_removes_old_completed_jobs() {
        let store = open_mem();
        let mut j1 = make_job("t1", "a");
        j1.created_at = 1000;
        j1.completed_at = 1010;
        j1.status = JobStatus::Completed;
        store.insert(&j1).unwrap();

        // retention_secs=0 means cutoff = now, should delete old job
        let deleted = store.cleanup(0).unwrap();
        assert_eq!(deleted, 1);
        assert!(store.get(&j1.id).unwrap().is_none());
    }

    #[test]
    fn cleanup_preserves_recent_jobs() {
        let store = open_mem();
        let j1 = make_job("t1", "a");
        store.insert(&j1).unwrap();
        store.mark_completed(&j1.id, "ok", &[]).unwrap();

        // retention = very large → nothing should be deleted
        let deleted = store.cleanup(999_999_999).unwrap();
        assert_eq!(deleted, 0);
        assert!(store.get(&j1.id).unwrap().is_some());
    }
}
