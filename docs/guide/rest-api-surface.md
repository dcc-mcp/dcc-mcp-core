# REST API Surface

Every per-DCC server and the multi-DCC gateway expose the same `/v1/*` REST
surface, alongside their MCP endpoint. This page is the integration contract
for **traditional callers** (cURL, CI pipelines, studio automation, non-MCP
tooling) — anything that can speak HTTP can drive a DCC through these routes
without touching the MCP protocol stack.

> **Relationship to MCP** — Gateway MCP's `call_tool` / `describe_tool` /
> `search_tools` wrappers route through the same code path as the REST
> endpoints. Choosing MCP vs REST is a transport decision, not a feature
> decision; the envelopes are identical.

---

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| `GET` | `/v1/healthz` | Liveness probe. `200 {"status": "ok"}` as long as the HTTP handler is up. |
| `GET` | `/v1/readyz` | Three-state readiness: `200 Ready` / `503 Booting` / body omitted `Unreachable` (see below). |
| `GET` | `/v1/skills` | Flat listing of loaded tools, deterministically sorted. |
| `POST` | `/v1/search` | Fuzzy / exact search across loaded + unloaded skills. |
| `POST` | `/v1/load_skill` | Load a discovered skill without using MCP `tools/call`. |
| `POST` | `/v1/unload_skill` | Unload a skill without using MCP `tools/call`. |
| `POST` | `/v1/describe` | Return the full input schema + annotations for one `tool_slug`. |
| `GET` | `/v1/tools/{slug}` | Alias of `/v1/describe` (read-only lookup via URL). |
| `POST` | `/v1/call` | **Invoke** a tool by slug. This is the canonical invocation plane. |
| `POST` | `/v1/call_batch` | Gateway only: invoke up to 25 ordered tool calls with optional `stop_on_error`. |
| `GET` | `/v1/context` | Scene / document snapshot (per-DCC or gateway-aggregated). |
| `GET` | `/v1/resources` | MCP-style resource list. |
| `GET` | `/v1/resources/{uri}` | Read one percent-encoded resource URI. |
| `GET` | `/v1/resources/{uri}/events` | Server-Sent Events for resource changes. |
| `GET` | `/v1/prompts` | MCP-style prompt template list. |
| `GET` | `/v1/prompts/{name}` | Render one prompt; pass JSON object arguments in `?args=...`. |
| `GET` | `/v1/jobs/{id}/events` | Server-Sent Events for one async job. |
| `DELETE` | `/v1/jobs/{id}` | Cancel one async job. |
| `GET` | `/v1/debug/instances` | Gateway only: stable agent-facing instance diagnostics. |
| `GET` | `/v1/debug/activity` | Gateway only: stable activity feed from audits, traces, and gateway events. |
| `GET` | `/v1/debug/traces` | Gateway only: recent dispatch trace list. |
| `GET` | `/v1/debug/traces/{request_id}` | Gateway only: dispatch trace detail by request id. |
| `GET` | `/v1/debug/trace-context/{lookup_id}` | Gateway only: resolve trace id or request id to the primary trace context. |
| `GET` | `/v1/debug/bundles/{request_id}` | Gateway only: full-chain debug bundle by request id or trace id. |
| `GET` | `/v1/debug/issue-reports/{request_id}` | Gateway only: GitHub-attachable debug report JSON. |
| `GET` | `/v1/debug/tasks` | Gateway only: task-like snapshots reconstructed from traces. |
| `GET` | `/v1/debug/calls` | Gateway only: recent audited calls. |
| `GET` | `/v1/debug/logs` | Gateway only: merged gateway events, file logs, and audit summaries. |
| `GET` | `/v1/debug/stats` | Gateway only: aggregated call statistics. |
| `GET` | `/v1/debug/health` | Gateway only: debug subsystem health summary. |
| `GET` | `/v1/openapi.json` | Auto-generated OpenAPI 3.x document for code-gen clients. |

