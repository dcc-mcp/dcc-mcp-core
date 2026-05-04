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
| `GET` | `/v1/skills` | Flat listing of loaded actions, deterministically sorted. |
| `POST` | `/v1/search` | Fuzzy / exact search across loaded + unloaded skills. |
| `POST` | `/v1/describe` | Return the full input schema + annotations for one `tool_slug`. |
| `GET` | `/v1/tools/{slug}` | Alias of `/v1/describe` (read-only lookup via URL). |
| `POST` | `/v1/call` | **Invoke** a tool by slug. This is the canonical invocation plane. |
| `GET` | `/v1/context` | Scene / document snapshot (per-DCC or gateway-aggregated). |
| `GET` | `/v1/openapi.json` | Auto-generated OpenAPI 3.x document for code-gen clients. |

The gateway exposes the same paths as an aggregating facade: `POST /v1/call`
on the gateway parses `<dcc>.<id8>.<action>` out of the slug and forwards to
the owning per-DCC backend.

---

## `POST /v1/call` — the invocation contract

### Request body

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "params": { "radius": 2.0, "segments": 32 },
  "meta": { "progressToken": "session-42" }
}
```

| Field | Required | Notes |
|---|---|---|
| `tool_slug` | ✅ | `<dcc>.<id8>.<action>` three-part form. The 8-hex-char `id8` prefix disambiguates identical action names across multiple live DCC instances. Get valid slugs from `POST /v1/search` or `GET /v1/skills` — do **not** construct them by hand. |
| `params` | ❌ | Tool-specific input. Defaults to `{}` so single-arg / no-arg calls stay ergonomic for cURL. The server validates this against the tool's JSON-Schema before dispatch. |
| `meta` | ❌ | MCP-style sidecar. Honored keys: `progressToken` (binds progress events to a client session), `dcc.async` (opt in to async dispatch), `dcc.wait_for_terminal` (block until terminal status). |

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
- `skill-not-loaded` (409) — slug is valid but the owning skill isn't loaded. Call `load_skill` first (via MCP or through the skill-management REST endpoints).
- `invalid-params` (400) — JSON-Schema validation failed against `params`.
- `unauthorized` (401) — the `AuthGate` rejected the request. Defaults to localhost-only on per-DCC servers; install `BearerTokenGate` for remote access.
- `not-ready` (503) — `/v1/readyz` is red; DCC is still starting up.
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
      "loaded": true
    }
  ]
}
```

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

## Three-state readiness (`GET /v1/readyz`)

| State | HTTP | Body | Meaning |
|---|---|---|---|
| `Ready` | 200 | `{ "status": "ready", "process": "ok", "dispatcher": "ok", "dcc": "ok" }` | Every readiness bit is green; `POST /v1/call` will dispatch. |
| `Booting` | 503 | `{ "status": "booting", ... which bits are red }` | The server is up but the DCC host or dispatcher hasn't finished initialising. The gateway keeps the instance's registry row but won't route traffic there. |
| `Unreachable` | no response | — | `/v1/readyz` didn't answer in 5 s. Gateway falls back to `GET /health` for pre-#660 backends; still nothing → the instance is counted as a probe failure and deregistered after 3 consecutive misses. |

The distinction matters: "my tool disappeared" has two very different
diagnoses and fixes. Surface `process`, `dispatcher`, `dcc` separately in
your dashboards.

---

## Envelope parity with MCP

| Concern | MCP `call_tool` (JSON-RPC) | REST `POST /v1/call` | Parity? |
|---|---|---|---|
| Success body | `result.content[].text` (str JSON) | `{slug, output, validation_skipped, request_id}` | ✅ same underlying `CallOutcome` |
| Error body | `result.content[].text` (str JSON) with `isError: true` | `{kind, message, hint?, request_id, candidates?}` | ✅ same `ServiceError`; MCP wraps it into the MCP `CallToolResult` shape |
| `request_id` | `_meta.request_id` | top-level field | ✅ same value |
| Cancellation | `notifications/cancelled` | close the HTTP connection | ✅ both reach the cooperative-cancellation path |
| Progress events | `notifications/progress` bound via `_meta.progressToken` | Server-Sent Events on a long-poll (roadmap #604) | ⚠ MCP only for now |

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
| `ToolInvoker` | `DispatcherInvoker` over `ActionDispatcher` | A queue-backed invoker that posts jobs onto the DCC main thread via `QueueDispatcher`. |
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
