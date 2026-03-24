//! Tenant management and registry.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{ChannelRuleConfig, GatewayConfig, TenantConfig};

// ── Core auth types ─────────────────────────────────────────────────

/// A resolved tenant with its policy.
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: String,
    pub policy: TenantPolicy,
}

#[derive(Debug, Clone)]
pub struct TenantPolicy {
    pub agents: AgentPolicy,
    pub operations: OperationPolicy,
    pub resources: ResourcePolicy,
    pub quotas: QuotaLimits,
    pub callbacks: CallbackPolicy,
}

#[derive(Debug, Clone)]
pub struct AgentPolicy {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OperationPolicy {
    pub sync_call: bool,
    pub stream: bool,
    pub async_jobs: bool,
    pub session_manage: bool,
    pub admin: bool,
}

#[derive(Debug, Clone)]
pub struct ResourcePolicy {
    pub workspace: PathBuf,
    pub env_inject: HashMap<String, String>,
    pub env_deny: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QuotaLimits {
    pub max_concurrent_sessions: usize,
    pub max_concurrent_jobs: usize,
    pub max_prompt_length: usize,
    pub session_ttl_hours: u64,
}

#[derive(Debug, Clone)]
pub struct CallbackPolicy {
    pub allowed_urls: Vec<String>,
    pub allowed_channels: Vec<ChannelRule>,
}

#[derive(Debug, Clone)]
pub struct ChannelRule {
    pub channel: String,
    pub targets: Vec<String>,
}

// ── Conversions from config ─────────────────────────────────────────

impl Tenant {
    /// Convert a tenant config entry into a resolved `Tenant`.
    pub fn from_config(id: String, cfg: &TenantConfig) -> Self {
        let p = &cfg.policy;
        Self {
            id,
            policy: TenantPolicy {
                agents: AgentPolicy {
                    allow: p.agents.allow.clone(),
                    deny: p.agents.deny.clone(),
                },
                operations: OperationPolicy {
                    sync_call: p.operations.sync_call,
                    stream: p.operations.stream,
                    async_jobs: p.operations.async_jobs,
                    session_manage: p.operations.session_manage,
                    admin: p.operations.admin,
                },
                resources: ResourcePolicy {
                    workspace: PathBuf::from(&p.resources.workspace),
                    env_inject: p.resources.env_inject.clone(),
                    env_deny: p.resources.env_deny.clone(),
                },
                quotas: QuotaLimits {
                    max_concurrent_sessions: p.quotas.max_concurrent_sessions,
                    max_concurrent_jobs: p.quotas.max_concurrent_jobs,
                    max_prompt_length: p.quotas.max_prompt_length,
                    session_ttl_hours: p.quotas.session_ttl_hours,
                },
                callbacks: CallbackPolicy {
                    allowed_urls: p.callbacks.allowed_urls.clone(),
                    allowed_channels: p
                        .callbacks
                        .allowed_channels
                        .iter()
                        .map(ChannelRule::from_config)
                        .collect(),
                },
            },
        }
    }
}

impl ChannelRule {
    fn from_config(cfg: &ChannelRuleConfig) -> Self {
        Self {
            channel: cfg.channel.clone(),
            targets: cfg.targets.clone(),
        }
    }
}

// ── Registry ────────────────────────────────────────────────────────

/// Registry that maps bearer tokens to tenants.
pub struct TenantRegistry {
    by_token: HashMap<String, Tenant>,
}

impl TenantRegistry {
    /// Build registry from gateway config.
    pub fn from_config(config: &GatewayConfig) -> Self {
        let mut by_token = HashMap::new();
        for (tenant_id, tenant_cfg) in &config.tenants {
            let tenant = Tenant::from_config(tenant_id.clone(), tenant_cfg);
            for cred in &tenant_cfg.credentials {
                by_token.insert(cred.token.clone(), tenant.clone());
            }
        }
        Self { by_token }
    }

    /// Resolve a bearer token to a tenant.
    pub fn resolve(&self, token: &str) -> Option<&Tenant> {
        self.by_token.get(token)
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AgentPolicyConfig, CallbackPolicyConfig, ChannelRuleConfig, CredentialConfig,
        OperationPolicyConfig, QuotaPolicyConfig, ResourcePolicyConfig, TenantConfig,
        TenantPolicyConfig,
    };

