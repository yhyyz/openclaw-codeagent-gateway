# Agent Gateway — Configuration Reference

Complete reference for `gateway.yaml`, the configuration file for Agent Gateway (`agw`).

---

## Installation

```bash
# Build from source
source $HOME/.cargo/env
cargo build --release

# Binary is at target/release/agw
cp target/release/agw /usr/local/bin/

# Create config from template
cp gateway.yaml.example gateway.yaml
```

## Starting the gateway

```bash
agw serve --config gateway.yaml
```

### CLI options

| Flag | Description | Default |
|------|-------------|---------|
| `--config <path>` | Path to YAML config file | `gateway.yaml` |
| `--host <addr>` | Override `server.host` | from config |
| `--port <port>` | Override `server.port` | from config |
| `--verbose` | Force log level to `debug` | off |

CLI flags override the corresponding YAML values.

---

## Environment variable expansion

All string values in `gateway.yaml` support `${VAR_NAME}` syntax. Before YAML parsing, the gateway replaces every `${...}` occurrence with the corresponding environment variable value. Undefined variables resolve to an empty string.

```yaml
env:
  ANTHROPIC_API_KEY: "${ANTHROPIC_API_KEY}"   # expanded at load time
  STATIC_VALUE: "literal-string"               # no expansion
```

---

## Full schema reference

### `server` — HTTP server settings

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `host` | string | `"0.0.0.0"` | Listen address |
| `port` | integer | `8001` | Listen port |
| `shutdown_timeout_secs` | integer | `30` | Graceful shutdown timeout (seconds) |
| `request_timeout_secs` | integer | `300` | Per-request timeout (seconds) |

```yaml
server:
  host: "0.0.0.0"
  port: 8001
  shutdown_timeout_secs: 30
  request_timeout_secs: 300
```

---

### `agents` — Agent definitions

A map of agent name → agent configuration. At least one agent must have `enabled: true`.

| Field | Type | Default | Required | Description |
|-------|------|---------|----------|-------------|
| `enabled` | boolean | `true` | no | Whether this agent is active |
| `mode` | string | — | **yes** | `"acp"` or `"pty"` |
| `command` | string | — | **yes** | Path to the agent executable |
| `acp_args` | list of string | `[]` | no | Arguments for ACP mode |
| `pty_args` | list of string | `[]` | no | Arguments for PTY mode |
| `working_dir` | string | `"."` | no | Working directory for the agent process |
| `description` | string | `""` | no | Human-readable description |
| `env` | map of string → string | `{}` | no | Environment variables injected into the agent process |

#### ACP mode vs PTY mode

| Aspect | ACP (`"acp"`) | PTY (`"pty"`) |
|--------|---------------|---------------|
| Process lifecycle | Long-running, managed by process pool | One-shot per invocation |
| Communication | JSON-RPC over stdin/stdout | Prompt passed as CLI argument, stdout captured |
| Session support | Yes — process reused across calls with same session_id | No — each call is independent |
| Arguments field | `acp_args` | `pty_args` |
| Output processing | JSON-RPC response parsing | ANSI escape code stripping |
| Status | Production | Experimental |

```yaml
agents:
  kiro:
    enabled: true
    mode: acp
    command: /usr/local/bin/kiro
    acp_args: ["--stdio"]
    working_dir: /tmp/agw/kiro
    description: "AWS Kiro — cloud-native coding agent"
    env:
      AWS_REGION: us-east-1

  codex:
    enabled: false
    mode: pty
    command: codex
    pty_args: ["--quiet", "--approval-mode", "full-auto"]
    working_dir: /tmp/agw/codex
    description: "OpenAI Codex CLI (PTY mode, experimental)"
    env:
      OPENAI_API_KEY: "${OPENAI_API_KEY}"
```

---

### `pool` — Process pool settings

Controls the pool of long-running ACP agent processes.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_processes` | integer | `8` | Global maximum active processes |
| `max_per_agent` | integer | `4` | Maximum processes per agent type |
| `idle_timeout_secs` | integer | `600` | Reclaim idle processes after this many seconds |
| `watchdog_interval_secs` | integer | `10` | Health check loop interval (seconds) |
| `stuck_timeout_secs` | integer | `900` | Force-fail jobs running longer than this (seconds) |

```yaml
pool:
  max_processes: 8
  max_per_agent: 4
  idle_timeout_secs: 600
  watchdog_interval_secs: 10
  stuck_timeout_secs: 900
