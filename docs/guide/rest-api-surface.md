# REST API Surface

Per-DCC adapter servers and the multi-DCC gateway expose overlapping, but not
identical, `/v1/*` REST surfaces alongside their MCP endpoint. This page is the
integration contract for **traditional callers** (cURL, CI pipelines, studio
automation, non-MCP tooling) — anything that can speak HTTP can drive a DCC
through these routes without touching the MCP protocol stack.

The gateway now publishes a gateway-specific `GET /v1/openapi.json` contract.
It documents only the routes mounted by the gateway router and deliberately
omits per-DCC-only resource, prompt, and job routes. Per-DCC adapter servers
continue to serve their own adapter OpenAPI contract from the same path.

> **Relationship to MCP** — The gateway advertises the same bounded workflow
> over MCP as four tools: `search`, `describe`, `load_skill`, and `call`.
> These tools share service code with `/v1/search`, `/v1/describe`,
> `/v1/load_skill`, `/v1/call`, and `/v1/call_batch`; REST remains the pure
> HTTP twin for clients that do not speak MCP.
> MCP clients can also opt into compact TOON payloads with
> `params._meta.response_format="toon"` or `params._meta.compact=true`.
> The `/mcp` HTTP content type and the outer JSON-RPC `jsonrpc`, `id`,
> `result`, and `error` envelope stay JSON; `Accept: application/toon` is a
> REST-only negotiation mechanism.

---

## Gateway Endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/v1/instances` | Live DCC instance rows known to the elected gateway. |
| `GET` | `/v1/healthz` | Gateway liveness probe. Returns `200 {"ok": true}` when the HTTP handler is up. |
| `GET` | `/v1/readyz` | Gateway readiness summary with per-instance readiness bits; the route stays `200` even when no instance is ready. |
| `GET` | `/v1/skills` | Loaded gateway capability records projected as skill entries. |
| `POST` | `/v1/list_skills` | Forward a skill-list request to a selected backend instance. |
| `POST` | `/v1/search` | Fuzzy / exact search across loaded + unloaded skills. |
| `POST` | `/v1/load_skill` | Load a discovered backend skill without using MCP `tools/call`; gateway default is lazy group activation. |
| `POST` | `/v1/unload_skill` | Unload a backend skill without using MCP `tools/call`. |
| `POST` | `/v1/describe` | Return the full input schema + annotations for one `tool_slug`. |
| `GET` | `/v1/tools/{slug}` | Alias of `/v1/describe` (read-only lookup via URL). |
| `POST` | `/v1/call` | Invoke one gateway capability by slug. This is the canonical gateway invocation plane. |
| `POST` | `/v1/call_batch` | Invoke up to 25 ordered gateway capability calls with optional `stop_on_error`. |
| `GET` | `/v1/context` | Gateway snapshot with live instances and aggregate capability counts. |
| `GET` | `/v1/dcc/{dcc_type}/instances/{instance_id}/describe` | Describe a backend tool on one DCC instance; accepts `backend_tool` / `tool` / `action` query keys. |
| `POST` | `/v1/dcc/{dcc_type}/instances/{instance_id}/call` | Call a backend tool on one DCC instance using `{backend_tool, arguments, meta}`. |
| `POST` | `/v1/dcc/{dcc_type}/instances/{instance_id}/stop` | Safe-stop route for test-owned instances that advertise `safe_stop_url` metadata. |
| `GET` | `/v1/debug/instances` | Gateway only: stable agent-facing instance diagnostics. |
| `GET` | `/v1/debug/activity` | Gateway only: stable activity feed from audits, traces, and gateway events. |
| `GET` | `/v1/debug/traces` | Gateway only: recent dispatch trace list. |
| `GET` | `/v1/debug/traces/{request_id}` | Gateway only: dispatch trace detail by request id. |
| `GET` | `/v1/debug/traffic` | Gateway only: `capture_status` plus retained metadata-only traffic-capture frames from an explicit `admin_live` sink. |
| `GET` | `/v1/debug/traffic/export` | Gateway only: retained metadata-only traffic-capture frames as JSONL. |
| `GET` | `/v1/debug/trace-context/{lookup_id}` | Gateway only: resolve trace id or request id to the primary trace context. |
| `GET` | `/v1/debug/agent-traces/{lookup_id}` | Gateway only: compact public-safe agent trace packet by trace id or request id. |
| `GET` | `/v1/debug/bundles/{request_id}` | Gateway only: full-chain debug bundle by request id or trace id. |
| `GET` | `/v1/debug/issue-reports/{request_id}` | Gateway only: public-safe GitHub issue report JSON by default; `?mode=raw` includes reviewed local evidence. |
| `GET` | `/v1/debug/tasks` | Gateway only: user-level task outcomes grouped across workflows, calls, artifacts, and validation. |
| `GET` | `/v1/debug/calls` | Gateway only: recent audited calls. |
| `GET` | `/v1/debug/logs` | Gateway only: merged gateway events, file logs, and audit summaries. |
| `GET` | `/v1/debug/stats` | Gateway only: aggregated call statistics. |
| `GET` | `/v1/debug/governance` | Gateway only: effective policy, traffic capture, redaction, quota, and recent governance decisions. |
| `GET` | `/v1/debug/health` | Gateway only: debug subsystem health summary. |
| `GET` | `/v1/openapi.json` | Gateway-specific OpenAPI 3.x document for code-gen clients. |
| `GET` | `/docs` | Scalar API reference rendered from the same gateway-specific OpenAPI document. |

Gateway capability slugs use `<dcc>.<id8>.<tool>` and are obtained from
`POST /v1/search`. Instance-scoped describe/call routes are for callers that
already know the target `dcc_type`, `instance_id`, and backend tool id.