    fn make_tenant_config() -> TenantConfig {
        TenantConfig {
            credentials: vec![
                CredentialConfig {
                    token: "tok_a".into(),
                },
                CredentialConfig {
                    token: "tok_b".into(),
                },
            ],
            policy: TenantPolicyConfig {
                agents: AgentPolicyConfig {
                    allow: vec!["kiro".into(), "codex".into()],
                    deny: vec!["dangerous".into()],
                },
                operations: OperationPolicyConfig {
                    sync_call: true,
                    stream: true,
                    async_jobs: false,
                    session_manage: true,
                    admin: false,
                },
                resources: ResourcePolicyConfig {
                    workspace: "/data/test".into(),
                    env_inject: HashMap::from([("TEAM".into(), "alpha".into())]),
                    env_deny: vec!["SECRET".into()],
                },
                quotas: QuotaPolicyConfig {
                    max_concurrent_sessions: 10,
                    max_concurrent_jobs: 20,
                    max_prompt_length: 8192,
                    session_ttl_hours: 12,
                },
                callbacks: CallbackPolicyConfig {
                    allowed_urls: vec!["https://hooks.example.com/*".into()],
                    allowed_channels: vec![ChannelRuleConfig {
                        channel: "slack".into(),
                        targets: vec!["#general".into(), "user:*".into()],
                    }],
                },
            },
        }
    }

    fn make_gateway_config() -> GatewayConfig {
        let yaml = r#"
server: {}
agents:
  test-agent:
    mode: acp
    command: /usr/bin/test
pool: {}
store: {}
tenants:
  team-a:
    credentials:
      - token: "tok_primary"
      - token: "tok_secondary"
    policy:
      agents:
        allow: ["test-agent"]
      operations:
        sync_call: true
"#;
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn test_from_config_maps_all_fields() {
        let cfg = make_tenant_config();
        let tenant = Tenant::from_config("team-alpha".into(), &cfg);

        assert_eq!(tenant.id, "team-alpha");
        assert_eq!(tenant.policy.agents.allow, vec!["kiro", "codex"]);
        assert_eq!(tenant.policy.agents.deny, vec!["dangerous"]);
        assert!(tenant.policy.operations.sync_call);
        assert!(tenant.policy.operations.stream);
        assert!(!tenant.policy.operations.async_jobs);
        assert!(tenant.policy.operations.session_manage);
        assert!(!tenant.policy.operations.admin);
        assert_eq!(
            tenant.policy.resources.workspace,
            PathBuf::from("/data/test")
        );
        assert_eq!(tenant.policy.resources.env_inject["TEAM"], "alpha");
        assert_eq!(tenant.policy.resources.env_deny, vec!["SECRET"]);
        assert_eq!(tenant.policy.quotas.max_concurrent_sessions, 10);
        assert_eq!(tenant.policy.quotas.max_concurrent_jobs, 20);
        assert_eq!(tenant.policy.quotas.max_prompt_length, 8192);
        assert_eq!(tenant.policy.quotas.session_ttl_hours, 12);
        assert_eq!(
            tenant.policy.callbacks.allowed_urls,
            vec!["https://hooks.example.com/*"]
        );
        assert_eq!(tenant.policy.callbacks.allowed_channels.len(), 1);
        assert_eq!(tenant.policy.callbacks.allowed_channels[0].channel, "slack");
        assert_eq!(
            tenant.policy.callbacks.allowed_channels[0].targets,
            vec!["#general", "user:*"]
        );
    }

    #[test]
    fn test_registry_resolves_valid_token() {
        let cfg = make_gateway_config();
        let registry = TenantRegistry::from_config(&cfg);

        let tenant = registry.resolve("tok_primary").expect("should resolve");
        assert_eq!(tenant.id, "team-a");

        let tenant2 = registry.resolve("tok_secondary").expect("should resolve");
        assert_eq!(tenant2.id, "team-a");
    }

    #[test]
    fn test_registry_returns_none_for_invalid() {
        let cfg = make_gateway_config();
        let registry = TenantRegistry::from_config(&cfg);

        assert!(registry.resolve("tok_nonexistent").is_none());
        assert!(registry.resolve("").is_none());
    }
}
