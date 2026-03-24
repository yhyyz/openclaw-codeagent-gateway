//! Gateway configuration parsing.

use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

// ── Default value functions ──────────────────────────────────────────

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    8001
}
fn default_shutdown_timeout() -> u64 {
    30
}
fn default_request_timeout() -> u64 {
    300
}
fn default_true() -> bool {
    true
}
fn default_working_dir() -> String {
    ".".into()
}
fn default_max_processes() -> usize {
    8
}
fn default_max_per_agent() -> usize {
    4
}
fn default_idle_timeout() -> u64 {
    600
}
fn default_watchdog_interval() -> u64 {
    10
}
fn default_stuck_timeout() -> u64 {
    900
}
fn default_store_path() -> String {
    "data/gateway.db".into()
}
fn default_retention() -> u64 {
    86400 * 7 // 7 days
}
fn default_retry_max() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    5
}
fn default_log_level() -> String {
    "info".into()
}
fn default_log_format() -> String {
    "json".into()
}
fn default_burst() -> u32 {
    10
}
fn default_workspace() -> String {
    "/tmp/agw-workspaces".into()
}
fn default_max_sessions() -> usize {
    5
}
fn default_max_jobs() -> usize {
    10
}
fn default_max_prompt() -> usize {
    32_768
}
fn default_session_ttl() -> u64 {
    24
}

// ── Config structs ───────────────────────────────────────────────────

