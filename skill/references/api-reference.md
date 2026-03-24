# Agent Gateway — API Reference

Base URL: `http://<host>:8001` (configurable via `server.host` / `server.port`)

All authenticated endpoints require `Authorization: Bearer <token>` header.
All error responses use the format `{"error": "<message>"}`.

---

## Route overview

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET` | `/health` | No | Gateway health check |
| `GET` | `/health/agents` | Bearer | Agent process health |
| `GET` | `/agents` | Bearer | List available agents |
| `POST` | `/runs` | Bearer | Synchronous agent call |
| `POST` | `/jobs` | Bearer | Submit async job |
| `GET` | `/jobs` | Bearer | List tenant's jobs |
| `GET` | `/jobs/{job_id}` | Bearer | Get job details |
| `DELETE` | `/sessions/{agent}/{session_id}` | Bearer | Close a session |
| `GET` | `/admin/tenants` | Bearer + admin | List tenants |
| `GET` | `/admin/pool` | Bearer + admin | Process pool stats |

---

## `GET /health`

Gateway liveness check. No authentication required.

### Response `200 OK`

```json
{
  "status": "ok",
  "version": "0.1.0",
  "uptime_secs": 12345
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | Always `"ok"` |
| `version` | string | Gateway version from Cargo.toml |
| `uptime_secs` | integer | Seconds since server start |

### Example

```bash
curl -sf "$AGW_URL/health" | jq .
```

---

## `GET /health/agents`

Process pool health — shows active agent process counts.

### Auth

Bearer token required.

### Response `200 OK`

```json
{
  "agents": {
    "total": 5,
    "by_agent": [
      ["kiro", 3],
      ["claude", 2]
    ]
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `agents.total` | integer | Total active processes |
| `agents.by_agent` | array of `[string, integer]` | Per-agent process count |

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/health/agents" | jq .
```

### Errors

| Status | Condition |
|--------|-----------|
| `401` | Missing or invalid token |

---

## `GET /agents`

List agents available to the authenticated tenant. Results are filtered by the tenant's `policy.agents.allow` / `deny` lists. Only agents with `enabled: true` are included.

### Auth

Bearer token required.

### Response `200 OK`

```json
{
  "agents": [
    {
      "name": "kiro",
      "mode": "acp",
      "description": "AWS Kiro — cloud-native coding agent"
    },
    {
      "name": "claude",
      "mode": "acp",
      "description": "Anthropic Claude Code"
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `agents[].name` | string | Agent identifier |
| `agents[].mode` | string | `"acp"` or `"pty"` |
| `agents[].description` | string | Human-readable description |

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/agents" | jq '.agents[].name'
```

### Errors

| Status | Condition |
|--------|-----------|
| `401` | Missing or invalid token |

---

## `POST /runs`

Execute a synchronous agent call. Blocks until the agent responds.

### Auth

Bearer token required. Tenant must have `operations.sync_call: true` and the agent must be in the allow list.

### Request body

```json
{
  "agent": "kiro",
  "prompt": "Explain the authentication module",
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "stream": false
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `agent` | string | **yes** | — | Agent name |
| `prompt` | string | **yes** | — | Prompt text |
| `session_id` | string | no | `null` | Session ID for multi-turn context |
| `stream` | boolean | no | `false` | Enable SSE streaming (reserved) |

### Response `200 OK`

```json
{
  "status": "completed",
  "agent": "kiro",
  "message": "agent gateway ready"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `status` | string | `"completed"` |
| `agent` | string | Agent that handled the request |
| `message` | string | Agent response content |

### Example

```bash
curl -sf -X POST "$AGW_URL/runs" \
  -H "Authorization: Bearer $AGW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"agent":"kiro","prompt":"List all files in src/"}' | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `403` | Agent not in allow list | `{"error": "forbidden: agent 'codex' not allowed"}` |
| `404` | Agent not found or disabled | `{"error": "agent not found: codex"}` |
| `422` | Prompt exceeds max length | `{"error": "prompt too long: 70000 chars (limit: 65536)"}` |
| `429` | Quota or rate limit exceeded | `{"error": "quota exceeded: max concurrent sessions"}` |
| `504` | Request timeout | `{"error": "timeout after 300s"}` |

---

## `POST /jobs`

Submit an asynchronous job. Returns immediately with a job ID. The gateway executes the job in the background and optionally delivers results via callback.

### Auth

Bearer token required. Tenant must have `operations.async_jobs: true` and the agent must be in the allow list.

### Request body

```json
{
  "agent": "claude",
  "prompt": "Refactor the authentication module for better testability",
  "session_id": "optional-session-uuid",
  "callback": {
    "channel": "slack",
    "target": "#ops-alerts",
    "account_id": "default"
  }
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `agent` | string | **yes** | — | Agent name |
| `prompt` | string | **yes** | — | Task description |
| `session_id` | string | no | auto-generated UUID v4 | Session ID |
| `callback` | object | no | `null` | Callback routing (channel-agnostic) |
| `callback.channel` | string | no | `""` | Message platform identifier |
| `callback.target` | string | no | `""` | Routing destination |
| `callback.account_id` | string | no | `""` | Bot account identifier |

### Response `202 Accepted`

```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "pending",
  "agent": "claude",
  "session_id": "optional-session-uuid"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `job_id` | string | UUID v4 job identifier |
| `status` | string | Always `"pending"` on creation |
| `agent` | string | Agent assigned to the job |
| `session_id` | string | Session ID (provided or auto-generated) |

### Example

```bash
curl -sf -X POST "$AGW_URL/jobs" \
  -H "Authorization: Bearer $AGW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"agent":"claude","prompt":"Write unit tests for src/auth/"}' | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `403` | Agent not allowed | `{"error": "forbidden: agent 'codex' not allowed"}` |
| `403` | Callback URL denied | `{"error": "callback URL denied: http://evil.com"}` |
| `403` | Callback channel denied | `{"error": "callback channel denied: telegram"}` |
| `404` | Agent not found or disabled | `{"error": "agent not found: codex"}` |
| `422` | Prompt exceeds max length | `{"error": "prompt too long: 70000 chars (limit: 65536)"}` |
| `429` | Quota exceeded | `{"error": "quota exceeded: max concurrent jobs"}` |

---

## `GET /jobs`

List all jobs belonging to the authenticated tenant. Cross-tenant isolation is enforced — you only see your own jobs.

### Auth

Bearer token required.

### Response `200 OK`

```json
{
  "jobs": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "agent": "claude",
      "status": "completed",
      "created_at": 1711234567,
      "completed_at": 1711234600
    },
    {
      "id": "660f9500-f39c-52e5-b827-557766551111",
      "agent": "kiro",
      "status": "running",
      "created_at": 1711234800,
      "completed_at": 0
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `jobs[].id` | string | Job UUID |
| `jobs[].agent` | string | Agent name |
| `jobs[].status` | string | `pending` / `running` / `completed` / `failed` / `interrupted` |
| `jobs[].created_at` | integer | Unix timestamp (seconds) |
| `jobs[].completed_at` | integer | Unix timestamp, `0` if not yet completed |

Returns at most 100 jobs, ordered by `created_at` descending.

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/jobs" | jq '.jobs[] | {id, status}'
```

### Errors

| Status | Condition |
|--------|-----------|
| `401` | Missing or invalid token |

---

## `GET /jobs/{job_id}`

Get detailed status of a single job. Returns `404` for jobs belonging to other tenants (not `403`, to prevent tenant enumeration).

### Auth

Bearer token required. Job must belong to the authenticated tenant.

### Path parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `job_id` | string | Job UUID |

### Response `200 OK`

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "agent": "claude",
  "session_id": "sess-abc-123",
  "status": "completed",
  "result": "Refactored auth module: extracted trait, added 12 unit tests...",
  "error": "",
  "tools": ["read_file", "write_file", "run_command"],
  "created_at": 1711234567,
  "completed_at": 1711234600,
  "duration_secs": 33.0
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Job UUID |
| `agent` | string | Agent name |
| `session_id` | string | Session ID used |
| `status` | string | `pending` / `running` / `completed` / `failed` / `interrupted` |
| `result` | string | Agent output (populated on completion) |
| `error` | string | Error message (populated on failure) |
| `tools` | array of string | Tools the agent invoked |
| `created_at` | integer | Unix timestamp |
| `completed_at` | integer | Unix timestamp, `0` if not yet completed |
| `duration_secs` | float | Execution duration in seconds |

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" \
  "$AGW_URL/jobs/550e8400-e29b-41d4-a716-446655440000" | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `404` | Job not found or belongs to another tenant | `{"error": "job not found: 550e8400-..."}` |

---

## `DELETE /sessions/{agent}/{session_id}`

Close an active session and terminate the associated agent process.

### Auth

Bearer token required. Tenant must have `operations.session_manage: true`.

### Path parameters

| Parameter | Type | Description |
|-----------|------|-------------|
| `agent` | string | Agent name |
| `session_id` | string | Session ID to close |

### Response `200 OK`

```json
{
  "status": "closed",
  "agent": "kiro",
  "session_id": "sess-abc-123"
}
```

### Example

```bash
curl -sf -X DELETE -H "Authorization: Bearer $AGW_TOKEN" \
  "$AGW_URL/sessions/kiro/sess-abc-123" | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `403` | `session_manage` not enabled | `{"error": "forbidden: session_manage required"}` |

---

## `GET /admin/tenants`

List all configured tenant names. Does not expose credentials or policy details.

### Auth

Bearer token required. Tenant must have `operations.admin: true`.

### Response `200 OK`

```json
{
  "tenants": ["ops-team", "dev-team"]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `tenants` | array of string | Tenant identifiers |

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/admin/tenants" | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `403` | Not an admin tenant | `{"error": "forbidden: admin required"}` |

---

## `GET /admin/pool`

Process pool statistics — total active processes and per-agent breakdown.

### Auth

Bearer token required. Tenant must have `operations.admin: true`.

### Response `200 OK`

```json
{
  "total": 5,
  "by_agent": [
    ["kiro", 3],
    ["claude", 2]
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `total` | integer | Total active processes across all agents |
| `by_agent` | array of `[string, integer]` | Per-agent active process count |

### Example

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/admin/pool" | jq .
```

### Errors

| Status | Condition | Example |
|--------|-----------|---------|
| `401` | Missing or invalid token | `{"error": "unauthorized"}` |
| `403` | Not an admin tenant | `{"error": "forbidden: admin required"}` |

---

## Error reference

All errors return JSON: `{"error": "<message>"}`.

| HTTP Status | Error Type | Trigger |
|-------------|-----------|---------|
| `401` | Unauthorized | Missing or invalid Bearer token |
| `403` | Forbidden | Agent not in allow list, operation not permitted, callback denied |
| `404` | Not Found | Agent disabled/missing, job not found or cross-tenant |
| `422` | Unprocessable Entity | Prompt exceeds `max_prompt_length` |
| `429` | Too Many Requests | Quota exceeded, rate limited, process pool exhausted |
| `500` | Internal Server Error | Agent crash, I/O error, database error |
| `504` | Gateway Timeout | Request exceeded `request_timeout_secs` |

### Job status values

| Status | Description |
|--------|-------------|
| `pending` | Job created, waiting for agent process |
| `running` | Agent is actively processing |
| `completed` | Agent finished successfully |
| `failed` | Agent returned an error |
| `interrupted` | Job was cancelled or timed out (`stuck_timeout_secs`) |