```

---

### `store` — Persistent storage

SQLite database for job records.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `path` | string | `"data/gateway.db"` | SQLite database file path |
| `job_retention_secs` | integer | `604800` (7 days) | How long to keep completed job records |

```yaml
store:
  path: "data/gateway.db"
  job_retention_secs: 604800
```

---

### `callback` — Webhook / callback settings

Global defaults for job completion callbacks. Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default_url` | string | `""` | Default callback URL (if job doesn't specify one) |
| `default_token` | string | `""` | Default auth token for callback requests |
| `retry_max` | integer | `3` | Maximum delivery retry attempts |
| `retry_base_delay_secs` | integer | `5` | Base delay between retries (exponential backoff) |

```yaml
callback:
  default_url: ""
  default_token: "${CALLBACK_DEFAULT_TOKEN}"
  retry_max: 3
  retry_base_delay_secs: 5
```

---

### `observability` — Logging and metrics

Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `log_level` | string | `"info"` | `trace`, `debug`, `info`, `warn`, `error` |
| `log_format` | string | `"json"` | `json` or `text` |
| `metrics_enabled` | boolean | `false` | Enable metrics collection |
| `audit_path` | string | `""` | Audit log file path (empty = disabled) |

```yaml
observability:
  log_level: info
  log_format: json
  metrics_enabled: false
  audit_path: ""
```

---

### `gateway` — Network security

Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allowed_ips` | list of string | `[]` | IP allowlist (CIDR format). Empty = allow all |
| `rate_limit` | object | `null` | Global rate limiting |

#### `rate_limit` sub-section

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `requests_per_minute` | integer | — | **Required** if `rate_limit` is set |
| `burst` | integer | `10` | Burst capacity above the per-minute rate |

```yaml
gateway:
  allowed_ips: []
  rate_limit:
    requests_per_minute: 300
    burst: 10
```

---

### `tenants` — Multi-tenant configuration

A map of tenant ID → tenant configuration. **At least one tenant is required.**

Each tenant has credentials (Bearer tokens) and a 5-dimension policy.

#### Tenant structure

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `credentials` | list of credential | **yes** | One or more auth tokens |
| `policy` | object | **yes** | 5-dimension access policy |

#### `credentials[*]`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `token` | string | **yes** | Bearer token (supports `${ENV_VAR}`) |

A tenant can have multiple tokens (e.g., primary + backup). All tokens map to the same tenant identity.

---

#### `policy.agents` — Dimension 1: Agent access

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allow` | list of string | — | **Required.** Allowed agent names. `"*"` = all agents |
| `deny` | list of string | `[]` | Denied agent names (takes priority over allow) |

Matching: deny is checked first → then allow. Supports `"*"` (match all) and `"prefix*"` (prefix match).

---

#### `policy.operations` — Dimension 2: Operation permissions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `sync_call` | boolean | `true` | Allow `POST /runs` |
| `stream` | boolean | `false` | Allow streaming responses (reserved) |
| `async_jobs` | boolean | `false` | Allow `POST /jobs` |
| `session_manage` | boolean | `false` | Allow `DELETE /sessions/...` |
| `admin` | boolean | `false` | Allow `/admin/*` endpoints |

---

#### `policy.resources` — Dimension 3: Resource isolation

Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `workspace` | string | `"/tmp/agw-workspaces"` | Tenant's workspace root directory |
| `env_inject` | map of string → string | `{}` | Extra env vars injected into agent processes |
| `env_deny` | list of string | `[]` | Env var names blocked from reaching agents |

`env_deny` filters both `env_inject` values and the agent's own `env` configuration.

---

#### `policy.quotas` — Dimension 4: Rate and resource limits

Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_concurrent_sessions` | integer | `5` | Max active sessions for this tenant |
| `max_concurrent_jobs` | integer | `10` | Max active async jobs |
| `max_prompt_length` | integer | `32768` | Max prompt length in characters |
| `session_ttl_hours` | integer | `24` | Session time-to-live (hours) |

Quotas are enforced at runtime via atomic counters with RAII guards.

---

#### `policy.callbacks` — Dimension 5: Callback restrictions

Optional section.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `allowed_urls` | list of string | `[]` | Allowed callback URL patterns (`*` suffix wildcard) |
| `allowed_channels` | list of channel rule | `[]` | Allowed notification channels |

#### Channel rule

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `channel` | string | **yes** | Platform name (e.g., `"slack"`) |
| `targets` | list of string | **yes** | Allowed targets (e.g., `"#ops-alerts"`, `"user:*"`) |

---

### Complete tenant example

```yaml
tenants:
  ops-team:
    credentials:
      - token: "${OPS_TEAM_TOKEN_PRIMARY}"
      - token: "${OPS_TEAM_TOKEN_SECONDARY}"
    policy:
      agents:
        allow: ["*"]
        deny: []
      operations:
        sync_call: true
        stream: true
        async_jobs: true
        session_manage: true
        admin: true
      resources:
        workspace: "/data/workspaces/ops"
        env_inject:
          TEAM: ops
          ENV: production
        env_deny: ["AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN"]
      quotas:
        max_concurrent_sessions: 20
        max_concurrent_jobs: 50
        max_prompt_length: 65536
        session_ttl_hours: 48
      callbacks:
        allowed_urls: ["https://hooks.example.com/*"]
        allowed_channels:
          - channel: slack
            targets: ["#ops-alerts", "#ops-logs"]

  dev-team:
    credentials:
      - token: "${DEV_TEAM_TOKEN}"
    policy:
      agents:
        allow: ["kiro", "claude", "opencode"]
        deny: ["codex"]
      operations:
        sync_call: true
        stream: true
        async_jobs: false
        session_manage: true
        admin: false
      resources:
        workspace: "/data/workspaces/dev"
        env_inject:
          TEAM: dev
        env_deny: ["AWS_SECRET_ACCESS_KEY"]
      quotas:
        max_concurrent_sessions: 5
        max_concurrent_jobs: 10
        max_prompt_length: 32768
        session_ttl_hours: 24
      callbacks:
        allowed_urls: []
        allowed_channels: []
```

---

## Validation rules

The gateway validates the config at startup and refuses to start if:

1. `tenants` is empty — at least one tenant must be configured
2. No agent has `enabled: true` — at least one active agent is required
3. Any enabled agent has a `mode` other than `"acp"` or `"pty"`

---

## Config loading flow

```
gateway.yaml
  → read raw text
  → expand ${VAR} references from environment
  → YAML deserialize into GatewayConfig
  → validate_config() checks invariants
  → CLI --host/--port/--verbose overrides applied
  → TenantRegistry built (token → tenant index)
  → ProcessPool, QuotaTracker, JobStore initialized
  → AppState assembled (Arc-shared across handlers)
```

---

## Security best practices

### Token management

- **Never hardcode tokens** in `gateway.yaml`. Always use `${ENV_VAR}` syntax.
- Rotate tokens by updating the environment variable and restarting the gateway.
- Each tenant can have multiple tokens for zero-downtime rotation.

### Environment variable filtering

- Use `env_deny` to prevent sensitive variables from reaching agent processes:
  ```yaml
  env_deny: ["AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "DATABASE_URL"]
  ```
- `env_deny` applies to both `resources.env_inject` and agent-level `env` values.

### Workspace isolation

- Each tenant should have a separate `resources.workspace` directory.
- Agent processes run in the configured `working_dir`, scoped per agent.

### Network security

- Use `gateway.allowed_ips` to restrict access by source IP.
- Place the gateway behind a reverse proxy (nginx, ALB) for TLS termination.
- Set `rate_limit` to prevent abuse.

### Callback security

- Restrict `callbacks.allowed_urls` to known webhook endpoints.
- Use `allowed_channels` to limit which platforms and targets each tenant can notify.

---

## Systemd service file

```ini
[Unit]
Description=Agent Gateway (agw)
After=network.target

[Service]
Type=simple
User=agw
Group=agw
WorkingDirectory=/opt/agw
ExecStart=/usr/local/bin/agw serve --config /opt/agw/gateway.yaml
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Load secrets from environment file
EnvironmentFile=/opt/agw/.env

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/opt/agw/data /tmp/agw

[Install]
WantedBy=multi-user.target
```

The `.env` file should contain all `${VAR}` references:

```bash
OPS_TEAM_TOKEN_PRIMARY=tok-xxxx
DEV_TEAM_TOKEN=tok-yyyy
ANTHROPIC_API_KEY=sk-ant-xxxx
OPENAI_API_KEY=sk-xxxx
CALLBACK_DEFAULT_TOKEN=cb-xxxx
```

---

## Example: Minimal configuration

The smallest valid config — one agent, one tenant:

```yaml
server:
  port: 8001

agents:
  claude:
    enabled: true
    mode: acp
    command: /usr/local/bin/claude
    acp_args: ["--stdio", "--no-browser"]

pool:
  max_processes: 4

store:
  path: "data/gateway.db"

tenants:
  default:
    credentials:
      - token: "${AGW_TOKEN}"
    policy:
      agents:
        allow: ["*"]
      operations:
        sync_call: true
```

---

## Example: Full production configuration

See `gateway.yaml.example` in the repository root for a complete production-ready template with all sections populated and commented.
