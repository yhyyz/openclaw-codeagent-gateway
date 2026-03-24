//! Job definition and state machine.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub tenant_id: String,
    pub agent: String,
    pub session_id: String,
    pub prompt: String,
    pub status: JobStatus,
    pub result: String,
    pub error: String,
    pub tools: Vec<String>,
    pub created_at: i64,
    pub completed_at: i64,
    pub callback_url: String,
    pub callback_routing: String,
    pub webhook_sent: bool,
    pub progress: String,
    pub progress_notify: bool,
}

impl Job {
    pub fn new(tenant_id: &str, agent: &str, session_id: &str, prompt: &str) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant_id.to_string(),
            agent: agent.to_string(),
            session_id: session_id.to_string(),
            prompt: prompt.to_string(),
            status: JobStatus::Pending,
            result: String::new(),
            error: String::new(),
            tools: Vec::new(),
            created_at: now,
            completed_at: 0,
            callback_url: String::new(),
            callback_routing: "{}".to_string(),
            webhook_sent: false,
            progress: String::new(),
            progress_notify: true,
        }
    }

    /// Returns seconds since creation, or until completion if completed_at > 0.
    pub fn duration_secs(&self) -> f64 {
        if self.completed_at > 0 {
            (self.completed_at - self.created_at) as f64
        } else {
            let now = chrono::Utc::now().timestamp();
            (now - self.created_at) as f64
        }
    }

    pub fn status_str(&self) -> &'static str {
        self.status.as_str()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackRequest {
    pub channel: String,
    pub target: String,
    pub account_id: String,
}

#[derive(Debug, Clone)]
pub struct CallbackTarget {
    pub url: String,
    pub token: Option<String>,
    pub routing: CallbackRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_job_has_uuid_and_pending_status() {
        let job = Job::new("tenant-1", "kiro", "sess-abc", "hello world");
        assert!(!job.id.is_empty());
        assert_eq!(job.id.len(), 36); // UUID v4 string length
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.tenant_id, "tenant-1");
        assert_eq!(job.agent, "kiro");
        assert_eq!(job.session_id, "sess-abc");
        assert_eq!(job.prompt, "hello world");
        assert!(job.result.is_empty());
        assert!(job.error.is_empty());
        assert!(job.tools.is_empty());
        assert_eq!(job.completed_at, 0);
        assert!(!job.webhook_sent);
    }

    #[test]
    fn status_str_returns_correct_values() {
        assert_eq!(JobStatus::Pending.as_str(), "pending");
        assert_eq!(JobStatus::Running.as_str(), "running");
        assert_eq!(JobStatus::Completed.as_str(), "completed");
        assert_eq!(JobStatus::Failed.as_str(), "failed");
        assert_eq!(JobStatus::Interrupted.as_str(), "interrupted");
    }

    #[test]
    fn job_serialization_round_trip() {
        let mut job = Job::new("t1", "agent", "s1", "prompt");
        job.status = JobStatus::Completed;
        job.result = "done".into();
        job.tools = vec!["read_file".into(), "write_file".into()];
        job.completed_at = job.created_at + 10;

        let json = serde_json::to_string(&job).unwrap();
        let restored: Job = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.id, job.id);
        assert_eq!(restored.status, JobStatus::Completed);
        assert_eq!(restored.result, "done");
        assert_eq!(restored.tools, vec!["read_file", "write_file"]);
    }

    #[test]
    fn duration_secs_with_completion() {
        let mut job = Job::new("t1", "a", "s1", "p");
        job.created_at = 1000;
        job.completed_at = 1042;
        assert!((job.duration_secs() - 42.0).abs() < f64::EPSILON);
    }

    #[test]
    fn duration_secs_without_completion_uses_now() {
        let job = Job::new("t1", "a", "s1", "p");
        // completed_at is 0, so it should compute from now
        let d = job.duration_secs();
        assert!(d >= 0.0);
    }

    #[test]
    fn callback_request_serialization() {
        let cr = CallbackRequest {
            channel: "hook".into(),
            target: "endpoint-1".into(),
            account_id: "acc-123".into(),
        };
        let json = serde_json::to_string(&cr).unwrap();
        let restored: CallbackRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.channel, "hook");
        assert_eq!(restored.target, "endpoint-1");
        assert_eq!(restored.account_id, "acc-123");
    }

    #[test]
    fn two_new_jobs_have_different_ids() {
        let j1 = Job::new("t", "a", "s", "p");
        let j2 = Job::new("t", "a", "s", "p");
        assert_ne!(j1.id, j2.id);
    }

    #[test]
    fn status_serde_round_trip() {
        let statuses = vec![
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Completed,
            JobStatus::Failed,
            JobStatus::Interrupted,
        ];
        for s in statuses {
            let json = serde_json::to_string(&s).unwrap();
            let restored: JobStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, s);
        }
    }

    #[test]
    fn status_serializes_as_snake_case() {
        assert_eq!(serde_json::to_string(&JobStatus::Pending).unwrap(), "\"pending\"");
        assert_eq!(serde_json::to_string(&JobStatus::Running).unwrap(), "\"running\"");
        assert_eq!(serde_json::to_string(&JobStatus::Completed).unwrap(), "\"completed\"");
        assert_eq!(serde_json::to_string(&JobStatus::Failed).unwrap(), "\"failed\"");
        assert_eq!(serde_json::to_string(&JobStatus::Interrupted).unwrap(), "\"interrupted\"");
    }
}
