//! Webhook delivery and retry logic.

use std::time::Duration;

use reqwest::Client;
use tracing;

use crate::dispatch::formatter;
use crate::scheduler::job::{CallbackTarget, Job};

/// Split a message into chunks of at most `max_len` characters,
/// preferring to split at newline boundaries.
pub fn split_message(message: &str, max_len: usize) -> Vec<String> {
    if message.len() <= max_len {
        return vec![message.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = message;
    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }
        // Try to split at last newline before max_len
        let split_at = remaining[..max_len].rfind('\n').unwrap_or(max_len);
        let split_at = if split_at == 0 { max_len } else { split_at };
        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }
    chunks
}

pub struct WebhookDispatcher {
    client: Client,
    max_retries: u32,
    base_delay: Duration,
}

impl WebhookDispatcher {
    pub fn new(max_retries: u32, base_delay_secs: u64) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("http client"),
            max_retries,
            base_delay: Duration::from_secs(base_delay_secs),
        }
    }

    pub fn build_payload(target: &CallbackTarget, job: &Job) -> serde_json::Value {
        let progress: serde_json::Value =
            serde_json::from_str(&job.progress).unwrap_or_default();

        let tool_counts: std::collections::HashMap<String, usize> = progress
            .get("tool_counts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let usage = progress.get("usage").unwrap_or(&serde_json::Value::Null);
        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_read_tokens = usage.get("cache_read_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_write_tokens = usage.get("cache_write_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let total_tokens = usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cost_usd = usage.get("cost_usd").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let message = formatter::format_result(
            &job.agent,
            &job.id,
            job.status_str(),
            &job.result,
            &job.error,
            &tool_counts,
            job.duration_secs(),
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            total_tokens,
            cost_usd,
        );
        serde_json::json!({
            "tool": "message",
            "args": {
                "action": "send",
                "channel": target.routing.channel,
                "target": target.routing.target,
                "message": message,
            },
            "sessionKey": "main"
        })
    }

    pub async fn deliver(&self, target: &CallbackTarget, job: &Job) -> bool {
        let payload = Self::build_payload(target, job);
        let message = payload["args"]["message"].as_str().unwrap_or_default();
        let chunks = split_message(message, 3800);

        for (i, chunk) in chunks.iter().enumerate() {
            let chunk_payload = serde_json::json!({
                "tool": "message",
                "args": {
                    "action": "send",
                    "channel": target.routing.channel,
                    "target": target.routing.target,
                    "message": chunk,
                },
                "sessionKey": "main"
            });

            let sent = self.send_single_payload(target, &chunk_payload, &job.id).await;
            if !sent {
                return false;
            }

            if i < chunks.len() - 1 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        true
    }

    async fn send_single_payload(
        &self,
        target: &CallbackTarget,
        payload: &serde_json::Value,
        job_id: &str,
    ) -> bool {
        for attempt in 0..=self.max_retries {
            let mut req = self.client.post(&target.url).json(payload);
            if let Some(token) = &target.token {
                req = req.bearer_auth(token);
            }
            req = req.header("x-gateway-account-id", &target.routing.account_id);
            req = req.header("x-gateway-message-channel", &target.routing.channel);

            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!(job_id = %job_id, attempt, "webhook delivered");
                    return true;
                }
                Ok(resp) => {
                    tracing::warn!(job_id = %job_id, status = %resp.status(), attempt, "webhook rejected");
                }
                Err(e) => {
                    tracing::warn!(job_id = %job_id, error = %e, attempt, "webhook failed");
                }
            }
            if attempt < self.max_retries {
                tokio::time::sleep(self.base_delay * 2u32.pow(attempt)).await;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::job::{CallbackRequest, JobStatus};

    fn make_test_job() -> Job {
        let mut job = Job::new("tenant-1", "kiro", "sess-1", "build it");
        job.status = JobStatus::Completed;
        job.result = "all done".into();
        job.tools = vec!["read_file".into(), "write_file".into()];
        job.progress = serde_json::json!({
            "tool_counts": {"read_file": 1, "write_file": 1}
        })
        .to_string();
        job.created_at = 1000;
        job.completed_at = 1042;
        job
    }

    fn make_test_target() -> CallbackTarget {
        CallbackTarget {
            url: "https://hooks.example.com/callback".into(),
            token: Some("secret_token".into()),
            routing: CallbackRequest {
                channel: "hook".into(),
                target: "endpoint-1".into(),
                account_id: "acc-123".into(),
            },
        }
    }

    #[test]
    fn payload_structure_is_correct() {
        let job = make_test_job();
        let target = make_test_target();
        let payload = WebhookDispatcher::build_payload(&target, &job);

        assert_eq!(payload["tool"], "message");
        assert_eq!(payload["sessionKey"], "main");
        assert_eq!(payload["args"]["action"], "send");
        assert_eq!(payload["args"]["channel"], "hook");
        assert_eq!(payload["args"]["target"], "endpoint-1");

        let message = payload["args"]["message"].as_str().unwrap();
        assert!(message.contains("kiro"));
        assert!(message.contains("all done"));
        assert!(message.contains("read_file"));
    }

    #[test]
    fn payload_contains_no_channel_specific_words() {
        let job = make_test_job();
        let target = make_test_target();
        let payload = WebhookDispatcher::build_payload(&target, &job);
        let payload_str = serde_json::to_string(&payload).unwrap().to_lowercase();

        assert!(!payload_str.contains("discord"));
        assert!(!payload_str.contains("telegram"));
        assert!(!payload_str.contains("whatsapp"));
    }

    #[test]
    fn payload_for_failed_job() {
        let mut job = make_test_job();
        job.status = JobStatus::Failed;
        job.result = String::new();
        job.error = "OOM killed".into();
        job.tools = vec![];
        let target = make_test_target();
        let payload = WebhookDispatcher::build_payload(&target, &job);

        let message = payload["args"]["message"].as_str().unwrap();
        assert!(message.contains("OOM killed"));
    }

    #[test]
    fn dispatcher_creation() {
        let dispatcher = WebhookDispatcher::new(3, 5);
        assert_eq!(dispatcher.max_retries, 3);
        assert_eq!(dispatcher.base_delay, Duration::from_secs(5));
    }

    #[test]
    fn payload_has_routing_fields() {
        let job = make_test_job();
        let target = make_test_target();
        let payload = WebhookDispatcher::build_payload(&target, &job);

        assert!(payload["args"].get("channel").is_some());
        assert!(payload["args"].get("target").is_some());
        assert!(payload["args"].get("message").is_some());
    }

    #[test]
    fn split_message_short_returns_single() {
        let msg = "Hello, world!";
        let chunks = split_message(msg, 3800);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], msg);
    }

    #[test]
    fn split_message_exact_limit_returns_single() {
        let msg = "a".repeat(3800);
        let chunks = split_message(&msg, 3800);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], msg);
    }

    #[test]
    fn split_message_splits_at_newlines() {
        let line = "x".repeat(100);
        let mut lines = Vec::new();
        for _ in 0..50 {
            lines.push(line.clone());
        }
        let msg = lines.join("\n");
        assert!(msg.len() > 3800);

        let chunks = split_message(&msg, 3800);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= 3800);
        }
        let rejoined = chunks.join("\n");
        assert_eq!(rejoined, msg);
    }

    #[test]
    fn split_message_no_newlines_splits_at_max() {
        let msg = "a".repeat(8000);
        let chunks = split_message(&msg, 3800);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 3800);
        assert_eq!(chunks[1].len(), 3800);
        assert_eq!(chunks[2].len(), 400);
    }

    #[test]
    fn split_message_empty_returns_single() {
        let chunks = split_message("", 3800);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }
}
