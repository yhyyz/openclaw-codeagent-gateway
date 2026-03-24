//! Process pool and reuse management.

use std::sync::Arc;

use dashmap::DashMap;

use crate::auth::policy::ExecutionContext;
use crate::auth::quota::QuotaGuard;
use crate::config::AgentConfig;
use crate::error::Error;
use crate::runtime::process::AgentProcess;

pub struct ProcessPool {
    connections: DashMap<(String, String), Arc<AgentProcess>>,
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total: usize,
    pub by_agent: Vec<(String, usize)>,
}

impl Default for ProcessPool {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessPool {
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn acquire(
        &self,
        agent: &str,
        session_id: &str,
        agent_cfg: &AgentConfig,
        context: &ExecutionContext,
        session_guard: QuotaGuard,
        max_processes: usize,
        max_per_agent: usize,
    ) -> Result<Arc<AgentProcess>, Error> {
        let key = (agent.to_string(), session_id.to_string());

        if let Some(existing) = self.connections.get(&key) {
            if existing.alive().await {
                return Ok(Arc::clone(&existing));
            }
            drop(existing);
            self.connections.remove(&key);
        }

        if self.connections.len() >= max_processes {
            return Err(Error::PoolExhausted(format!(
                "total limit {} reached",
                max_processes
            )));
        }

        let agent_count = self
            .connections
            .iter()
            .filter(|e| e.key().0 == agent)
            .count();
        if agent_count >= max_per_agent {
            return Err(Error::PoolExhausted(format!(
                "per-agent limit {} reached for '{}'",
                max_per_agent, agent
            )));
        }

        let proc = AgentProcess::spawn(agent_cfg, context, session_guard).await?;
        let arc = Arc::new(proc);
        self.connections.insert(key, Arc::clone(&arc));
        Ok(arc)
    }

    /// Check if there's capacity for a new process for this agent.
    /// This is a best-effort pre-flight check (race possible but acceptable).
    pub fn has_capacity(&self, agent: &str, max_processes: usize, max_per_agent: usize) -> bool {
        if self.connections.len() >= max_processes {
            return false;
        }
        let agent_count = self
            .connections
            .iter()
            .filter(|e| e.key().0 == agent)
            .count();
        agent_count < max_per_agent
    }

    pub fn release(&self, agent: &str, session_id: &str) {
        let key = (agent.to_string(), session_id.to_string());
        self.connections.remove(&key);
    }

    pub fn close_session(&self, agent: &str, session_id: &str) {
        self.release(agent, session_id);
    }

    pub fn stats(&self) -> PoolStats {
        let mut by_agent: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for entry in self.connections.iter() {
            *by_agent.entry(entry.key().0.clone()).or_insert(0) += 1;
        }
        let total = self.connections.len();
        let by_agent_vec: Vec<(String, usize)> = by_agent.into_iter().collect();
        PoolStats {
            total,
            by_agent: by_agent_vec,
        }
    }

    pub async fn cleanup_idle(&self, ttl_secs: i64) {
        let now = chrono::Utc::now().timestamp();
        let mut to_remove = Vec::new();
        for entry in self.connections.iter() {
            let last = entry.value().last_active();
            if now - last > ttl_secs {
                to_remove.push(entry.key().clone());
            }
        }
        for key in to_remove {
            self.connections.remove(&key);
        }
    }

    pub fn shutdown(&self) {
        self.connections.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::quota::QuotaTracker;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Duration;

    fn test_context() -> ExecutionContext {
        ExecutionContext {
            tenant_id: "test".into(),
            workspace: PathBuf::from("/tmp"),
            env_inject: HashMap::new(),
            env_deny: HashSet::new(),
            session_ttl: Duration::from_secs(3600),
            idle_timeout: Duration::from_secs(600),
        }
    }

    fn test_guard() -> QuotaGuard {
        let tracker = QuotaTracker::new();
        tracker.try_acquire_session("test-pool", 100).unwrap()
    }

    fn cat_config() -> AgentConfig {
        AgentConfig {
            enabled: true,
            mode: "acp".into(),
            command: "cat".into(),
            acp_args: vec![],
            pty_args: vec![],
            working_dir: "/tmp".into(),
            description: String::new(),
            env: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn acquire_creates_entry() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();

        let proc = pool.acquire("cat", "s1", &cfg, &ctx, guard, 10, 5).await.unwrap();
        assert!(proc.alive().await);

        let stats = pool.stats();
        assert_eq!(stats.total, 1);
    }

    #[tokio::test]
    async fn acquire_reuses_existing() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        let p1 = pool.acquire("cat", "s1", &cfg, &ctx, g1, 10, 5).await.unwrap();
        let pid1 = p1.pid();

        let g2 = test_guard();
        let p2 = pool.acquire("cat", "s1", &cfg, &ctx, g2, 10, 5).await.unwrap();
        assert_eq!(p2.pid(), pid1);

        assert_eq!(pool.stats().total, 1);
    }

    #[tokio::test]
    async fn release_removes_entry() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();

        pool.acquire("cat", "s1", &cfg, &ctx, guard, 10, 5).await.unwrap();
        assert_eq!(pool.stats().total, 1);

        pool.release("cat", "s1");
        assert_eq!(pool.stats().total, 0);
    }

    #[tokio::test]
    async fn pool_exhausted_at_max_processes() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        pool.acquire("cat", "s1", &cfg, &ctx, g1, 1, 5).await.unwrap();

        let g2 = test_guard();
        let result = pool.acquire("cat", "s2", &cfg, &ctx, g2, 1, 5).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("total limit"));
    }

    #[tokio::test]
    async fn pool_exhausted_at_max_per_agent() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        pool.acquire("cat", "s1", &cfg, &ctx, g1, 10, 1).await.unwrap();

        let g2 = test_guard();
        let result = pool.acquire("cat", "s2", &cfg, &ctx, g2, 10, 1).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("per-agent limit"));
    }

    #[tokio::test]
    async fn stats_counts_by_agent() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        pool.acquire("cat", "s1", &cfg, &ctx, g1, 10, 5).await.unwrap();
        let g2 = test_guard();
        pool.acquire("cat", "s2", &cfg, &ctx, g2, 10, 5).await.unwrap();

        let stats = pool.stats();
        assert_eq!(stats.total, 2);
        let cat_count = stats.by_agent.iter().find(|(a, _)| a == "cat").map(|(_, c)| *c);
        assert_eq!(cat_count, Some(2));
    }

    #[tokio::test]
    async fn shutdown_clears_all() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        pool.acquire("cat", "s1", &cfg, &ctx, g1, 10, 5).await.unwrap();
        let g2 = test_guard();
        pool.acquire("cat", "s2", &cfg, &ctx, g2, 10, 5).await.unwrap();

        pool.shutdown();
        assert_eq!(pool.stats().total, 0);
    }

    #[tokio::test]
    async fn close_session_same_as_release() {
        let pool = ProcessPool::new();
        let cfg = cat_config();
        let ctx = test_context();

        let g1 = test_guard();
        pool.acquire("cat", "s1", &cfg, &ctx, g1, 10, 5).await.unwrap();
        pool.close_session("cat", "s1");
        assert_eq!(pool.stats().total, 0);
    }
}
