---
name: dcc-rest-gateway
description: >-
  Control live DCC hosts (Maya, Blender, Houdini, Photoshop, 3ds Max, and
  others) through the DCC-MCP gateway REST API only — no MCP client required.
  For any agent host (OpenClaw, ClawHub, Cursor, Claude, custom HTTP clients): inventory
  online instances, obtain user consent before setup when none are registered,
  then search, describe, and call tools via POST /v1/*.
license: MIT-0
compatibility: Requires curl on PATH; DCC-MCP gateway reachable at DCC_MCP_GATEWAY_URL (default http://127.0.0.1:9765)
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "0.17.37"  # x-release-please-version
    search-hint: "rest api, gateway, http, no mcp, instances, dcc control, clawhub"
    tags: "rest, gateway, infrastructure, clawhub, openclaw, instances"
  openclaw:
    requires:
      env:
        - DCC_MCP_GATEWAY_URL
      bins:
        - curl
    primaryEnv: DCC_MCP_GATEWAY_URL
    emoji: "🌐"
    homepage: https://github.com/loonghao/dcc-mcp-core/blob/main/skills/dcc-rest-gateway/SKILL.md
---

# DCC REST Gateway — Agent Control Plane (No MCP)

This skill teaches **any** AI agent to drive DCC software through the **DCC-MCP
gateway HTTP API** only. You do **not** need MCP `tools/list`, `call_tool`,
`resources/read`, or Streamable HTTP — only `curl` (or any HTTP client) against
the elected gateway (default port **9765**).

Agent-facing gateway REST routes return compact TOON by default. The examples in
this skill request `Accept: application/json` so shell agents can keep using
plain JSON tooling; remove that header when the caller can decode TOON.

Install via OpenClaw/ClawHub, or point your agent at this `SKILL.md` after cloning
[`dcc-mcp-core/skills/dcc-rest-gateway/`](https://github.com/loonghao/dcc-mcp-core/tree/main/skills/dcc-rest-gateway).

Full contract: [`docs/guide/rest-api-surface.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/docs/guide/rest-api-surface.md).

---

## CRITICAL RULES (read first)

| Situation | You MUST |
|-----------|----------|
| Starting any DCC task | Run **instance inventory** (below) before `search` / `call` |
| `GET /v1/instances` → `total == 0` | **STOP** — do **not** call `POST /v1/search` or `POST /v1/call` |
| Gateway unreachable (`healthz` fails) | **STOP** — explain; ask user permission before troubleshooting |
| User has **not** agreed to setup | **FORBID** `pip install`, editing env files, launching GUI apps, writing configs |
| User **approved** setup | Follow [`references/ZERO_INSTANCES.md`](references/ZERO_INSTANCES.md); poll instances after each step |
| After DCC crash/restart | Re-run inventory — `instance_id` and `tool_slug` values change |

---

## Configuration

| Variable | Default | Meaning |
|----------|---------|---------|
| `DCC_MCP_GATEWAY_URL` | `http://127.0.0.1:9765` | Gateway root (no trailing slash required) |

Set in the shell or OpenClaw env before running commands:

```bash
export DCC_MCP_GATEWAY_URL="${DCC_MCP_GATEWAY_URL:-http://127.0.0.1:9765}"
GATEWAY="$DCC_MCP_GATEWAY_URL"
```

Quick probe (cross-platform helper — stdlib only, no curl required):

```bash
# Linux / macOS
python3 scripts/check_gateway.py

# Windows (cmd or PowerShell)
py -3 scripts\check_gateway.py
# PowerShell wrapper:
pwsh scripts/check_gateway.ps1

# Optional: bash scripts/check_gateway.sh
```

Flags: `--gateway URL`, `--pretty` for indented JSON.

---

## Step 0 — Mandatory instance inventory

Run **every** time you begin work or after the user starts/stops a DCC host:

```bash
GATEWAY="${DCC_MCP_GATEWAY_URL:-http://127.0.0.1:9765}"

curl -sf "$GATEWAY/v1/healthz" || echo "GATEWAY_UNREACHABLE"
curl -s  "$GATEWAY/v1/readyz"
curl -s  "$GATEWAY/v1/instances"
curl -s  "$GATEWAY/v1/context"    # optional summary
```

### Interpret `GET /v1/instances`

Response shape:

```json
{
  "total": 2,
  "instances": [
    {
      "instance_id": "full-uuid",
      "dcc_type": "maya",
      "status": "available",
      "stale": false,
      "display_name": "...",
      "scene": "...",
      "port": 8765,
      "mcp_url": "http://127.0.0.1:8765/mcp"
    }
  ]
}
```

**Report to the user:**

1. **`total`** — how many registry rows exist (routable targets when not stale).
2. **Count by `dcc_type`** — e.g. `maya: 1`, `blender: 1`.
3. Per row: `instance_id`, `status`, `stale` (ignore or warn on `stale: true`).
4. From `GET /v1/context` (optional): `live_instance_count`, `loaded_skill_count`, `action_count`.

Save one target `instance_id` per DCC you will use — pass it to `POST /v1/search` as `instance_id` (full UUID or unique ≥4-char prefix).

### If `total == 0`

1. Tell the user: gateway may be up but **no DCC has registered** yet.
2. **Do not** call `search`, `describe`, or `call` (they fail or return empty).
3. Ask explicitly: *"May I guide you through starting a DCC adapter for &lt;product&gt;?"*
4. Only after **yes** → open [`references/ZERO_INSTANCES.md`](references/ZERO_INSTANCES.md).
5. After each user action, re-run `GET /v1/instances` until `total >= 1`.

---

## Step 1 — Discover tools (`POST /v1/search`)

Only when `total >= 1` and rows are not stale.

```bash
curl -s -X POST "$GATEWAY/v1/search" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "sphere",
    "dcc_type": "maya",
    "instance_id": "<from-inventory>",
    "limit": 20
  }'
```

Copy `hits[].tool_slug` **verbatim** — format: `<dcc>.<id8>.<backend_tool>` (e.g. `maya.a1b2c3d4.maya_primitives__create_sphere`). **Never** hand-build slugs.

---

## Step 2 — Inspect schema (`POST /v1/describe`)

```bash
curl -s -X POST "$GATEWAY/v1/describe" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{"tool_slug": "<from-search>", "include_schema": true}'
```

Read `tool.inputSchema` and safety annotations before calling.

---

## Step 3 — Invoke (`POST /v1/call`)

```bash
curl -s -X POST "$GATEWAY/v1/call" \
  -H "Accept: application/json" \
  -H "Content-Type: application/json" \
  -d '{
    "tool_slug": "<from-search>",
    "arguments": { "radius": 2.0 }
  }'
```

- Tool-specific fields (`code`, `file_path`, `radius`, …) belong **inside** `arguments`, not at the top level.
- Multi-step (≤25): `POST /v1/call_batch` with `calls[]` and optional `stop_on_error`.

Path-style alternative when you already know `dcc_type` + `instance_id`:

`POST /v1/dcc/{dcc_type}/instances/{instance_id}/call` with body
`{"backend_tool": "...", "arguments": {...}}`.

See [`references/REST_CHEATSHEET.md`](references/REST_CHEATSHEET.md) for the full endpoint list.

---

## What this skill does NOT use

- MCP JSON-RPC (`POST /mcp`, `tools/call`, `resources/read`)
- Gateway MCP wrappers (including hidden `call_tool`) unless your host maps them to REST internally

REST and MCP share the same backend; this skill is for agents that only have HTTP.
