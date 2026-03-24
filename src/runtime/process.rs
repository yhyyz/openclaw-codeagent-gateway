//! Agent process lifecycle management.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, oneshot, Mutex};
use tokio::task::JoinHandle;

use crate::auth::policy::ExecutionContext;
use crate::auth::quota::QuotaGuard;
use crate::config::AgentConfig;
use crate::error::Error;
use crate::runtime::event::AgentEvent;
use crate::runtime::protocol::*;

pub struct AgentProcess {
    child_pid: u32,
    writer: Arc<Mutex<BufWriter<ChildStdin>>>,
    next_id: AtomicU64,
    pending: Arc<DashMap<u64, oneshot::Sender<RpcMessage>>>,
    events_tx: broadcast::Sender<serde_json::Value>,
    _read_handle: JoinHandle<()>,
    _child: Mutex<Child>,
    last_active: AtomicI64,
    _session_guard: QuotaGuard,
}

impl std::fmt::Debug for AgentProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentProcess")
            .field("pid", &self.child_pid)
            .finish()
    }
}

impl AgentProcess {
    pub async fn spawn(
        cfg: &AgentConfig,
        context: &ExecutionContext,
        session_guard: QuotaGuard,
    ) -> Result<Self, Error> {
        let mut cmd = Command::new(&cfg.command);
        cmd.args(&cfg.acp_args);
        cmd.current_dir(&context.workspace);

        cmd.env_clear();
        cmd.env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()));
        cmd.env("HOME", std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
        cmd.env("TERM", "dumb");
        cmd.env("LANG", "en_US.UTF-8");
        for (k, v) in &context.env_inject {
            if !context.env_deny.contains(k) {
                cmd.env(k, v);
            }
        }
        for (k, v) in &cfg.env {
            if !context.env_deny.contains(k) {
                cmd.env(k, v);
            }
        }

        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        cmd.kill_on_drop(true);
        cmd.process_group(0);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);

        let stdin = child.stdin.take().ok_or_else(|| {
            Error::AgentCrashed("failed to capture stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            Error::AgentCrashed("failed to capture stdout".into())
        })?;

        let writer = Arc::new(Mutex::new(BufWriter::new(stdin)));
        let pending: Arc<DashMap<u64, oneshot::Sender<RpcMessage>>> = Arc::new(DashMap::new());
        let (events_tx, _) = broadcast::channel(256);

        let read_pending = Arc::clone(&pending);
        let read_events = events_tx.clone();
        let read_writer = Arc::clone(&writer);

        let read_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let parsed: serde_json::Value = match serde_json::from_str(trimmed) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if let Some(evt) = AgentEvent::from_notification(&parsed) {
                    if let AgentEvent::PermissionRequest { rpc_id, .. } = &evt {
                        let reply = build_permission_reply(*rpc_id);
                        let reply_str = serde_json::to_string(&reply).unwrap_or_default();
                        let mut w = read_writer.lock().await;
                        let _ = w.write_all(reply_str.as_bytes()).await;
                        let _ = w.write_all(b"\n").await;
                        let _ = w.flush().await;
                    }
                    let _ = read_events.send(parsed.clone());
                    continue;
                }

                let msg: RpcMessage = match serde_json::from_value(parsed.clone()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };

                if let Some(id) = msg.id {
                    if let Some((_, tx)) = read_pending.remove(&id) {
                        let _ = tx.send(msg);
                        continue;
                    }
                }

                let _ = read_events.send(parsed);
            }
        });

        let now = chrono::Utc::now().timestamp();

        Ok(Self {
            child_pid: pid,
            writer,
            next_id: AtomicU64::new(1),
            pending,
            events_tx,
            _read_handle: read_handle,
            _child: Mutex::new(child),
            last_active: AtomicI64::new(now),
            _session_guard: session_guard,
        })
    }

    pub async fn send_rpc(&self, req: &RpcRequest) -> Result<RpcMessage, Error> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(req.id, tx);

        let json = serde_json::to_string(req)
            .map_err(|e| Error::Internal(anyhow::anyhow!("serialize rpc: {}", e)))?;

        {
            let mut w = self.writer.lock().await;
            w.write_all(json.as_bytes()).await?;
            w.write_all(b"\n").await?;
            w.flush().await?;
        }

        self.last_active.store(chrono::Utc::now().timestamp(), Ordering::Relaxed);

        rx.await.map_err(|_| Error::AgentCrashed("read loop terminated".into()))
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<serde_json::Value> {
        self.events_tx.subscribe()
    }

    pub async fn alive(&self) -> bool {
        let mut child = self._child.lock().await;
        matches!(child.try_wait(), Ok(None))
    }

    pub fn last_active(&self) -> i64 {
        self.last_active.load(Ordering::Relaxed)
    }

    pub fn pid(&self) -> u32 {
        self.child_pid
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        if self.child_pid > 0 {
            let pgid = nix::unistd::Pid::from_raw(self.child_pid as i32);
            let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::Duration;
    use crate::auth::quota::QuotaTracker;

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
        tracker.try_acquire_session("test-proc", 100).unwrap()
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
    async fn spawn_cat_and_echo_line() {
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();
        let proc = AgentProcess::spawn(&cfg, &ctx, guard).await.unwrap();

        assert!(proc.alive().await);
        assert!(proc.pid() > 0);

        // cat echoes back what we write — send a JSON-RPC line, read it back
        let test_msg = r#"{"jsonrpc":"2.0","id":99,"result":{"ok":true}}"#;
        {
            let mut w = proc.writer.lock().await;
            w.write_all(test_msg.as_bytes()).await.unwrap();
            w.write_all(b"\n").await.unwrap();
            w.flush().await.unwrap();
        }

        // Give read loop time to process
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    #[tokio::test]
    async fn drop_kills_process() {
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();
        let proc = AgentProcess::spawn(&cfg, &ctx, guard).await.unwrap();
        let pid = proc.pid();
        assert!(pid > 0);

        drop(proc);
        tokio::time::sleep(Duration::from_millis(100)).await;

        // After drop, the process group should be killed
        let alive = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid as i32),
            None,
        );
        assert!(alive.is_err(), "process should be dead after drop");
    }

    #[tokio::test]
    async fn spawn_nonexistent_command_fails() {
        let mut cfg = cat_config();
        cfg.command = "/nonexistent/binary/12345".into();
        let ctx = test_context();
        let guard = test_guard();
        let result = AgentProcess::spawn(&cfg, &ctx, guard).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn next_id_increments() {
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();
        let proc = AgentProcess::spawn(&cfg, &ctx, guard).await.unwrap();
        let id1 = proc.next_id();
        let id2 = proc.next_id();
        assert_eq!(id2, id1 + 1);
    }

    #[tokio::test]
    async fn send_rpc_to_cat_gets_echo() {
        let cfg = cat_config();
        let ctx = test_context();
        let guard = test_guard();
        let proc = AgentProcess::spawn(&cfg, &ctx, guard).await.unwrap();

        let req = RpcRequest::new(1, "test/method", Some(serde_json::json!({"key": "val"})));
        // cat echoes back the JSON line, which the read loop parses as a response with id=1
        let resp = tokio::time::timeout(Duration::from_secs(2), proc.send_rpc(&req))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resp.id, Some(1));
    }
}
