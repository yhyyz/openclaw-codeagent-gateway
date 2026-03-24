---
name: agent-gateway
description: "Interact with a running Agent Gateway (agw) — submit async jobs to AI coding agents (Claude, OpenCode, Kiro) via HTTP with fire-and-forget pattern and webhook callbacks. Use when the user mentions: agent gateway, agw, submit job, remote agent, agent-to-agent, check job status, gateway health, manage sessions, list agents, or webhook callback."
---

# Agent Gateway Skill

Submit async jobs to AI coding agents (Claude, OpenCode, Kiro) via an HTTP gateway with automatic webhook delivery.

## CRITICAL: Fire-and-forget pattern

1. Submit job via `POST /jobs` with callback
2. Tell the user the `job_id` — results will arrive automatically
3. **END YOUR TURN IMMEDIATELY** — do NOT poll, do NOT wait

The gateway pushes results to this chat via webhook when the job completes.

**CORRECT:**
```
POST /jobs → get job_id → reply "✅ Submitted (job: abc123). Results arrive automatically." → DONE
```

**WRONG (blocks the session):**
```
POST /jobs → sleep → GET /jobs/{id} → sleep → GET /jobs/{id} → ...
```

Only use `GET /jobs/{id}` when the user explicitly asks "what's the status of job X?"

## Environment (pre-configured)

```bash
AGW_URL=http://127.0.0.1:8001
AGW_TOKEN=agw-local-token-2024
```

## Quick start

### 1. Health check

```bash
curl -sf "$AGW_URL/health" | jq .
```

### 2. List agents

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/agents" | jq .
```

### 3. Submit a job (with callback — REQUIRED)

```bash
curl -sf -X POST "$AGW_URL/jobs" \
  -H "Authorization: Bearer $AGW_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "agent": "claude",
    "prompt": "Analyze the auth module and suggest improvements",
    "progress_notify": true,
    "callback": {
      "channel": "telegram",
      "target": "tg:1704924315",
      "account_id": "default"
    }
  }' | jq .
```

Response: `202 Accepted` with `job_id`, `status: "pending"`, `session_id`.

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `agent` | yes | — | Agent name: `claude`, `opencode`, `kiro` |
| `prompt` | yes | — | Task description |
| `callback` | yes* | — | Webhook routing (*without it, results are lost) |
| `session_id` | no | auto-generated | Reuse for multi-turn conversations |
| `progress_notify` | no | `true` | `false` for silent mode (only final result delivered) |

### Callback format

```json
{
  "channel": "telegram",
  "target": "tg:1704924315",
  "account_id": "default"
}
```

The gateway POSTs results to `http://127.0.0.1:18789/tools/invoke` using OpenClaw format:

```json
{
  "tool": "message",
  "args": {
    "action": "send",
    "channel": "telegram",
    "target": "tg:1704924315",
    "message": "..."
  },
  "sessionKey": "main"
}
```

## Auto-notification

- **Progress**: Tool starts, plans, and intermediate updates pushed to chat during execution (when `progress_notify: true`)
- **Final result**: Full result + tools used + token usage + cost pushed automatically on completion
- **Errors**: Failures also delivered via callback with error details

## Check job status (only when user explicitly asks)

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/jobs/<job_id>" | jq .
```

Status values: `pending` → `running` → `completed` / `failed` / `interrupted`

### List all jobs

```bash
curl -sf -H "Authorization: Bearer $AGW_TOKEN" "$AGW_URL/jobs" | jq .
```

## Session management

- **Multi-turn**: Provide the same `session_id` across jobs to maintain conversation context
- **Isolation**: Different session IDs → isolated agent processes
- **Auto-rebuild**: If an agent crashes, gateway rebuilds (context lost, user notified)

### Close a session

```bash
curl -sf -X DELETE -H "Authorization: Bearer $AGW_TOKEN" \
  "$AGW_URL/sessions/<agent>/<session_id>" | jq .
```

## Administration

Requires `operations.admin: true` in tenant policy.

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Gateway health (no auth) |
| `GET /health/agents` | Agent health check |
| `GET /admin/tenants` | List all tenants |
| `GET /admin/pool` | Process pool status |

## Agent notes

| Agent | ACP Command | Token reporting | Notes |
|-------|------------|-----------------|-------|
| claude | `npx -y @zed-industries/claude-agent-acp` | Full (input/output/cache read/cache write) | |
| opencode | `opencode acp` | Total only | |
| kiro | `kiro-cli acp -a` | None | Slow startup (~19s), needs `-a` for auto-accept permissions |

## Using agw-client.sh

```bash
AGW_CLIENT="<skill-dir>/scripts/agw-client.sh"

$AGW_CLIENT -l                              # List agents
$AGW_CLIENT "Refactor the auth module"      # Submit job (default agent)
$AGW_CLIENT -a claude "Analyze code"        # Specify agent
$AGW_CLIENT --job-status <job_id>           # Query job
$AGW_CLIENT -s <uuid> "Continue from here"  # Multi-turn session
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `401 unauthorized` | Bad token | Check `tenants.*.credentials` in gateway.yaml |
| `403 agent not allowed` | Agent not in allow list | Add to `policy.agents.allow` |
| `403 admin required` | No admin permission | Set `operations.admin: true` |
| `429 quota exceeded` | Hit concurrent limit | Increase `quotas.max_concurrent_*` |
| `404 agent not found` | Agent disabled or missing | Set `agents.*.enabled: true` |
| `503 pool exhausted` | No capacity for agent | Wait or increase `pool.max_per_agent` |
| Health timeout | Gateway not running | Check `systemctl status agw` |
| Job stuck running | Agent hung | Auto-fails after `stuck_timeout_secs` |
| No callback received | Missing callback field | Always include `callback` in request |

## Security

- Never display tokens in plaintext — use `AGW_TOKEN` env var
- Gateway filters env vars per tenant (`env_deny` blocks secrets)
- Each tenant gets an isolated workspace directory
- Cross-tenant job isolation enforced