## Per-DCC Adapter Endpoints

Per-DCC adapter servers expose the skill REST API for one host process. Their
OpenAPI document may include routes that the gateway does not mount:

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/v1/healthz` | Adapter liveness probe. |
| `GET` | `/v1/readyz` | Adapter readiness: may report `Ready`, `Booting`, or be unreachable while the DCC starts. |
| `GET` | `/v1/skills` | Skills already discovered by that adapter. |
| `POST` | `/v1/search` | Search skills in that adapter's catalog. |
| `POST` | `/v1/load_skill` | Load one adapter skill. |
| `POST` | `/v1/unload_skill` | Unload one adapter skill. |
| `POST` | `/v1/describe` | Describe an adapter-local tool slug. |
| `GET` | `/v1/tools/{slug}` | URL alias of adapter `/v1/describe`. |
| `POST` | `/v1/call` | Call an adapter-local tool slug. |
| `POST` | `/v1/dcc/{dcc_type}/call` | Adapter-local backend call helper; not mounted by the gateway. |
| `GET` | `/v1/context` | Host scene/document snapshot. |
| `GET` | `/v1/resources` | MCP-style resource list for that adapter. |
| `GET` | `/v1/resources/{uri}` | Read one percent-encoded adapter resource URI. |
| `GET` | `/v1/resources/{uri}/events` | Resource update SSE stream. |
| `GET` | `/v1/prompts` | MCP-style prompt template list. |
| `GET` | `/v1/prompts/{name}` | Render one prompt; pass JSON object arguments in `?args=...`. |
| `GET` | `/v1/jobs/{id}/events` | Async job SSE stream. |
| `DELETE` | `/v1/jobs/{id}` | Cancel one async job. |
| `GET` | `/v1/openapi.json` | Per-DCC adapter OpenAPI document. |
| `GET` | `/docs` | Scalar API reference rendered from the adapter OpenAPI document. |

---

## Gateway Agent Debug API

The elected gateway promotes the Admin telemetry providers to stable
`/v1/debug/*` routes for agents and CI diagnostics. These routes are included
in `GET /v1/openapi.json`; the `/admin/api/*` routes remain compatibility
aliases for the embedded dashboard.

This surface requires the gateway `admin` feature and runtime Admin telemetry.
The shipped `dcc-mcp-server` and Python `dcc-mcp-http` gateway path enable it
by default; minimal direct `dcc-mcp-gateway` builds without `admin`, or
runtimes started with Admin disabled (`--no-admin` / `admin_enabled = false`),
omit both the `/v1/debug/*` routes and their OpenAPI entries.

Phase-1 debug routes intentionally preserve the existing Admin payload fields
so operators and agents can compare results one-to-one:

| Stable route | Compatibility route | Notes |
|---|---|---|
| `/v1/debug/instances` | `/admin/api/instances` | Accepts `view=live\|all`, `include_stale`, and `include_dead`. |
| `/v1/debug/activity?limit=200` | `/admin/api/activity?limit=200` | Unified activity feed. |
| `/v1/debug/traces?limit=200` | `/admin/api/traces?limit=200` | Recent dispatch trace rows. |
| `/v1/debug/traces/{request_id}` | `/admin/api/traces/{request_id}` | Exact request-id trace detail. |
| `/v1/debug/traffic?limit=300` | `/admin/api/traffic?limit=300` | Capture state plus recent metadata-only traffic-capture frames from an `admin_live` sink. |
| `/v1/debug/traffic/export?limit=1000` | `/admin/api/traffic/export?limit=1000` | Retained metadata-only traffic-capture frames as JSONL. |
| `/v1/debug/trace-context/{lookup_id}` | n/a | Trace-context lookup by `trace_id` or `request_id`. |
| `/v1/debug/agent-traces/{lookup_id}` | n/a | Public-safe agent packet for one retained trace; accepts `trace_id` or `request_id`. |
| `/v1/debug/bundles/{request_id}` | `/admin/api/debug-bundle/{request_id}` | Accepts request ids and retained trace ids. |
| `/v1/debug/issue-reports/{request_id}` | `/admin/api/issue-report/{request_id}` | Public-safe JSON export suitable for GitHub issue attachment by default; `?mode=raw` includes the full debug bundle for reviewed local evidence. |
| `/v1/debug/workflows` | `/admin/api/workflows` | Agent session/workflow projection from retained search telemetry, traces, and audits. |
| `/v1/debug/tasks` | `/admin/api/tasks` | User-level task outcome projection from retained traces and audits. |
| `/v1/debug/calls` | `/admin/api/calls` | Recent audit rows. |
| `/v1/debug/logs` | `/admin/api/logs` | Merged gateway/file/audit logs. |
| `/v1/debug/stats` | `/admin/api/stats` | Aggregated call stats. |
| `/v1/debug/governance?limit=300` | `/admin/api/governance?limit=300` | Effective policy, read-only state, traffic capture/redaction controls, middleware pressure, and recent allow/deny/throttle decisions. |
| `/v1/debug/health` | `/admin/api/health` | Debug provider health summary. |

Compact-aware debug routes (`/v1/debug/traces`, `/v1/debug/traces/{request_id}`,
`/v1/debug/trace-context/{lookup_id}`, `/v1/debug/bundles/{request_id}`, and
`/v1/debug/stats`) default to JSON for browser and GitHub compatibility. Agents
can request TOON with `Accept: application/toon`, `?response_format=toon`, or
`?compact=true`. The response includes `x-dcc-mcp-*` byte/token accounting
headers. Debug bundle compact output is a public-safe summary with root cause,
tool, DCC type, status, timing, token accounting, redaction summary, postmortem
counts, hints, and links to the full JSON material.

Every list endpoint supports the existing `limit` parameter where the Admin
provider already accepted one. The OpenAPI contract reserves `cursor`,
`since`, and `until` for the follow-up normalized envelope work; callers should
ignore missing `next_cursor` fields until that phase lands.

Common correlation fields include `request_id`, `trace_id`, `instance_id`,
`dcc_type`, `tool` / `tool_slug`, `transport`, `agent_id`, `agent_name`,
`agent_model`, `parent_request_id`, and timestamps where the underlying provider
has them. Use `request_id` for exact request detail and `trace_id` for
full-chain bundles or `/v1/debug/trace-context/{trace_id}`.

Use `/v1/debug/agent-traces/{lookup_id}` for machine-readable agent hand-off
packets. The route resolves both trace ids and request ids and omits request /
response payload previews, prompts, scripts, and scene data. Browser URLs such
as `/admin?panel=traces&trace=<request_id>` and historical
`/admin?agent=traces&trace=<id>` links are UI navigation, not a stable API.

---

## `POST /v1/call` — the invocation contract

### Request body

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "arguments": { "radius": 2.0, "segments": 32 },
  "meta": { "progressToken": "session-42" }
}
```

| Field | Required | Notes |
|---|---|---|
| `tool_slug` | ✅ | Gateway: `<dcc>.<id8>.<tool>`. Direct per-DCC REST: `<dcc>.<skill>.<action>`. Get valid slugs from `POST /v1/search` or `GET /v1/skills` — do **not** construct them by hand. |
| `arguments` | ❌ | Canonical tool-specific input, matching MCP `tools/call`. Missing / `null` / empty string normalizes to `{}`; JSON objects are used as-is; JSON strings that parse to objects are accepted for wrapper compatibility; arrays, booleans, numbers, and non-object strings are rejected. |
| `params` | ❌ | Backward-compatible alias for `arguments`. Prefer `arguments` in new clients so REST and MCP examples stay identical. |
| `meta` | ❌ | MCP-style sidecar. Missing / `null` normalizes to absent. If provided, it must be an object (or an object-shaped JSON string). Honored keys: `progressToken`, `dcc.async`, `dcc.wait_for_terminal`. |

The canonical normalization rules live in `dcc-mcp-wire`; Python host wrappers can reuse them via `dcc_mcp_core.host.normalize_tool_arguments()` and `normalize_tool_meta()` instead of hand-rolling JSON coercion.

### Wrapper payloads and object-shaped arguments

When a **host** (Maya, Blender, Houdini…) or a **connector** (Zapier, n8n, a CI runner) wraps the gateway call surface, the inner payload passed to MCP `call` or REST `/v1/call` / `/v1/call_batch` **MUST** remain a single JSON object with:

1. **`tool_slug`** — a string (e.g. `"maya.a1b2c3d4.create_sphere"`)
2. **`arguments`** — omitted for no-arg tools, or a JSON **object** `{}`
3. **`meta`** (optional) — a JSON **object** `{}`

Backend-specific fields such as `code`, `script`, `file_path`, or `radius` belong inside `arguments`, never at the wrapper top level.

#### ✅ Correct payload (object-shaped arguments)

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "arguments": {
    "radius": 2.0,
    "segments": 32
  },
  "meta": {
    "progressToken": "session-42"
  }
}
```

#### ❌ Incorrect payloads (common failure modes)

**1. Backend fields placed at the wrapper top level**

```json
{
  "tool_slug": "maya.a1b2c3d4.execute_python",
  "code": "cmds.polySphere()"
}
```

**Fix:** move tool input under `arguments`: `{ "tool_slug": "...", "arguments": { "code": "..." } }`.

**2. Non-object JSON such as arrays, booleans, or numbers**

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "arguments": ["radius", 2.0]
}
```

**Error you'll see:**
```
Validation error: document root must be an object
```

**Why?** The server validates `arguments` against the tool's JSON Schema, whose root is an object.

**3. Missing arguments for no-arg tools**

```json
{
  "tool_slug": "maya.a1b2c3d4.list_scene"
}
```

This is now valid and normalizes to `{}`. Explicit `"arguments": {}` is still recommended in examples because it makes wrapper intent obvious.

**4. Double-stringified payload (wrapper serializes twice)**

```python
# ❌ WRONG: serializing the entire payload twice
import json
payload = {
    "tool_slug": "maya.a1b2c3d4.create_sphere",
    "arguments": json.dumps({"radius": 2.0})  # becomes a string
}
requests.post(url, json=payload)  # serializes again → double-stringified
```

```python
# ✅ CORRECT: pass objects directly
import json
payload = {
    "tool_slug": "maya.a1b2c3d4.create_sphere",
    "arguments": {"radius": 2.0}  # object, not string
}
requests.post(url, json=payload)  # serializes once → correct
```

#### Testing wrapper payloads end-to-end

1. **Validate locally** with `jq` or a JSON schema validator:
   ```bash
   echo '$PAYLOAD' | jq .arguments  # must be an object `{}`, not a string
   ```

2. **Call `POST /v1/describe` first** to fetch the tool's schema, then validate your `arguments` against it.

3. **Enable audit logging** (`DCC_MCP_GATEWAY_AUDIT_DIR`) and inspect the JSONL rows:
   - `call.request.arguments` — must be an object, not a string.
   - `call.error` — if present, check whether it mentions "document root must be an object".

4. **Test with parsed objects in your wrapper**:
   ```python
   # ✅ GOOD: parse the JSON response before passing to the next layer
   response = requests.post(url, json=payload)
   result = response.json()  # parse once
   process(result)              # pass parsed object
   ```

#### MCP `tools/call` equivalent

When calling via MCP (not REST), the same rule applies:

```jsonrpc
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "maya.a1b2c3d4.create_sphere",
    "arguments": {"radius": 2.0, "segments": 32}  // ✅ object, not string
  }
}
```

**Remember:** MCP `params.arguments` is omitted or a JSON object `{}`, never an array/number/boolean.

### Success response — `200 OK`

```json
{
  "slug": "maya.a1b2c3d4.create_sphere",
  "output": { "sphere_id": "pSphere1" },
  "validation_skipped": false,
  "request_id": "req-7f3c..."
}
```

`slug` always echoes the slug the caller used so clients can correlate
batched dispatches without threading request IDs through their own
bookkeeping.

### Error response — structured, kebab-cased

```json
{
  "kind": "unknown-slug",
  "message": "no action registered for slug 'maya.a1b2c3d4.make_sphere'",
  "hint": "call /v1/search to list available tools",
  "request_id": "req-7f3c...",
  "candidates": ["maya.a1b2c3d4.create_sphere"]
}
```

Error-kind vocabulary (HTTP status in parentheses):

- `unknown-slug` (404) — no action matched; `candidates` may carry suggested slugs.
- `ambiguous` (409) — slug matched multiple actions; `candidates` lists all of them.
- `skill-not-loaded` (409) — slug is valid but the owning skill isn't loaded. Call MCP `load_skill` first; on the gateway you may target a backend with `instance_id` or `dcc`.
- `invalid-params` (400) — JSON-Schema validation failed against normalized `arguments`.
- `unauthorized` (401) — the `AuthGate` rejected the request. Defaults to localhost-only on per-DCC servers; install `BearerTokenGate` for remote access.
- `not-ready` (503) — `/v1/readyz` is red; DCC is still starting up.
- `host-busy` (503) — the DCC host is alive but its dispatcher is saturated; retry with backoff or route to another live instance.
- `throttled` (429) — gateway middleware rate or concurrency controls rejected the request before backend routing; retry after backoff.
- `affinity-violation` (409) — the caller tried to invoke a main-thread tool from a worker thread.
- `bad-request` (400) — malformed envelope (missing `tool_slug`, bad JSON, etc.).
- `backend-error` (502) — the owning DCC process responded but the tool failed.
- `policy-denied` (403) — **gateway only** — gateway policy rejected the
  operation before routing it to a backend. Inspect `policy.reason`.
- `instance-offline` (503) — **gateway only** — the `<id8>` prefix resolves to an instance that is no longer live.
- `schema-unavailable` (502) — **gateway only** — the owning DCC stopped answering `tools/list` between discovery and call.
- `internal` (500) — the REST layer itself failed; check server logs.

### Gateway Policy

The gateway can narrow the dynamic capability surface before clients see tools
or before backend calls are routed. Configure `GatewayPolicy` from Rust through
`McpHttpConfig::with_gateway_policy(...)` / `with_gateway_read_only(...)`, or
from Python through `McpHttpConfig.gateway_read_only`,
`allowed_dcc_types`, `allowed_skill_names`, `allowed_skill_families`,
`allowed_tool_slugs`, and `allowed_tool_slug_prefixes`.

Policy rules are part of the gateway contract:

- Empty allowlists are unrestricted. Non-empty allowlists are
  case-insensitive and match DCC type, exact skill name, skill family prefix,
  exact canonical gateway `tool_slug`, or `tool_slug` prefix.
- `search` hides policy-denied capabilities, so clients should treat the
  result set as the deployer's allowed surface rather than a complete backend
  inventory.
- `describe` remains available for allowed capabilities even when read-only is
  enabled, so agents can inspect schemas before deciding what is safe.
- `read_only = true` rejects `load_skill`, `unload_skill`, tool-group changes,
  and backend calls unless the backend record declares
  `annotations.readOnlyHint = true`.
- Denied REST operations return HTTP 403 with `kind: "policy-denied"` and a
  structured `policy` object. Batch calls keep HTTP 200 and place the same
  `policy-denied` envelope on the denied result item so batch ordering remains
  stable.
- `GET /v1/debug/governance` exposes the effective read-only state,
  allowlists, capture/redaction controls, middleware quota pressure, and recent
  allow/deny/throttle decisions so agents can inspect the active boundary
  without probing blocked tools.

Example denial:

```json
{
  "kind": "policy-denied",
  "message": "gateway policy denied call for tool slug 'maya.a1b2c3d4.create_sphere'",
  "request_id": "req-7f3c...",
  "policy": {
    "reason": "read-only",
    "operation": "call",
    "read_only": true,
    "dcc_type": "maya",
    "skill_name": "maya-modeling",
    "tool_slug": "maya.a1b2c3d4.create_sphere"
  }
}
```

### Request ID

Every request gets a `request_id` (client-supplied `X-Request-Id` header wins,
otherwise the server generates one). The id flows into the audit log, the
response envelope, and the MCP `_meta.request_id` field on the gateway so
MCP and REST callers can trace the same unit of work.

Gateway REST responses that participate in discovery, skill loading, describe,
single-call, and batch execution also expose stable observability headers:

| Header | Meaning |
|---|---|
| `x-dcc-mcp-request-id` | Gateway request id. Mirrors `X-Request-Id` when supplied. |
| `x-dcc-mcp-trace-id` | End-to-end trace id for gateway, sidecar, and host correlation. |
| `traceparent` | W3C trace context for clients that continue the HTTP trace. |
| `x-dcc-mcp-index-generation` | Opaque capability-index fingerprint when the route touches discovery/call state. |
| `x-dcc-mcp-search-id` | Search-quality correlation id when a route creates or consumes a search result set. |
| `x-dcc-mcp-ranker-version` | Bounded ranker identifier for the correlated search result set. |

`/v1/search`, `/v1/describe`, `/v1/load_skill`, and `/v1/call_batch` also include
`request_id`, `trace_id`, and `index_generation` in JSON/TOON bodies. `/v1/call`
keeps the backend result envelope byte-shape compatible and exposes this metadata
through headers only.
Search responses also include `search_id`, `ranker_version`, and
`index_generation`; pass `meta.search_id` from the returned `next_step` into
`/v1/describe`, `/v1/load_skill`, `/v1/call`, or `/v1/call_batch` so gateway
telemetry can measure the search-to-action path without storing full prompts.

### Caller Attribution

REST callers may add bounded caller attribution in `meta.agent_context`,
top-level `agent_context` / `caller_context`, or headers. MCP callers use the
same shape in `params._meta.agent_context`. Gateway MCP also reads
`initialize.params.clientInfo` once per `Mcp-Session-Id` and uses it to fill
missing `agent_name`, `agent_version`, `agent_kind = "mcp-client"`, and
`client_platform` on later MCP calls. This schema is metadata only; do not send
hidden reasoning, full prompts, raw user messages, secrets, bearer tokens, or
raw agent replies.

| Concept | JSON fields | Header fields |
|---|---|---|
| Actor | `actor_id`, `actor_name`, `actor_email_hash` | `x-dcc-mcp-actor-id`, `x-dcc-mcp-actor-name`, `x-dcc-mcp-actor-email-hash` |
| Agent runtime | `agent_id`, `agent_name`, `agent_kind`, `agent_version`, `model`, `model_provider`, `model_version` | `x-dcc-mcp-agent-id`, `x-dcc-mcp-agent-name`, `x-dcc-mcp-agent`, `x-dcc-mcp-agent-kind`, `x-dcc-mcp-agent-version`, `x-dcc-mcp-agent-model`, `x-dcc-mcp-agent-model-provider`, `x-dcc-mcp-agent-model-version` |
| Client platform | `client_platform`, `client_os`, `client_host` | `x-dcc-mcp-client-platform`, `x-dcc-mcp-client-os`, `x-dcc-mcp-client-host` |
| Auth subject | `auth_subject` | `x-dcc-mcp-auth-subject` |
| Network source | `source_ip`, `forwarded_for` | Server-derived only |

All string fields are bounded before storage. `source_ip` and `forwarded_for`
are reserved for server-derived network data after proxy trust policy has been
applied; values supplied in REST request bodies, MCP `_meta`, or caller headers
are ignored. Use `client_platform` values such as `cursor`, `claude-desktop`,
`openclaw`, `clawhub`, `custom-http`, or `studio-tool` to distinguish surfaces
without overloading agent identity. If a REST request omits `client_platform`,
the gateway falls back to the first `User-Agent` product token, for example
`dcc-mcp-cli` from `dcc-mcp-cli/0.17.37`.

The gateway annotates stored context with a server-computed `trust` map and
Admin rows expose the same map as `attribution_trust`. Values are
`self_reported` for REST body / MCP `_meta`, `header` for ordinary
`x-dcc-mcp-*` headers, `auth` for authentication-derived subjects,
`server_derived` for peer-address network data, and `trusted_proxy` for
forwarded addresses accepted by the configured trusted-proxy depth. Treat
`self_reported` and `header` actor fields as filtering hints only; do not use
them for access control on a LAN unless gateway auth or a trusted proxy owns the
identity boundary. Raw actor email is intentionally unsupported; send only
`actor_email_hash` after pseudonymizing the address. Opt-in traffic capture
masks common attribution identity fields by default and supports explicit
`redact:` rules for any studio-specific metadata paths.

---

## `POST /v1/call_batch` — gateway ordered batches

`/v1/call_batch` is the REST twin of gateway MCP `call({calls:[...]})`. Use
it when an agent must execute several backend tools in a known order without
paying one HTTP/MCP round-trip per step.

```json
{
  "calls": [
    { "id": "create", "tool_slug": "maya.a1b2c3d4.create_sphere", "arguments": { "radius": 2.0 } },
    { "id": "material", "tool_slug": "maya.a1b2c3d4.assign_material", "arguments": { "name": "mat_blue" } }
  ],
  "stop_on_error": true
}
```

Rules:

- `calls` is required and capped at 25 entries.
- Each entry uses the same `tool_slug` / `arguments` / `meta` wrapper shape as
  `POST /v1/call`; missing `arguments` normalizes to `{}`.
- Each entry may include an `id` string/number/boolean. The matching result item
  echoes it unchanged alongside the stable numeric `index`.
- `stop_on_error: true` stops at the first failed call. `false` executes all
  calls and returns per-call success/error envelopes.
- Preserve response order to correlate results with the request array; do not
  infer order from request ids.

---

## `POST /v1/search`

```json
{
  "query": "render",
  "dcc_type": "maya",
  "tags": ["batch"],
  "loaded_only": false,
  "limit": 20,
  "mode": "fuzzy",
  "meta": {
    "agent_context": {
      "model_provider": "openai",
      "model_version": "gpt-5.1",
      "reasoning_effort": "medium",
      "session_id": "session-42",
      "turn_id": "turn-7",
      "user_intent_summary": "Find the render tool before invoking it.",
      "agent_reply_summary": "Search first, then describe or load the best hit.",
      "user_input_hash": "sha256:...",
      "agent_reply_hash": "sha256:...",
      "user_input_chars": 128,
      "agent_reply_chars": 160
    }
  }
}
```

- `query` (required) — free-text. On the gateway, `mode: "fuzzy"`
  (default) uses a hybrid ranker: weighted lexical matches over tool names,
  skills, tags, summaries, author-declared aliases, and schema-field tokens first, then
  nucleo-matcher fuzzy fallback for typos and partial names. `mode: "exact"`
  falls back to the pre-#659 substring table.
- `dcc_type`, `tags`, `loaded_only` — progressive filters. `loaded_only = false` surfaces unloaded skills as search hits so agents can discover `load_skill` candidates.
- `limit` — the server enforces a ~512 B/hit token budget so search stays cheap for large catalogues.
- Gateway policy filters run before the final hit list is returned. A missing
  hit may mean the capability is absent, unloaded, or intentionally hidden by
  DCC, skill, or tool allowlists.
- Gateway hits include `score` plus bounded `match_reasons` such as
  `tool_lexical`, `alias_lexical`, `schema_lexical`, `summary_fuzzy`,
  `schema_fuzzy`, or `multi_token_lexical`
  so agents and maintainers can understand the rank without fetching schemas.
- Gateway hits include one-based `rank`. Generated `next_step` payloads carry
  `meta.search_id`, `meta.ranker_version`, and `meta.index_generation` for
  follow-up calls; clients may also pass the same object as MCP `_meta`.
- Optional `meta.agent_context`, top-level `caller_context`, or
  `x-dcc-mcp-*` attribution headers carry bounded actor, agent, client, auth,
  and turn metadata for Admin workflow, search-quality, and OTLP correlation.
  Gateway MCP calls can also inherit bounded client identity from the session's
  `initialize.params.clientInfo`; REST requests without explicit
  `client_platform` use the first `User-Agent` product token as a fallback.
  Send concise summaries, hashes, and character counts only; raw user input and
  raw agent replies are high sensitivity and belong only in an explicit
  redacted traffic-capture flow.
- Full `input_schema` remains behind `describe`. Search may carry only bounded
  `metadata.dcc.searchAliases` / `metadata.dcc.searchTokens` hints from a
  per-DCC backend to the gateway index; gateway search responses do not expose
  those internal index tokens as public fields.

Gateway response shape:

```json
{
  "search_id": "9c8a1e9f-8d6f-4f3f-86e5-a9caa9138a5d",
  "ranker_version": "gateway-hybrid-v2",
  "index_generation": "1:ab12cd34",
  "total": 3,
  "hits": [
    {
      "rank": 1,
      "slug": "maya.a1b2c3d4.render_frame",
      "skill_name": "maya-render",
      "action_name": "render_frame",
      "dcc": "maya",
      "tags": ["batch"],
      "loaded": false,
      "load_state": "unloaded",
      "available_groups": [
        {
          "name": "core",
          "tools": ["render_frame"],
          "default_active": true,
          "active": false
        }
      ],
      "next_step": {
        "action": "load_skill",
        "arguments": {
          "skill_name": "maya-render",
          "dcc": "maya",
          "dcc_type": "maya",
          "instance_id": "a1b2c3d4-0000-0000-0000-000000000001",
          "meta": {
            "search_id": "9c8a1e9f-8d6f-4f3f-86e5-a9caa9138a5d",
            "ranker_version": "gateway-hybrid-v2",
            "index_generation": "1:ab12cd34"
          }
        },
        "mcp": {
          "tool": "load_skill",
          "arguments": {
            "skill_name": "maya-render",
            "dcc_type": "maya",
            "instance_id": "a1b2c3d4-0000-0000-0000-000000000001",
            "meta": {
              "search_id": "9c8a1e9f-8d6f-4f3f-86e5-a9caa9138a5d",
              "ranker_version": "gateway-hybrid-v2",
              "index_generation": "1:ab12cd34"
            }
          },
          "_meta": {
            "search_id": "9c8a1e9f-8d6f-4f3f-86e5-a9caa9138a5d",
            "ranker_version": "gateway-hybrid-v2",
            "index_generation": "1:ab12cd34"
          }
        },
        "rest": {
          "method": "POST",
          "path": "/v1/load_skill",
          "body": {
            "skill_name": "maya-render",
            "dcc_type": "maya",
            "instance_id": "a1b2c3d4-0000-0000-0000-000000000001",
            "meta": {
              "search_id": "9c8a1e9f-8d6f-4f3f-86e5-a9caa9138a5d",
              "ranker_version": "gateway-hybrid-v2",
              "index_generation": "1:ab12cd34"
            }
          }
        }
      }
    }
  ]
}
```

When `loaded=false`, clients may POST `next_step.arguments` directly to
`/v1/load_skill`, then repeat `/v1/search` or call `/v1/describe` for the same
tool. Per-DCC REST omits `instance_id` because there is only one owning server;
the gateway includes it so same-DCC multi-instance calls stay routed. Gateway
`load_skill` defaults to lazy group activation (`activate_groups=false` unless
supplied), so heavier groups should be activated explicitly.

### Compact output

`/v1/search`, `/v1/describe`, `/v1/tools/{slug}`, the direct per-instance
describe/call routes, `/v1/call`, and `/v1/call_batch` return compact TOON by
default. Set `DCC_MCP_GATEWAY_RESPONSE_FORMAT=json` (or the legacy alias
`DCC_MCP_RESPONSE_FORMAT=json`) when a deployment needs a JSON-first
compatibility window. REST clients can also opt out per request:

```bash
curl -H 'Accept: application/json' \
  -d '{"query":"render","limit":20}' \
  http://127.0.0.1:9765/v1/search
```

The request body may set `"response_format": "json"` to force legacy JSON, or
`"response_format": "toon"` / `"compact": true` to force compact output even
when an `Accept` header prefers JSON. If neither `Accept` nor the body says
otherwise, REST returns `application/toon`.

Every compact-capable REST response includes approximate token accounting
headers:

| Header | Meaning |
|---|---|
| `x-dcc-mcp-response-format` | `json` or `toon`. |
| `x-dcc-mcp-token-estimator` | Estimator id; currently `dcc-mcp-byte4-v1`. |
| `x-dcc-mcp-original-bytes` / `x-dcc-mcp-returned-bytes` | Serialized legacy JSON bytes vs returned bytes. |
| `x-dcc-mcp-original-tokens` / `x-dcc-mcp-returned-tokens` | Approximate bytes/4 token estimates for planning context budget, not billing. |
| `x-dcc-mcp-saved-tokens` / `x-dcc-mcp-savings-pct` | Estimated savings compared with legacy JSON. |

The same accounting is copied into retained gateway traces, audited call rows,
and `/v1/debug/stats` / `/admin/api/stats` aggregates. Legacy JSON responses
are recorded explicitly with `response_format: "json"` and zero token savings,
so clients can compare compact and compatibility traffic without fetching full
trace payloads.

The compact search shape preserves the workflow fields agents need next:
`tool_slug`, `backend_tool`, `dcc_type`, `instance_id`, `loaded`,
`load_state`, `available_groups`, `has_schema`, `score`, `match_reasons`, and
`rank`, `search_id`, `ranker_version`, `index_generation`, and `next_step` for unloaded skills. It omits redundant defaults such as `callable_id` when it
matches `backend_tool`, empty arrays, and empty objects. RTK's compaction model
is treated as design guidance here; the
gateway uses the deterministic in-process `toon-format` library so
`serde_json::Value` payloads round-trip inside Rust tests without spawning an
external codec process.

Compact describe output applies the same small-record rules to `record` while
preserving the full `tool` definition verbatim, including `inputSchema`,
annotations, and validation hints. Compact call output preserves the same
success/error envelope and HTTP status as JSON, just encoded as TOON. Compact
batch output preserves request order and includes per-result `token_accounting`
metadata, while the response headers report the aggregate body savings. Legacy
JSON batch responses keep the same result shape and still report zero savings in
the `x-dcc-mcp-*` accounting headers.

The gateway MCP endpoint exposes the same compact codec without changing
JSON-RPC framing. Legacy MCP clients that omit response-format metadata keep the
normal JSON result shape. Compact-capable MCP clients should set
`params._meta.response_format` to `"toon"` (or `params._meta.compact=true`) on
`tools/list`, `resources/read`, `prompts/get`, or `tools/call` requests after
`initialize` advertises
`capabilities.experimental["dcc-mcp"].compactResponses`; set
`params._meta.response_format="json"` to opt out for a single request.
Non-`tools/call` compact results become a JSON object with `response_format`,
`mimeType`, `text`, and `_meta.token_accounting`. `tools/call` keeps the MCP `CallToolResult` shape:
`content[]`, `type`, and `isError` stay in place, while text content receives
`mimeType: "application/toon"` and TOON text. JSON-RPC errors remain normal
`error` objects so legacy error handling continues to work.

## `POST /v1/load_skill` and `/v1/unload_skill`

```json
{ "skill_name": "maya-render", "dcc": "maya", "instance_id": "a1b2c3d4" }
```

`skill_name` is required. Gateway callers should pass the `dcc` / `dcc_type`
and `instance_id` returned in `/v1/search.next_step.arguments` when more than
one DCC instance is live. A successful load refreshes the gateway capability
index and returns `loaded`, `skill_name`, `dcc_type`, `instance_id`,
`activated_groups`, `new_tool_slugs`, `index_generation`, and a suggested
`next_step` (`describe` when a new slug is known, otherwise `search`).
Use the returned `index_generation` or the `x-dcc-mcp-index-generation` header
to decide whether cached `tool_slug` values came from an older gateway index.

---

## `POST /v1/describe`

```json
{ "tool_slug": "maya.a1b2c3d4.render_frame", "include_schema": true }
```

Returns a `record` (compact capability descriptor) plus the full `tool`
definition (`input_schema`, annotations, next-tool hints). `include_schema`
defaults to `true`; set `false` on follow-up calls where only the metadata
has changed.
The response includes the current `index_generation` so generated clients can
detect stale describe/call sequences without parsing logs. When the describe
request includes `meta.search_id`, the response `next_step` for `call` carries
that search id forward and the gateway records selected-rank telemetry.

---

## Resources, prompts, and jobs over REST

`GET /v1/resources` returns `{total, resources, request_id}` where each record
mirrors MCP's resource definition shape. To read a resource, percent-encode the
entire URI into the path segment:

```bash
curl -s 'http://127.0.0.1:8765/v1/resources/scene%3A%2F%2Fcurrent'
```

`GET /v1/prompts` returns `{total, prompts, request_id}`. Render one template
with `GET /v1/prompts/{name}`; when the prompt declares arguments, pass a JSON
object in the `args` query parameter. The gateway uses this same REST hop when
serving MCP `prompts/get`, so prompt arguments are preserved end-to-end.

Async jobs can be watched with `GET /v1/jobs/{id}/events` and cancelled with
`DELETE /v1/jobs/{id}`. Resource subscriptions use
`GET /v1/resources/{uri}/events`. Both event endpoints are Server-Sent Events.

---

## Readiness (`GET /v1/readyz`)

On the gateway, `GET /v1/readyz` always returns `200` and summarises the current
registry view. Besides `live_instance_count`, `ready_instance_count`, and
`not_ready_instance_count`, it reports `dispatch_reported_instance_count`,
`dispatch_ready_instance_count`, and `dispatch_not_ready_instance_count`; each
instance row also includes the same nested `dispatch` object exposed by
`GET /v1/instances`. Use those dispatch counters for sidecar-driven adapters:
they distinguish "the DCC process is listed" from "the sidecar dispatcher is
actually callable". The same response includes the per-instance `gateway`
object plus `gateway_recovery_driver_counts`, `registration_refresh_mode_counts`,
`gateway_daemon_guardian_instance_count`, and `gateway_daemon_guardian_ready`.
Those fields let launchers and admin panels answer whether at least one live DCC
service can restart the machine-wide gateway daemon.

Per-DCC `/v1/readyz` endpoints use the readiness states below:

| State | HTTP | Body | Meaning |
|---|---|---|---|
| `Ready` | 200 | `{ "process": true, "dcc": true, "skill_catalog": true, "dispatcher": true, "host_execution_bridge": true, "main_thread_executor": true }` | Base routing bits are green; `POST /v1/call` will dispatch. |
| `Booting` | 503 | `{ "status": "booting", ... which bits are red }` | The server is up but the DCC host or dispatcher hasn't finished initialising. The gateway keeps the instance's registry row but won't route traffic there. |
| `Unreachable` | no response | — | `/v1/readyz` didn't answer in 5 s. Gateway falls back to `GET /health` for pre-#660 backends; still nothing → the instance is counted as a probe failure and deregistered after 3 consecutive misses. |

The distinction matters: "my tool disappeared" has two very different
diagnoses and fixes. Surface `process`, `dcc`, `skill_catalog`, `dispatcher`,
`host_execution_bridge`, and `main_thread_executor` separately in your
dashboards.

---

## Envelope parity with MCP

| Concern | MCP `call` (JSON-RPC) | REST `POST /v1/call` | Parity? |
|---|---|---|---|
| Success body | `result.content[].text` (str JSON) | `{slug, output, validation_skipped, request_id}` | ✅ same underlying `CallOutcome` |
| Error body | `result.content[].text` (str JSON) with `isError: true` | `{kind, message, hint?, request_id, candidates?}` | ✅ same `ServiceError`; MCP wraps it into the MCP `CallToolResult` shape |
| `request_id` | `_meta.request_id` | top-level field | ✅ same value |
| Cancellation | `notifications/cancelled` | `DELETE /v1/jobs/{id}` for async jobs | ✅ both reach the cooperative-cancellation path |
| Progress/job events | `notifications/progress` and `notifications/$/dcc.jobUpdated` | `GET /v1/jobs/{id}/events` SSE | ✅ job lifecycle is available over both transports |
| Resource updates | `notifications/resources/updated` after `resources/subscribe` | `GET /v1/resources/{uri}/events` SSE | ✅ resource updates are available over both transports |

The contract is locked down by OpenAPI snapshot tests in
`crates/dcc-mcp-skill-rest/src/openapi.rs` (`call_request_schema_contract_is_stable`,
`call_outcome_schema_contract_is_stable`). Any change to these tests
indicates a downstream-visible envelope break.

---

## When to choose REST vs MCP

| You are … | Prefer |
|---|---|
| Writing an **AI agent** (Claude, Cursor, ChatGPT desktop, custom) | **MCP** when available: use gateway `search` / `describe` / `load_skill` / `call`. Use REST when the client has no MCP stack or needs OpenAPI-generated HTTP bindings. |
| Writing a **cURL script** / cron job / CI pipeline | **REST**. Pure HTTP + JSON, no MCP library required. |
| Writing an **enterprise backend** that talks to many DCCs | **REST on the gateway**. Single endpoint, same envelope across all DCCs, OpenAPI doc for code generation. |
| Writing an **in-host plugin** (Maya plug-in, Blender add-on) | Neither — call `DccServerBase.register_*` directly. REST / MCP are for external callers. |
| Debugging "why didn't my tool run?" | **REST** first: `curl /v1/healthz` then `/v1/readyz` then `/v1/search`. The three endpoints give you a straight line from "is the process alive" to "is my tool discoverable". |

---

## Pluggable traits (for embedders)

The REST surface is composed of five traits, every one of them swappable.
Defaults work out of the box for localhost-only development; production
deployments replace the ones that matter to them.

| Trait | Default | Common overrides |
|---|---|---|
| `SkillCatalogSource` | Live `SkillCatalog` | A test fixture that returns a canned action list; a read-through cache against a remote registry. |
| `ToolInvoker` | `DispatcherInvoker` over `ToolDispatcher` | A queue-backed invoker that posts jobs onto the DCC main thread via `QueueDispatcher`. |
| `AuthGate` | `AllowLocalhostGate` | `BearerTokenGate::new(vec![token])` for remote access; a studio SSO gate for enterprise. |
| `AuditSink` | `NoopSink` | A `FileAuditSink` that appends JSONL rows; a Kafka producer for central audit. |
| `ReadinessProbe` | Static `Ready` | A probe that checks the DCC host's own `ready` signal and goes red while the scene is loading. |

This is the DIP story for the REST surface: handlers depend on the trait,
not the concrete type, so you never touch `handle_call` to plug in custom
auth / audit / invocation.

---

## Next reads

- [Gateway diagnostics](gateway-diagnostics.md) — how to read contention, probes, election, and ghost eviction.
- [CLI reference](cli-reference.md) — launching per-DCC servers, tunnel relay + agent.
- [AGENTS.md](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/AGENTS.md) — rules for AI agents.