/// Top-level gateway configuration.
#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub agents: HashMap<String, AgentConfig>,
    pub pool: PoolConfig,
    pub store: StoreConfig,
    #[serde(default)]
    pub callback: CallbackConfig,
    #[serde(default)]
    pub observability: ObservabilityConfig,
    #[serde(default)]
    pub gateway: GatewaySecurityConfig,
    pub tenants: HashMap<String, TenantConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_secs: u64,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,
}

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub mode: String,
    pub command: String,
    #[serde(default)]
    pub acp_args: Vec<String>,
    #[serde(default)]
    pub pty_args: Vec<String>,
    #[serde(default = "default_working_dir")]
    pub working_dir: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct PoolConfig {
    #[serde(default = "default_max_processes")]
    pub max_processes: usize,
    #[serde(default = "default_max_per_agent")]
    pub max_per_agent: usize,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: u64,
    #[serde(default = "default_watchdog_interval")]
    pub watchdog_interval_secs: u64,
    #[serde(default = "default_stuck_timeout")]
    pub stuck_timeout_secs: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct StoreConfig {
    #[serde(default = "default_store_path")]
    pub path: String,
    #[serde(default = "default_retention")]
    pub job_retention_secs: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct CallbackConfig {
    #[serde(default)]
    pub default_url: String,
    #[serde(default)]
    pub default_token: String,
    #[serde(default = "default_retry_max")]
    pub retry_max: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_base_delay_secs: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct ObservabilityConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub metrics_enabled: bool,
    #[serde(default)]
    pub audit_path: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct GatewaySecurityConfig {
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    #[serde(default = "default_burst")]
    pub burst: u32,
}

#[derive(Debug, Deserialize)]
pub struct TenantConfig {
    pub credentials: Vec<CredentialConfig>,
    pub policy: TenantPolicyConfig,
}

#[derive(Debug, Deserialize)]
pub struct CredentialConfig {
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct TenantPolicyConfig {
    pub agents: AgentPolicyConfig,
    pub operations: OperationPolicyConfig,
    #[serde(default)]
    pub resources: ResourcePolicyConfig,
    #[serde(default)]
    pub quotas: QuotaPolicyConfig,
    #[serde(default)]
    pub callbacks: CallbackPolicyConfig,
}

#[derive(Debug, Deserialize)]
pub struct AgentPolicyConfig {
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct OperationPolicyConfig {
    #[serde(default = "default_true")]
    pub sync_call: bool,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub async_jobs: bool,
    #[serde(default)]
    pub session_manage: bool,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct ResourcePolicyConfig {
    #[serde(default = "default_workspace")]
    pub workspace: String,
    #[serde(default)]
    pub env_inject: HashMap<String, String>,
    #[serde(default)]
    pub env_deny: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct QuotaPolicyConfig {
    #[serde(default = "default_max_sessions")]
    pub max_concurrent_sessions: usize,
    #[serde(default = "default_max_jobs")]
    pub max_concurrent_jobs: usize,
    #[serde(default = "default_max_prompt")]
    pub max_prompt_length: usize,
    #[serde(default = "default_session_ttl")]
    pub session_ttl_hours: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct CallbackPolicyConfig {
    #[serde(default)]
    pub allowed_urls: Vec<String>,
    #[serde(default)]
    pub allowed_channels: Vec<ChannelRuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelRuleConfig {
    pub channel: String,
    pub targets: Vec<String>,
}

// ── Public functions ─────────────────────────────────────────────────

/// Expand `${ENV_VAR}` references in a string.
pub fn expand_env_vars(input: &str) -> String {
    let re = Regex::new(r"\$\{(\w+)\}").expect("invalid regex");
    re.replace_all(input, |caps: &regex::Captures| {
        std::env::var(&caps[1]).unwrap_or_default()
    })
    .to_string()
}

/// Load and parse a gateway configuration file.
pub fn load_config(path: &str) -> anyhow::Result<GatewayConfig> {
    if !Path::new(path).exists() {
        anyhow::bail!("config file '{}' not found", path);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read config file '{}': {}", path, e))?;
    let expanded = expand_env_vars(&raw);
    let config: GatewayConfig = serde_yaml::from_str(&expanded)
        .map_err(|e| anyhow::anyhow!("failed to parse config: {}", e))?;
    validate_config(&config)?;
    Ok(config)
}

/// Validate config constraints.
pub fn validate_config(config: &GatewayConfig) -> anyhow::Result<()> {
    if config.tenants.is_empty() {
        anyhow::bail!("at least one tenant must be configured");
    }

    let enabled_agents: Vec<&String> = config
        .agents
        .iter()
        .filter(|(_, a)| a.enabled)
        .map(|(name, _)| name)
        .collect();

    if enabled_agents.is_empty() {
        anyhow::bail!("at least one agent must be enabled");
    }

    for (name, agent) in &config.agents {
        if agent.enabled && agent.mode != "acp" && agent.mode != "pty" {
            anyhow::bail!(
                "agent '{}' has invalid mode '{}', must be 'acp' or 'pty'",
                name,
                agent.mode
            );
        }
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal YAML that satisfies all required fields.
    fn minimal_yaml() -> &'static str {
        r#"
server: {}
agents:
  test-agent:
    mode: acp
    command: /usr/bin/test-agent
pool: {}
store: {}
tenants:
  default:
    credentials:
      - token: "tok_test"
    policy:
      agents:
        allow: ["test-agent"]
      operations:
        sync_call: true
"#
    }

    /// Full YAML exercising every section.
    fn full_yaml() -> String {
        r##"
server:
  host: "127.0.0.1"
  port: 9000
  shutdown_timeout_secs: 15
  request_timeout_secs: 120
agents:
  kiro:
    enabled: true
    mode: acp
    command: /usr/local/bin/kiro
    acp_args: ["--stdio"]
    working_dir: /tmp/kiro
    description: "AWS Kiro agent"
    env:
      AWS_REGION: us-east-1
  codex:
    enabled: false
    mode: pty
    command: codex
    pty_args: ["--quiet"]
pool:
  max_processes: 16
  max_per_agent: 4
  idle_timeout_secs: 300
  watchdog_interval_secs: 5
  stuck_timeout_secs: 600
store:
  path: "data/test.db"
  job_retention_secs: 3600
callback:
  default_url: "https://hooks.example.com/gateway"
  default_token: "cb_secret"
  retry_max: 5
  retry_base_delay_secs: 10
observability:
  log_level: debug
  log_format: text
  metrics_enabled: true
  audit_path: "/var/log/agw/audit.log"
gateway:
  allowed_ips: ["10.0.0.0/8"]
  rate_limit:
    requests_per_minute: 600
    burst: 20
tenants:
  ops-team:
    credentials:
      - token: "tok_ops_primary"
      - token: "tok_ops_secondary"
    policy:
      agents:
        allow: ["*"]
      operations:
        sync_call: true
        stream: true
        async_jobs: true
        session_manage: true
        admin: true
      resources:
        workspace: "/data/ops"
        env_inject:
          TEAM: ops
        env_deny: ["AWS_SECRET_ACCESS_KEY"]
      quotas:
        max_concurrent_sessions: 20
        max_concurrent_jobs: 50
        max_prompt_length: 65536
        session_ttl_hours: 48
      callbacks:
        allowed_urls: ["https://hooks.example.com/*"]
        allowed_channels:
          - channel: slack
            targets: ["#ops-alerts"]
  dev-team:
    credentials:
      - token: "tok_dev"
    policy:
      agents:
        allow: ["kiro"]
        deny: ["codex"]
      operations:
        sync_call: true
"##
        .to_string()
    }

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("AGW_TEST_VAR", "hello_world");
        let input = "token: ${AGW_TEST_VAR}, missing: ${AGW_NONEXISTENT_12345}";
        let result = expand_env_vars(input);
        assert_eq!(result, "token: hello_world, missing: ");
        std::env::remove_var("AGW_TEST_VAR");
    }

    #[test]
    fn test_expand_env_vars_no_vars() {
        let input = "plain text without variables";
        assert_eq!(expand_env_vars(input), input);
    }

    #[test]
    fn test_parse_minimal_config() {
        let cfg: GatewayConfig = serde_yaml::from_str(minimal_yaml()).unwrap();

        // Server defaults
        assert_eq!(cfg.server.host, "0.0.0.0");
        assert_eq!(cfg.server.port, 8001);
        assert_eq!(cfg.server.shutdown_timeout_secs, 30);
        assert_eq!(cfg.server.request_timeout_secs, 300);

        // Agent
        assert_eq!(cfg.agents.len(), 1);
        let agent = &cfg.agents["test-agent"];
        assert!(agent.enabled);
        assert_eq!(agent.mode, "acp");

        // Pool defaults
        assert_eq!(cfg.pool.max_processes, 8);
        assert_eq!(cfg.pool.max_per_agent, 4);

        // Store defaults
        assert_eq!(cfg.store.path, "data/gateway.db");

        // Tenant
        assert_eq!(cfg.tenants.len(), 1);
        assert!(cfg.tenants.contains_key("default"));

        // Validation passes
        validate_config(&cfg).unwrap();
    }

    #[test]
    fn test_parse_full_config() {
        let yaml = full_yaml();
        let cfg: GatewayConfig = serde_yaml::from_str(&yaml).unwrap();

        // Server overrides
        assert_eq!(cfg.server.host, "127.0.0.1");
        assert_eq!(cfg.server.port, 9000);

        // Agents
        assert_eq!(cfg.agents.len(), 2);
        assert!(cfg.agents["kiro"].enabled);
        assert!(!cfg.agents["codex"].enabled);
        assert_eq!(cfg.agents["kiro"].mode, "acp");
        assert_eq!(cfg.agents["codex"].mode, "pty");
        assert_eq!(cfg.agents["kiro"].env["AWS_REGION"], "us-east-1");

        // Pool
        assert_eq!(cfg.pool.max_processes, 16);

        // Store
        assert_eq!(cfg.store.path, "data/test.db");
        assert_eq!(cfg.store.job_retention_secs, 3600);

        // Callback
        assert_eq!(cfg.callback.retry_max, 5);

        // Observability
        assert_eq!(cfg.observability.log_level, "debug");
        assert!(cfg.observability.metrics_enabled);

        // Gateway security
        assert_eq!(cfg.gateway.allowed_ips, vec!["10.0.0.0/8"]);
        let rl = cfg.gateway.rate_limit.as_ref().unwrap();
        assert_eq!(rl.requests_per_minute, 600);
        assert_eq!(rl.burst, 20);

        // Tenants
        assert_eq!(cfg.tenants.len(), 2);
        let ops = &cfg.tenants["ops-team"];
        assert_eq!(ops.credentials.len(), 2);
        assert!(ops.policy.operations.admin);
        assert_eq!(ops.policy.quotas.max_concurrent_sessions, 20);

        let dev = &cfg.tenants["dev-team"];
        assert_eq!(dev.policy.agents.deny, vec!["codex"]);

        validate_config(&cfg).unwrap();
    }

    #[test]
    fn test_validation_no_tenants() {
        let yaml = r#"
server: {}
agents:
  a:
    mode: acp
    command: /bin/a
pool: {}
store: {}
tenants: {}
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("at least one tenant"));
    }

    #[test]
    fn test_validation_no_enabled_agents() {
        let yaml = r#"
server: {}
agents:
  disabled-agent:
    enabled: false
    mode: acp
    command: /bin/nope
pool: {}
store: {}
tenants:
  t:
    credentials:
      - token: "x"
    policy:
      agents:
        allow: ["*"]
      operations: {}
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("at least one agent must be enabled"));
    }

    #[test]
    fn test_validation_invalid_mode() {
        let yaml = r#"
server: {}
agents:
  bad:
    enabled: true
    mode: xyz
    command: /bin/bad
pool: {}
store: {}
tenants:
  t:
    credentials:
      - token: "x"
    policy:
      agents:
        allow: ["bad"]
      operations: {}
"#;
        let cfg: GatewayConfig = serde_yaml::from_str(yaml).unwrap();
        let err = validate_config(&cfg).unwrap_err();
        assert!(err.to_string().contains("invalid mode 'xyz'"));
    }

    #[test]
    fn test_load_config_missing_file() {
        let err = load_config("/nonexistent/path/gateway.yaml").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
