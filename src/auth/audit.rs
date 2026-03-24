//! Async JSONL audit log writer.

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub ts: DateTime<Utc>,
    pub request_id: String,
    pub tenant: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

pub struct AuditLog {
    tx: mpsc::UnboundedSender<AuditEntry>,
}

impl AuditLog {
    pub fn new(path: std::path::PathBuf) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<AuditEntry>();
        tokio::spawn(async move {
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .ok();
            while let Some(entry) = rx.recv().await {
                if let (Some(f), Ok(line)) = (file.as_mut(), serde_json::to_string(&entry)) {
                    let _ = f.write_all(format!("{}\n", line).as_bytes()).await;
                }
            }
        });
        Self { tx }
    }

    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { tx }
    }

    pub fn record(&self, entry: AuditEntry) {
        let _ = self.tx.send(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    fn make_entry(action: &str, verdict: &str) -> AuditEntry {
        AuditEntry {
            ts: Utc::now(),
            request_id: "req-001".into(),
            tenant: "team-a".into(),
            action: action.into(),
            agent: Some("kiro".into()),
            resource: None,
            verdict: verdict.into(),
            reason: None,
            duration_ms: Some(42),
        }
    }

    #[tokio::test]
    async fn write_entries_to_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let log = AuditLog::new(path.clone());
        log.record(make_entry("call_agent", "allow"));
        log.record(make_entry("submit_job", "deny"));

        drop(log);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let file = std::fs::File::open(&path).unwrap();
        let lines: Vec<String> = std::io::BufReader::new(file)
            .lines()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(lines.len(), 2);

        let v: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(v["action"], "call_agent");
        assert_eq!(v["verdict"], "allow");
        assert_eq!(v["tenant"], "team-a");

        let v2: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(v2["action"], "submit_job");
        assert_eq!(v2["verdict"], "deny");
    }

    #[tokio::test]
    async fn noop_does_not_panic() {
        let log = AuditLog::noop();
        log.record(make_entry("test", "allow"));
    }

    #[tokio::test]
    async fn optional_fields_skipped_when_none() {
        let entry = AuditEntry {
            ts: Utc::now(),
            request_id: "req-002".into(),
            tenant: "t".into(),
            action: "a".into(),
            agent: None,
            resource: None,
            verdict: "allow".into(),
            reason: None,
            duration_ms: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("agent"));
        assert!(!json.contains("resource"));
        assert!(!json.contains("reason"));
        assert!(!json.contains("duration_ms"));
    }
}
