//! Concurrent quota tracking with RAII guards.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use crate::error::Error;

/// Tracks per-tenant concurrent usage with atomic counters.
pub struct QuotaTracker {
    sessions: DashMap<String, Arc<AtomicUsize>>,
    jobs: DashMap<String, Arc<AtomicUsize>>,
}

/// RAII guard that decrements the counter on drop.
#[derive(Debug)]
pub struct QuotaGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for QuotaGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for QuotaTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaTracker {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            jobs: DashMap::new(),
        }
    }

    /// Try to acquire a session slot for the given tenant.
    pub fn try_acquire_session(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<QuotaGuard, Error> {
        self.try_acquire(&self.sessions, tenant_id, limit, "sessions")
    }

    /// Try to acquire a job slot for the given tenant.
    pub fn try_acquire_job(&self, tenant_id: &str, limit: usize) -> Result<QuotaGuard, Error> {
        self.try_acquire(&self.jobs, tenant_id, limit, "jobs")
    }

    /// Return current (sessions, jobs) counts for a tenant.
    pub fn snapshot(&self, tenant_id: &str) -> (usize, usize) {
        let sessions = self
            .sessions
            .get(tenant_id)
            .map_or(0, |c| c.load(Ordering::SeqCst));
        let jobs = self
            .jobs
            .get(tenant_id)
            .map_or(0, |c| c.load(Ordering::SeqCst));
        (sessions, jobs)
    }

    fn try_acquire(
        &self,
        map: &DashMap<String, Arc<AtomicUsize>>,
        tenant_id: &str,
        limit: usize,
        kind: &str,
    ) -> Result<QuotaGuard, Error> {
        let counter = map
            .entry(tenant_id.to_string())
            .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
            .clone();

        let prev = counter.fetch_add(1, Ordering::SeqCst);
        if prev >= limit {
            counter.fetch_sub(1, Ordering::SeqCst);
            return Err(Error::QuotaExceeded(format!(
                "{}: {}/{} for '{}'",
                kind, prev, limit, tenant_id
            )));
        }

        Ok(QuotaGuard { counter })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_session_up_to_limit() {
        let qt = QuotaTracker::new();
        let _g1 = qt.try_acquire_session("t1", 2).unwrap();
        let _g2 = qt.try_acquire_session("t1", 2).unwrap();
        assert_eq!(qt.snapshot("t1").0, 2);
    }

    #[test]
    fn exceed_session_limit_returns_error() {
        let qt = QuotaTracker::new();
        let _g1 = qt.try_acquire_session("t1", 1).unwrap();
        let err = qt.try_acquire_session("t1", 1).unwrap_err();
        assert!(err.to_string().contains("sessions"));
        assert!(err.to_string().contains("t1"));
    }

    #[test]
    fn drop_guard_frees_slot() {
        let qt = QuotaTracker::new();
        {
            let _g = qt.try_acquire_session("t1", 1).unwrap();
            assert_eq!(qt.snapshot("t1").0, 1);
        }
        assert_eq!(qt.snapshot("t1").0, 0);

        let _g2 = qt.try_acquire_session("t1", 1).unwrap();
        assert_eq!(qt.snapshot("t1").0, 1);
    }

    #[test]
    fn job_quota_independent_of_session() {
        let qt = QuotaTracker::new();
        let _s = qt.try_acquire_session("t1", 1).unwrap();
        let _j = qt.try_acquire_job("t1", 1).unwrap();
        assert_eq!(qt.snapshot("t1"), (1, 1));
    }

    #[test]
    fn exceed_job_limit_returns_error() {
        let qt = QuotaTracker::new();
        let _g1 = qt.try_acquire_job("t1", 1).unwrap();
        let err = qt.try_acquire_job("t1", 1).unwrap_err();
        assert!(err.to_string().contains("jobs"));
    }

    #[test]
    fn snapshot_unknown_tenant_is_zero() {
        let qt = QuotaTracker::new();
        assert_eq!(qt.snapshot("unknown"), (0, 0));
    }

    #[test]
    fn separate_tenants_are_independent() {
        let qt = QuotaTracker::new();
        let _g1 = qt.try_acquire_session("t1", 1).unwrap();
        let _g2 = qt.try_acquire_session("t2", 1).unwrap();
        assert_eq!(qt.snapshot("t1").0, 1);
        assert_eq!(qt.snapshot("t2").0, 1);
    }
}