The gateway exposes the same paths as an aggregating facade. Gateway capability
slugs use `<dcc>.<id8>.<tool>` and are obtained from `POST /v1/search`; direct
per-DCC REST slugs use `<dcc>.<skill>.<action>` and do not include an instance
id prefix.

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
| `/v1/debug/trace-context/{lookup_id}` | n/a | Trace-context lookup by `trace_id` or `request_id`. |
| `/v1/debug/bundles/{request_id}` | `/admin/api/debug-bundle/{request_id}` | Accepts request ids and retained trace ids. |
| `/v1/debug/issue-reports/{request_id}` | `/admin/api/issue-report/{request_id}` | JSON export suitable for GitHub issue attachment. |
| `/v1/debug/tasks` | `/admin/api/tasks` | Task projection from retained traces. |
| `/v1/debug/calls` | `/admin/api/calls` | Recent audit rows. |
| `/v1/debug/logs` | `/admin/api/logs` | Merged gateway/file/audit logs. |
| `/v1/debug/stats` | `/admin/api/stats` | Aggregated call stats. |
| `/v1/debug/health` | `/admin/api/health` | Debug provider health summary. |

Every list endpoint supports the existing `limit` parameter where the Admin
provider already accepted one. The OpenAPI contract reserves `cursor`,
`since`, and `until` for the follow-up normalized envelope work; callers should
ignore missing `next_cursor` fields until that phase lands.

Common correlation fields include `request_id`, `trace_id`, `instance_id`,
`dcc_type`, `tool` / `tool_slug`, `transport`, `agent_id`, `agent_name`,
`agent_model`, `parent_request_id`, and timestamps where the underlying provider
has them. Use `request_id` for exact request detail and `trace_id` for
full-chain bundles or `/v1/debug/trace-context/{trace_id}`.

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

When a **host** (Maya, Blender, Houdini…) or a **connector** (Zapier, n8n, a CI runner) wraps the gateway call surface, the inner payload passed to `call_tool` / `call_tools` **MUST** remain a single JSON object with:

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
- `affinity-violation` (409) — the caller tried to invoke a main-thread tool from a worker thread.
- `bad-request` (400) — malformed envelope (missing `tool_slug`, bad JSON, etc.).
- `backend-error` (502) — the owning DCC process responded but the tool failed.
- `instance-offline` (503) — **gateway only** — the `<id8>` prefix resolves to an instance that is no longer live.
- `schema-unavailable` (502) — **gateway only** — the owning DCC stopped answering `tools/list` between discovery and call.
- `internal` (500) — the REST layer itself failed; check server logs.

### Request ID

Every request gets a `request_id` (client-supplied `X-Request-Id` header wins,
otherwise the server generates one). The id flows into the audit log, the
response envelope, and the MCP `_meta.request_id` field on the gateway so
MCP and REST callers can trace the same unit of work.

---

## `POST /v1/call_batch` — gateway ordered batches

`/v1/call_batch` is the REST twin of the gateway MCP `call_tools` wrapper. Use
it when an agent must execute several backend tools in a known order without
paying one HTTP/MCP round-trip per step.

```json
{
  "calls": [
    { "tool_slug": "maya.a1b2c3d4.create_sphere", "arguments": { "radius": 2.0 } },
    { "tool_slug": "maya.a1b2c3d4.assign_material", "arguments": { "name": "mat_blue" } }
  ],
  "stop_on_error": true
}
```

Rules:

- `calls` is required and capped at 25 entries.
- Each entry uses the same `tool_slug` / `arguments` / `meta` wrapper shape as
  `POST /v1/call`; missing `arguments` normalizes to `{}`.
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
  "mode": "fuzzy"
}
```

- `query` (required) — free-text. `mode: "fuzzy"` (default) uses a nucleo-matcher-backed scorer with typo / prefix tolerance; `mode: "exact"` falls back to the pre-#659 substring table.
- `dcc_type`, `tags`, `loaded_only` — progressive filters. `loaded_only = false` surfaces unloaded skills as search hits so agents can discover `load_skill` candidates.
- `limit` — the server enforces a ~512 B/hit token budget so search stays cheap for large catalogues.

Response shape (gateway + per-DCC are identical):

```json
{
  "total": 3,
  "hits": [
    {
      "slug": "maya.a1b2c3d4.render_frame",
      "skill_name": "maya-render",
      "action_name": "render_frame",
      "dcc": "maya",
      "tags": ["batch"],
      "loaded": false,
      "next_step": {
        "action": "load_skill",
        "arguments": {
          "skill_name": "maya-render",
          "dcc": "maya",
          "dcc_type": "maya",
          "instance_id": "a1b2c3d4-0000-0000-0000-000000000001"
        },
        "rest": {"method": "POST", "path": "/v1/load_skill"},
        "mcp": {"name": "load_skill"}
      }
    }
  ]
}
```

When `loaded=false`, clients may POST `next_step.arguments` directly to
`/v1/load_skill`, then repeat `/v1/search` or call `/v1/describe` for the same
tool. Per-DCC REST omits `instance_id` because there is only one owning server;
the gateway includes it so same-DCC multi-instance calls stay routed.

### Compact output

`/v1/search` keeps legacy JSON as the default response format. Agent clients
that want a smaller discovery payload can request TOON explicitly:

```bash
curl -H 'Accept: application/toon' \
  -d '{"query":"render","limit":20}' \
  http://127.0.0.1:9765/v1/search
```

The request body may also set `"response_format": "toon"` or `"compact": true`.
Set `"response_format": "json"` to force the legacy JSON body even when the
`Accept` header prefers TOON.

Every search response includes approximate token accounting headers:

| Header | Meaning |
|---|---|
| `x-dcc-mcp-response-format` | `json` or `toon`. |
| `x-dcc-mcp-token-estimator` | Estimator id; currently `dcc-mcp-byte4-v1`. |
| `x-dcc-mcp-original-bytes` / `x-dcc-mcp-returned-bytes` | Serialized legacy JSON bytes vs returned bytes. |
| `x-dcc-mcp-original-tokens` / `x-dcc-mcp-returned-tokens` | Approximate bytes/4 token estimates for planning context budget, not billing. |
| `x-dcc-mcp-saved-tokens` / `x-dcc-mcp-savings-pct` | Estimated savings compared with legacy JSON. |

The compact search shape preserves the workflow fields agents need next:
`tool_slug`, `backend_tool`, `dcc_type`, `instance_id`, `loaded`,
`has_schema`, `score`, and `next_step` for unloaded skills. It omits redundant
defaults such as `callable_id` when it matches `backend_tool`, empty arrays, and
empty objects. RTK's compaction model is treated as design guidance here; the
gateway uses the deterministic in-process `toon-format` library so
`serde_json::Value` payloads round-trip inside Rust tests without spawning an
external codec process.

## `POST /v1/load_skill` and `/v1/unload_skill`

```json
{ "skill_name": "maya-render", "dcc": "maya", "instance_id": "a1b2c3d4" }
```

`skill_name` is required. Gateway callers should pass the `dcc` / `dcc_type`
and `instance_id` returned in `/v1/search.next_step.arguments` when more than
one DCC instance is live. A successful load refreshes the gateway capability
index, so the next `/v1/search` sees the newly callable actions.

---

## `POST /v1/describe`

```json
{ "tool_slug": "maya.a1b2c3d4.render_frame", "include_schema": true }
```

Returns a `record` (compact capability descriptor) plus the full `tool`
definition (`input_schema`, annotations, next-tool hints). `include_schema`
defaults to `true`; set `false` on follow-up calls where only the metadata
has changed.

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

| Concern | MCP `call_tool` (JSON-RPC) | REST `POST /v1/call` | Parity? |
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
| Writing an **AI agent** (Claude, Cursor, ChatGPT desktop, custom) | **MCP**. Use `search_tools` / `describe_tool` / `call_tool` against the gateway; get streaming events, progressive discovery, and the MCP capability registry. |
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
- [AGENTS.md](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md) — rules for AI agents.
