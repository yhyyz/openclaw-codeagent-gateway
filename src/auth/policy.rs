//! Five-dimension authorization policy engine.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use crate::auth::tenant::Tenant;

pub enum Action<'a> {
    ListAgents,
    CallAgent { agent: &'a str, stream: bool },
    SubmitJob {
        agent: &'a str,
        prompt_len: usize,
        callback: Option<&'a CallbackRequest>,
    },
    QueryJobs,
    ManageSession { agent: &'a str },
    Admin,
}

#[derive(Debug, Clone)]
pub struct CallbackRequest {
    pub url: String,
    pub channel: String,
    pub target: String,
}

pub enum Verdict {
    Allow(ExecutionContext),
    Deny(String),
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub tenant_id: String,
    pub workspace: PathBuf,
    pub env_inject: HashMap<String, String>,
    pub env_deny: HashSet<String>,
    pub session_ttl: Duration,
    pub idle_timeout: Duration,
}

pub struct PolicyEngine;

impl PolicyEngine {
    pub fn evaluate(tenant: &Tenant, action: &Action) -> Verdict {
        if let Err(reason) = Self::check_operation(tenant, action) {
            return Verdict::Deny(reason);
        }
        if let Err(reason) = Self::check_agent(tenant, action) {
            return Verdict::Deny(reason);
        }
        if let Err(reason) = Self::check_prompt_length(tenant, action) {
            return Verdict::Deny(reason);
        }
        if let Err(reason) = Self::check_callback(tenant, action) {
            return Verdict::Deny(reason);
        }
        Verdict::Allow(Self::build_context(tenant))
    }

    fn check_operation(tenant: &Tenant, action: &Action) -> Result<(), String> {
        let ops = &tenant.policy.operations;
        match action {
            Action::ListAgents | Action::QueryJobs => Ok(()),
            Action::CallAgent { stream: false, .. } => {
                if ops.sync_call {
                    Ok(())
                } else {
                    Err("sync_call not permitted".into())
                }
            }
            Action::CallAgent { stream: true, .. } => {
                if ops.stream {
                    Ok(())
                } else {
                    Err("stream not permitted".into())
                }
            }
            Action::SubmitJob { .. } => {
                if ops.async_jobs {
                    Ok(())
                } else {
                    Err("async_jobs not permitted".into())
                }
            }
            Action::ManageSession { .. } => {
                if ops.session_manage {
                    Ok(())
                } else {
                    Err("session_manage not permitted".into())
                }
            }
            Action::Admin => {
                if ops.admin {
                    Ok(())
                } else {
                    Err("admin not permitted".into())
                }
            }
        }
    }

    fn check_agent(tenant: &Tenant, action: &Action) -> Result<(), String> {
        let agent_name = match action {
            Action::CallAgent { agent, .. }
            | Action::SubmitJob { agent, .. }
            | Action::ManageSession { agent } => agent,
            _ => return Ok(()),
        };

        let ap = &tenant.policy.agents;

        if ap.deny.iter().any(|p| pattern_matches(p, agent_name)) {
            return Err(format!("agent '{}' is denied", agent_name));
        }

        if ap.allow.iter().any(|p| pattern_matches(p, agent_name)) {
            Ok(())
        } else {
            Err(format!("agent '{}' not in allow list", agent_name))
        }
    }

    fn check_prompt_length(tenant: &Tenant, action: &Action) -> Result<(), String> {
        if let Action::SubmitJob { prompt_len, .. } = action {
            let limit = tenant.policy.quotas.max_prompt_length;
            if *prompt_len > limit {
                return Err(format!("prompt length {} exceeds limit {}", prompt_len, limit));
            }
        }
        Ok(())
    }

    fn check_callback(tenant: &Tenant, action: &Action) -> Result<(), String> {
        let cb = match action {
            Action::SubmitJob {
                callback: Some(cb), ..
            } => cb,
            _ => return Ok(()),
        };

        let cp = &tenant.policy.callbacks;

        if !cp
            .allowed_urls
            .iter()
            .any(|pattern| pattern_matches(pattern, &cb.url))
        {
            return Err(format!("callback url '{}' not allowed", cb.url));
        }

        let channel_ok = cp.allowed_channels.iter().any(|rule| {
            rule.channel == cb.channel
                && rule
                    .targets
                    .iter()
                    .any(|pattern| target_matches(pattern, &cb.target))
        });

        if !channel_ok {
            return Err(format!(
                "callback channel '{}' target '{}' not allowed",
                cb.channel, cb.target
            ));
        }

        Ok(())
    }

    fn build_context(tenant: &Tenant) -> ExecutionContext {
        let p = &tenant.policy;
        ExecutionContext {
            tenant_id: tenant.id.clone(),
            workspace: p.resources.workspace.clone(),
            env_inject: p.resources.env_inject.clone(),
            env_deny: p.resources.env_deny.iter().cloned().collect(),
            session_ttl: Duration::from_secs(p.quotas.session_ttl_hours * 3600),
            idle_timeout: Duration::from_secs(p.quotas.session_ttl_hours * 3600 / 4),
        }
    }
}

/// Glob-style pattern matching for agent names and URLs.
/// Supports `*` as a wildcard suffix.
fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

/// Target matching for callback channel targets.
/// `"*"` matches everything, `"user:*"` matches `"user:123"`, exact otherwise.
fn target_matches(pattern: &str, target: &str) -> bool {
    pattern_matches(pattern, target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::tenant::*;
    use std::path::PathBuf;

    fn make_tenant() -> Tenant {
        Tenant {
            id: "test-tenant".into(),
            policy: TenantPolicy {
                agents: AgentPolicy {
                    allow: vec!["kiro".into(), "codex".into()],
                    deny: vec!["dangerous".into()],
                },
                operations: OperationPolicy {
                    sync_call: true,
                    stream: true,
                    async_jobs: true,
                    session_manage: true,
                    admin: false,
                },
                resources: ResourcePolicy {
                    workspace: PathBuf::from("/tmp/test"),
                    env_inject: HashMap::from([("TEAM".into(), "alpha".into())]),
                    env_deny: vec!["SECRET".into()],
                },
                quotas: QuotaLimits {
                    max_concurrent_sessions: 5,
                    max_concurrent_jobs: 10,
                    max_prompt_length: 4096,
                    session_ttl_hours: 24,
                },
                callbacks: CallbackPolicy {
                    allowed_urls: vec!["https://hooks.example.com/*".into()],
                    allowed_channels: vec![ChannelRule {
                        channel: "slack".into(),
                        targets: vec!["#ops".into(), "user:*".into()],
                    }],
                },
            },
        }
    }

    fn make_restricted_tenant() -> Tenant {
        Tenant {
            id: "restricted".into(),
            policy: TenantPolicy {
                agents: AgentPolicy {
                    allow: vec!["kiro".into()],
                    deny: vec![],
                },
                operations: OperationPolicy {
                    sync_call: true,
                    stream: false,
                    async_jobs: false,
                    session_manage: false,
                    admin: false,
                },
                resources: ResourcePolicy {
                    workspace: PathBuf::from("/tmp/r"),
                    env_inject: HashMap::new(),
                    env_deny: vec![],
                },
                quotas: QuotaLimits {
                    max_concurrent_sessions: 1,
                    max_concurrent_jobs: 1,
                    max_prompt_length: 1024,
                    session_ttl_hours: 1,
                },
                callbacks: CallbackPolicy {
                    allowed_urls: vec![],
                    allowed_channels: vec![],
                },
            },
        }
    }

    #[test]
    fn test_agent_allowed() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "kiro", stream: false });
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_agent_denied_not_in_allow() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "unknown", stream: false });
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("not in allow list")));
    }

    #[test]
    fn test_agent_in_deny_list() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "dangerous", stream: false });
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("denied")));
    }

    #[test]
    fn test_operation_sync_allowed() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "kiro", stream: false });
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_operation_async_denied() {
        let t = make_restricted_tenant();
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: None,
            },
        );
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("async_jobs")));
    }

    #[test]
    fn test_operation_stream_denied() {
        let t = make_restricted_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "kiro", stream: true });
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("stream")));
    }

    #[test]
    fn test_operation_admin_denied() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::Admin);
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("admin")));
    }

    #[test]
    fn test_prompt_length_ok() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 1000,
                callback: None,
            },
        );
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_prompt_too_long() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 5000,
                callback: None,
            },
        );
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("prompt length")));
    }

    #[test]
    fn test_callback_url_allowed() {
        let t = make_tenant();
        let cb = CallbackRequest {
            url: "https://hooks.example.com/webhook".into(),
            channel: "slack".into(),
            target: "#ops".into(),
        };
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: Some(&cb),
            },
        );
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_callback_url_denied() {
        let t = make_tenant();
        let cb = CallbackRequest {
            url: "https://evil.com/steal".into(),
            channel: "slack".into(),
            target: "#ops".into(),
        };
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: Some(&cb),
            },
        );
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("callback url")));
    }

    #[test]
    fn test_callback_channel_wildcard() {
        let t = make_tenant();
        let cb = CallbackRequest {
            url: "https://hooks.example.com/wh".into(),
            channel: "slack".into(),
            target: "user:123".into(),
        };
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: Some(&cb),
            },
        );
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_callback_channel_exact() {
        let t = make_tenant();
        let cb = CallbackRequest {
            url: "https://hooks.example.com/wh".into(),
            channel: "slack".into(),
            target: "#ops".into(),
        };
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: Some(&cb),
            },
        );
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_callback_channel_denied() {
        let t = make_tenant();
        let cb = CallbackRequest {
            url: "https://hooks.example.com/wh".into(),
            channel: "slack".into(),
            target: "#random".into(),
        };
        let v = PolicyEngine::evaluate(
            &t,
            &Action::SubmitJob {
                agent: "kiro",
                prompt_len: 100,
                callback: Some(&cb),
            },
        );
        assert!(matches!(v, Verdict::Deny(ref msg) if msg.contains("channel")));
    }

    #[test]
    fn test_target_matches_star() {
        assert!(target_matches("*", "anything"));
        assert!(target_matches("*", ""));
    }

    #[test]
    fn test_target_matches_prefix_wildcard() {
        assert!(target_matches("user:*", "user:123"));
        assert!(target_matches("user:*", "user:abc"));
        assert!(!target_matches("user:*", "admin:123"));
    }

    #[test]
    fn test_target_matches_exact() {
        assert!(target_matches("chat:456", "chat:456"));
        assert!(!target_matches("chat:456", "chat:789"));
    }

    #[test]
    fn test_pattern_matches_wildcard_all() {
        assert!(pattern_matches("*", "anything"));
    }

    #[test]
    fn test_list_agents_always_allowed() {
        let t = make_restricted_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::ListAgents);
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_query_jobs_always_allowed() {
        let t = make_restricted_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::QueryJobs);
        assert!(matches!(v, Verdict::Allow(_)));
    }

    #[test]
    fn test_allow_context_fields() {
        let t = make_tenant();
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "kiro", stream: false });
        match v {
            Verdict::Allow(ctx) => {
                assert_eq!(ctx.tenant_id, "test-tenant");
                assert_eq!(ctx.workspace, PathBuf::from("/tmp/test"));
                assert_eq!(ctx.env_inject["TEAM"], "alpha");
                assert!(ctx.env_deny.contains("SECRET"));
                assert_eq!(ctx.session_ttl, Duration::from_secs(24 * 3600));
            }
            Verdict::Deny(msg) => panic!("expected Allow, got Deny: {}", msg),
        }
    }

    #[test]
    fn test_wildcard_agent_allow() {
        let t = Tenant {
            id: "wildcard".into(),
            policy: TenantPolicy {
                agents: AgentPolicy {
                    allow: vec!["*".into()],
                    deny: vec![],
                },
                operations: OperationPolicy {
                    sync_call: true,
                    stream: true,
                    async_jobs: true,
                    session_manage: true,
                    admin: true,
                },
                resources: ResourcePolicy {
                    workspace: PathBuf::from("/tmp"),
                    env_inject: HashMap::new(),
                    env_deny: vec![],
                },
                quotas: QuotaLimits {
                    max_concurrent_sessions: 1,
                    max_concurrent_jobs: 1,
                    max_prompt_length: 4096,
                    session_ttl_hours: 1,
                },
                callbacks: CallbackPolicy {
                    allowed_urls: vec![],
                    allowed_channels: vec![],
                },
            },
        };
        let v = PolicyEngine::evaluate(&t, &Action::CallAgent { agent: "any-agent", stream: false });
        assert!(matches!(v, Verdict::Allow(_)));
    }
}
