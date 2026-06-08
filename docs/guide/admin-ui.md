# Built-in Admin Dashboard

The gateway ships an embedded `/admin` web dashboard (issue #772). At runtime it is a single HTML payload served from the binary; contributors edit the Vite/React source in `admin-ui/`, and `crates/dcc-mcp-gateway/build.rs` embeds the built asset during Cargo builds.

## Activation and Defaults

`/admin` is enabled by default on the elected gateway. This is intentional: the gateway and admin dashboard are part of the default local observability surface.

### `dcc-mcp-server` / `server.exe`

```bash
# Default: joins gateway election on :9765; elected process serves /admin
dcc-mcp-server --app maya

# Disable gateway entirely (also disables admin)
dcc-mcp-server --gateway-port 0

# Keep gateway but disable admin
dcc-mcp-server --no-admin

# Move admin under another prefix
dcc-mcp-server --admin-path /dcc-admin
```

Equivalent env vars:

| Env var | Default | Description |
|---------|---------|-------------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | Gateway election port. `0` disables gateway/admin. |
| `DCC_MCP_NO_ADMIN` | `false` | Disable the read-only Admin UI on the elected gateway. |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL prefix. |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | unset | Optional JSONL directory for durable `audit.jsonl` and `traces.jsonl`; unset keeps zero-disk in-memory behavior. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | Max JSONL rows retained per durable file when persistence is enabled. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | Approx. 50 MiB byte cap per durable JSONL file; the gateway enforces both row and byte limits. |
| `DCC_MCP_LOG_DIR` | platform log dir | Directory scanned by `/admin/api/logs` for `*.log` files; defaults to `%USERPROFILE%\\AppData\\Local\\dcc-mcp\\log` on Windows and `~/.local/share/dcc-mcp/log` elsewhere. |

### Python API

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=0, server_name="maya-mcp")
# Defaults for Python embedders:
# cfg.gateway_port == 9765
# cfg.admin_enabled is True
# cfg.admin_path == "/admin"

# Disable gateway/admin for an isolated local-only server:
cfg.gateway_port = 0

# Or keep gateway but hide admin:
cfg.admin_enabled = False

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

### Rust gateway API

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,          // default
    admin_path: "/admin".into(),  // default
    ..GatewayConfig::default()
};
```

When using `dcc-mcp-gateway` directly, compile with the `admin` Cargo feature. `dcc-mcp-http` and the shipped server binary enable this for their embedded gateway path.

## Locale Detection

The embedded Admin UI includes a small in-bundle i18n runtime. It reads
`navigator.languages` / `navigator.language`, normalizes supported browser tags,
and falls back to English when no supported preference is present. No
translation assets are fetched over the network.

Supported runtime locales are:

- `en`
- `zh-CN` for `zh`, `zh-Hans`, `zh-CN`, and other Simplified Chinese tags
- `ja` for `ja` / `ja-JP`
- `ko` for `ko` / `ko-KR`

Translation entries live in feature namespaces in `admin-ui/src/i18n.ts`.
Shared chrome and status labels use `common`, app shell text uses `chrome`,
navigation labels use `navigation`, and panel-owned copy can live in its own
namespace such as `setup`, `health`, `instances`, `tools`, `tasks`, `openapi`,
`calls`, `traces`, `stats`, `logs`, or `skillPaths`.

Panels should request only their own namespace plus shared namespaces via the
typed namespace translator. Dynamic text should use the interpolation helper
instead of string concatenation so future translations can reorder grammar.
Machine identifiers such as tool slugs, request ids, DCC types, JSON fields,
and log payloads should remain unmodified. Tests audit namespace/locale parity
so missing keys are caught before release. Future manual language selection
should pass an explicit override through the same detection helper instead of
bypassing the runtime.

### Maintaining Admin UI Translations

When adding a panel or changing visible UI chrome:

1. Add or update the feature namespace in `admin-ui/src/i18n.ts`.
2. Provide every key for all supported locales: `en`, `zh-CN`, `ja`, and `ko`.
3. Keep shared actions, table labels, status notices, and search metadata in
   `common` or `search` when they are reused across panels.
4. Use interpolation placeholders such as `{count}` or `{value}` for dynamic
   grammar instead of assembling translated sentences with string
   concatenation in React code.
5. Do not translate machine identifiers or raw technical payloads: tool slugs,
   request IDs, trace IDs, DCC types, file paths, JSON keys, HTTP methods,
   status codes, log messages, and backend-provided payload text must remain
   exact.

Validation for i18n changes:

```bash
vx npm run build
vx npx playwright test tests/i18n.spec.ts tests/admin.spec.ts
vx just admin-build
vx git diff --check
```

`tests/i18n.spec.ts` verifies locale normalization and namespace parity for all
supported locales. `tests/admin.spec.ts` includes mocked admin API flows in both
English and non-English browser locales so UI chrome is localized while machine
data remains stable.

## Dashboard Screenshots

The screenshots below use representative demo data and show the browser-first
operator workflows exposed by the embedded dashboard.

![Admin Connect IDE panel](../assets/admin-ui/admin-connect-ide.png)

The **Connect IDE** panel provides copyable MCP configuration snippets for
Claude Desktop, Cursor, CodeBuddy, VS Code, Cline, and Codex / OpenAI, using
the current gateway URL and platform-aware config paths.

![Admin Skills paths panel](../assets/admin-ui/admin-skills-paths.png)

The **Skills** panel shows loaded skills, action counts, per-instance prefixes,
active skill discovery roots, and the local developer default
`~/.dcc-mcp/{dcc-type}/skills` path when present.

![Admin skill markdown detail panel](../assets/admin-ui/admin-skill-detail.png)

Clicking a skill opens its detail panel, including backend instance metadata,
registered tools, `SKILL.md` source path, parsed frontmatter, and rendered
Markdown body for developer review.

## Marketplace Panel

The **Marketplace** panel, accessible from the left navigation, provides a
graphical interface for browsing, installing, and managing skill packages from
the DCC-MCP marketplace catalog. It exposes the same capabilities as the CLI
`marketplace` subcommand through two tabs: **Browse** and **Installed**.

### Browse Tab

The Browse tab displays available skill packages in a searchable catalog grid.
Each card shows the package name, description, DCC type badges, current version,
and an Install button.

Users can filter the catalog by:
- **Search query**: text search across name, description, and tags.
- **DCC type filter**: a row of Chip buttons at the top of the tab to filter by
  DCC type. Search and DCC filter can be combined.

Clicking a card opens the **Marketplace detail modal** showing full metadata:
name, description, version, tags, DCC type, maintainer, project URL, source,
install type (git, zip, path), and `min_core_version`. When the package's
`min_core_version` is lower than the currently running core version, a
compatibility warning is displayed.

Installing a package from the detail modal or card triggers the marketplace
install flow. On success, an inline notice appears with a **View in Skills**
deep link that navigates to the Skills panel and highlights the newly loaded
skill. If the backend reports a `reload_required` flag, the skill index is
refreshed automatically.

### Installed Tab

The Installed tab lists all currently installed marketplace packages as
per-package cards, showing:
- Package name and version
- DCC type
- Install type (git, zip, path)
- Uninstall button

Users can uninstall a package directly from this tab. On success, the skill
index is refreshed to reflect the removed package.

### API Endpoints

| Route | Content-Type | Description |
|-------|-------------|-------------|
| `GET /admin/api/marketplace/catalog` | `application/json` | List available packages from all configured sources. Returns `{ entries: MarketplaceEntry[] }`. |
| `GET /admin/api/marketplace/installed` | `application/json` | List locally installed packages with per-DCC grouping. Returns `{ packages: InstalledMarketplacePackage[] }`. |
| `POST /admin/api/marketplace/install` | `application/json` | Install a package. Body: `{ name, dcc, source?, force? }`. Returns `InstallResultResponse` with `reload_required` flag. |
| `POST /admin/api/marketplace/uninstall` | `application/json` | Uninstall a package. Body: `{ name, dcc }`. Returns `UninstallResultResponse` with `reload_required` flag. |
| `GET /admin/api/marketplace/sources` | `application/json` | List configured marketplace sources with origin (builtin, config, env). |
| `POST /admin/api/marketplace/sources` | `application/json` | Add a marketplace source to the persistent config. Body: `{ source }`. Duplicate additions are idempotent. |
| `GET /admin/api/marketplace/outdated` | `application/json` | List installed packages that have newer versions available. Supports `?name=&dcc=` filters. |
| `POST /admin/api/marketplace/update` | `application/json` | Update one or all outdated packages. Body: `{ name?, dcc?, all? }`. Returns per-result `reload_required` flags. |

## Routes

| Route | Content-Type | Description |
|-------|-------------|-------------|
| `GET /admin` | `text/html` | Embedded React/Vite dashboard served as one HTML asset |
| `GET /admin/api/activity?limit=300` | `application/json` | Unified activity timeline built from audits, traces, and gateway events |
| `GET /admin/api/instances` | `application/json` | Connected DCC instances |
| `GET /admin/api/tools` | `application/json` | Registered MCP tools |
| `GET /admin/api/workflows?limit=200` | `application/json` | Agent session/workflow view reconstructed from search telemetry, traces, and audits |
| `GET /admin/api/tasks?limit=300` | `application/json` | User-level task outcomes grouped across workflows, calls, artifacts, and validation |
| `GET /admin/api/calls` | `application/json` | Recent tool calls, including compact/JSON token accounting when available (requires `AuditMiddleware`) |
| `GET /admin/api/traces` | `application/json` | Recent per-call dispatch traces with payload sizes and token accounting; accepts `?limit=200` |
| `GET /admin/api/traces/{request_id}` | `application/json` | Full waterfall for one recorded dispatch trace, including token accounting without storing unbounded payloads |
| `GET /admin/api/traffic?limit=300` | `application/json` | `capture_status` plus retained metadata-only `traffic.frame` envelopes from an explicit `admin_live` traffic sink |
| `GET /admin/api/traffic/export?limit=1000` | `application/x-ndjson` | Retained metadata-only traffic frames as JSONL for safe local inspection/diff workflows |
| `GET /admin/api/debug-bundle/{request_id}` | `application/json` | One-stop debug bundle containing the trace, matching audit row, related activity, and hints |
| `GET /admin/api/stats?range=1h\|24h\|7d` | `application/json` | Aggregated call counts, success rate, latency, top tools/instances/agents, and token-savings totals |
| `GET /admin/api/governance?limit=300` | `application/json` | Effective gateway policy, traffic capture, redaction, middleware controls, and recent allow/deny/throttle decisions |
| `GET /admin/api/workers` | `application/json` | Per-instance cards from the live registry; response field names remain `workers` for compatibility |
| `GET /admin/api/logs` | `application/json` | Merged gateway contention events, on-disk `*.log` rows, and audited call summaries |
| `GET /admin/api/health` | `application/json` | Service health summary, including active response-format defaults and token estimator metadata |
| `GET /admin/api/skills` | `application/json` | Live skill inventory grouped by DCC type, skill name, load state, tools, backend instance, and skill health/adoption metrics from search telemetry plus audited calls |
| `GET /admin/api/skill-detail?name=...` | `application/json` | One skill's backend detail payload, including rendered-review `SKILL.md` markdown when available |
| `GET /admin/api/skill-paths` | `application/json` | Current skill discovery roots with safe path aliases, present/missing status, and source/id metadata for public-safe screenshots and exports |
| `POST /admin/api/skill-paths` | `application/json` | Add a SQLite-backed custom skill discovery root, then refresh live backend skill indexes |
| `DELETE /admin/api/skill-paths/{id}` | `application/json` | Remove a SQLite-backed custom skill discovery root, then refresh live backend skill indexes |
| `GET /admin/api/integrations` | `application/json` | Read-only integration configuration summary: Sentry DSN status, webhook count, OTLP endpoint, and pending-restart flags |

Stable agent-facing mirrors are exposed under `/v1/debug/*` and are included in
`GET /v1/openapi.json`. The Admin routes above remain the dashboard
compatibility layer; automation should prefer:

| Stable route | Mirrors |
|--------------|---------|
| `GET /v1/debug/instances` | `/admin/api/instances` |
| `GET /v1/debug/activity?limit=300` | `/admin/api/activity` |
| `GET /v1/debug/traces?limit=200` | `/admin/api/traces` |
| `GET /v1/debug/traces/{request_id}` | `/admin/api/traces/{request_id}` |
| `GET /v1/debug/traffic?limit=300` | `/admin/api/traffic` |
| `GET /v1/debug/traffic/export?limit=1000` | `/admin/api/traffic/export` |
| `GET /v1/debug/trace-context/{lookup_id}` | trace id or request id lookup |
| `GET /v1/debug/agent-traces/{lookup_id}` | public-safe agent trace packet by trace id or request id |
| `GET /v1/debug/bundles/{request_id_or_trace_id}` | `/admin/api/debug-bundle/{request_id}` |
| `GET /v1/debug/issue-reports/{request_id}` | `/admin/api/issue-report/{request_id}`; public-safe by default, `?mode=raw` for reviewed local evidence |
| `GET /v1/debug/workflows` | `/admin/api/workflows` |
| `GET /v1/debug/tasks` | `/admin/api/tasks` |
| `GET /v1/debug/calls` | `/admin/api/calls` |
| `GET /v1/debug/logs` | `/admin/api/logs` |
| `GET /v1/debug/stats` | `/admin/api/stats` |
| `GET /v1/debug/governance?limit=300` | `/admin/api/governance` |
| `GET /v1/debug/health` | `/admin/api/health` |

Browser deep links such as `/admin?panel=traces&trace=<request_id>` are UI
navigation only. Historical `/admin?agent=traces&trace=<id>` links should be
treated the same way; automation and agents should resolve the machine-readable
packet through `GET /v1/debug/agent-traces/{lookup_id}`.

Compact-aware debug routes keep JSON as the default for browser downloads and
GitHub issue attachments. Agents can request TOON on `/v1/debug/traces`,
`/v1/debug/traces/{request_id}`, `/v1/debug/trace-context/{lookup_id}`,
`/v1/debug/bundles/{request_id_or_trace_id}`, and `/v1/debug/stats` with
`Accept: application/toon`, `?response_format=toon`, or `?compact=true`.
Responses include `x-dcc-mcp-response-format`, byte counts, estimated token
counts, and savings headers. Debug bundle compact output is a summary with root
cause, tool, DCC type, status, timing, token accounting, redaction summary, and
links to the full JSON bundle.

## Optional Agent / Caller Context

MCP and REST callers may attach optional context so the Admin UI can correlate
why a request was made with the request waterfall. This is a telemetry contract:
callers should send concise summaries, plans, observations, tags, and correlation
ids. The gateway does not attempt to capture hidden model chain-of-thought, raw
user input, or raw agent replies on the default path.

Supported carriers:

- MCP `initialize` `params.clientInfo`; the gateway keeps bounded client
  identity per `Mcp-Session-Id` and fills missing agent/client fields on later
  MCP `tools/call` rows.
- MCP `tools/call` `params._meta.agent_context`
- REST body `agent_context`, `agentContext`, `caller_context`, or
  `meta.agent_context`
- Headers such as `x-dcc-mcp-actor-id`, `x-dcc-mcp-actor-name`,
  `x-dcc-mcp-actor-email-hash`, `x-dcc-mcp-agent-id`,
  `x-dcc-mcp-agent-name`, `x-dcc-mcp-agent`, `x-dcc-mcp-agent-kind`,
  `x-dcc-mcp-agent-version`, `x-dcc-mcp-agent-model`,
  `x-dcc-mcp-agent-model-provider`, `x-dcc-mcp-agent-model-version`,
  `x-dcc-mcp-agent-reasoning-effort`, `x-dcc-mcp-client-platform`,
  `x-dcc-mcp-client-os`, `x-dcc-mcp-client-host`,
  `x-dcc-mcp-auth-subject`, `x-dcc-mcp-agent-session-id`,
  `x-dcc-mcp-agent-turn-id`, `x-dcc-mcp-agent-user-intent-summary`,
  `x-dcc-mcp-agent-reply-summary`, `x-dcc-mcp-agent-user-input-hash`,
  `x-dcc-mcp-agent-reply-hash`, `x-dcc-mcp-agent-user-input-chars`,
  `x-dcc-mcp-agent-reply-chars`, `x-dcc-mcp-agent-task`,
  `x-dcc-mcp-reasoning-summary`, `x-dcc-mcp-parent-request-id`, and
  `x-dcc-mcp-agent-context` (JSON object)
- REST `User-Agent`; when no explicit `client_platform` is provided, the first
  product token is stored as a bounded client-platform fallback.

Caller attribution separates five concepts:

| Concept | Fields | Notes |
| --- | --- | --- |
| Human or service actor | `actor_id`, `actor_name`, `actor_email_hash` | Optional, bounded identifiers. Hash email addresses before sending them. |
| Agent runtime | `agent_id`, `agent_name`, `agent_kind`, `agent_version`, `model`, `model_provider`, `model_version` | `model` also accepts `agentModel`; `model_version` also accepts `agentModelVersion`. |
| Client platform | `client_platform`, `client_os`, `client_host` | Examples: `cursor`, `claude-desktop`, `openclaw`, `clawhub`, `custom-http`, `studio-tool`. |
| Auth subject | `auth_subject` | API-key, bearer-token, OAuth, or local identity subject after authentication. |
| Network source | `source_ip`, `forwarded_for` | Server-derived only. MCP `_meta`, REST request bodies, and caller headers cannot set these fields. |

Stored `agent_context` values include a server-computed `trust` map, and Admin
call/trace rows expose the same data as `attribution_trust`. Trust values are:

| Trust value | Meaning |
| --- | --- |
| `self_reported` | Supplied by REST body or MCP `_meta`; useful for filtering, not identity proof. |
| `header` | Supplied by `x-dcc-mcp-*` attribution headers; still self-asserted unless a trusted proxy/auth layer owns those headers. |
| `auth` | Derived from gateway authentication or an identity-provider integration. |
| `server_derived` | Derived by the gateway from the socket peer after stripping caller-supplied network fields. |
| `trusted_proxy` | Derived from `Forwarded` / `X-Forwarded-For` after the configured trusted-proxy depth has been applied. |

Cursor-like MCP clients can place attribution in `_meta.agent_context`, custom
REST clients can use `meta.agent_context`, and LAN studio tools that cannot
shape JSON bodies can use the `x-dcc-mcp-*` headers above. Do not send hidden
reasoning, full prompts, raw user messages, secrets, bearer tokens, or raw agent
replies in any caller-attribution field. LAN operators must not treat
`self_reported` or `header` actor fields as access control; use gateway auth or
a trusted proxy/identity provider before relying on actor metadata for
permissions. Raw actor email is not a supported field; send `actor_email_hash`
only after hashing or otherwise pseudonymizing it.

Example REST request:

```json
{
  "tool_slug": "maya.abcdef01.scene__inspect",
  "arguments": { "include_materials": true },
  "meta": {
    "agent_context": {
      "actor_id": "artist-42",
      "actor_name": "Morgan Artist",
      "actor_email_hash": "sha256:...",
      "agent_id": "agent-42",
      "agent_name": "Layout Inspector",
      "agent_kind": "coding-agent",
      "agent_version": "0.9.0",
      "client_platform": "custom-http",
      "client_os": "windows",
      "client_host": "workstation-42",
      "auth_subject": "apikey:team-layout",
      "model_provider": "openai",
      "model_version": "gpt-5.1",
      "model": "gpt-5.4",
      "reasoning_effort": "medium",
      "session_id": "session-42",
      "turn_id": "turn-7",
      "task": "Find the cheapest scene inspection path before editing",
      "user_intent_summary": "User asked for a non-destructive scene inspection before editing.",
      "agent_reply_summary": "The agent will inspect topology and material counts first.",
      "user_input_hash": "sha256:...",
      "agent_reply_hash": "sha256:...",
      "user_input_chars": 128,
      "agent_reply_chars": 192,
      "reasoning_summary": "Need scene topology and material counts before selecting an edit tool.",
      "plan": ["inspect scene", "choose edit target"],
      "observations": ["user asked for non-destructive update"],
      "parent_request_id": "req-parent"
    }
  }
}
```

Admin list rows expose `transport`, `agent_id`, `agent_name`, `agent_model`,
`trace_id`, `span_id`, `parent_span_id`, span counts, payload byte counts,
slowest span summaries, and a `links` object
with absolute URLs for the Admin trace page, trace API, debug bundle, issue
report JSON, OpenAPI Inspector, OpenAPI spec, OpenAPI docs, and stats page.
Full trace rows include `agent_context`, request/response payload previews, a
span waterfall, and the same copyable links. These URLs are designed to be
pasted directly into an LLM evaluation prompt or another agent's debugging task.
The Tasks panel and `GET /admin/api/tasks` group retained trace/audit data into
user-level outcomes before rendering cards. A task row keeps the historical
`task_id`, `task_type`, `status`, `title`, `started_at`, `duration_ms`, and
`correlation` fields, then adds outcome-oriented fields such as `goal`,
`summary`, `final_result`, `failure_reason`, `app_types`, `related`,
`artifacts`, `validation_checks`, and navigation `links`. Grouping prefers an
explicit `agent_context.metadata.task_id`/`workflow_id`, then agent
session/turn, session id, trace id, and finally request id. Public task rows
must not expose raw local paths or loopback/private callback URLs; failure
reasons and artifact labels stay summarized or redacted.

The Workflows panel and `GET /admin/api/workflows` group the same bounded data
by session, explicit workflow id, trace id, or request chain. Each workflow row
stays lightweight: model identity, turn id, user/agent summaries, discovery
quality, and a compact stage preview. Selecting a workflow opens a staged detail
graph with Intent, Discovery, Skill Load, Tool Calls, Fallbacks, Artifacts,
Validation, and Report nodes. Node details expose timestamps, DCC app/instance,
transport, request/parent ids, search telemetry, and trace/debug links so an
operator can identify fallback scripting or the failed stage without reading raw
call logs.
Raw prompts and raw replies are high-sensitivity data: keep them out of
`agent_context`; use only an explicitly configured traffic capture policy with
redaction, sampling, retention, and operator visibility when raw text is needed
for a private investigation. Traffic capture also treats actor/user/platform
metadata as potentially sensitive: built-in redaction masks common attribution
identity fields, and capture configs can add explicit `redact:` rules for
deployment-specific metadata paths.

`request_id` and `trace_id` are intentionally different. `request_id` identifies
one HTTP/MCP request (or JSON-RPC id), while `trace_id` identifies the
end-to-end unit of work. REST callers may send both `X-Request-Id` and W3C
`traceparent`; the gateway keeps `X-Request-Id` as the request id and parses the
trace id, parent span id, and flags from `traceparent`.

The Admin UI also exposes a standalone `GET /admin/api/issue-report/{request_id}`
export. It returns a public-safe GitHub issue report by default with summary,
status, DCC type, tool family, timing, sanitized error kind, token accounting,
redaction status, and relative same-session links. Default exports intentionally
exclude raw payload previews, prompts, scripts, auth material, local callback
URLs, absolute filesystem paths, and private scene/project identifiers. Use
`GET /admin/api/issue-report/{request_id}?mode=raw` only for reviewed local
evidence; raw mode embeds the correlated debug bundle and should not be pasted
into a public issue without inspection.

The front-end product name is **Admin Dashboard**. The lower REST/OpenAPI
contract view is named **OpenAPI Inspector**. It reads the live gateway
`/v1/openapi.json` contract by default, can load a per-instance OpenAPI
contract from `?panel=openapi&spec=...&docs=...&label=...`, and links to the
matching Scalar reference.

Admin token fields intentionally separate two accounting models:

- `payload_token_usage`, trace `input_tokens`/`output_tokens`, and
  `payload_token_accounting` are deterministic payload-preview estimates from
  captured request/response bodies. Missing payload estimates are reported as
  `missing_payload_tokens`, not silently treated as true zero-token payloads.
- `token_usage`, `response_token_accounting`, and per-call
  `original_tokens`/`returned_tokens`/`saved_tokens` describe response-format
  accounting: how many response tokens were returned after JSON/TOON
  compaction, and how many were saved.

## API Response Shapes

```json
// GET /admin/api/health
{
  "status": "ok",
  "uptime_secs": 3600,
  "instances_total": 3,
  "instances_ready": 2,
  "response_format": {
    "default": "toon",
    "legacy_mime": "application/json",
    "compact_mime": "application/toon",
    "token_estimator": "dcc-mcp-byte4-v1"
  }
}

// GET /admin/api/instances
{
  "total": 3,
  "instances": [
    { "id": "a1b2c3d4-...", "dcc_type": "maya", "status": "ready", "address": "127.0.0.1:9001" }
  ]
}

// GET /admin/api/activity?limit=300
{
  "total": 2,
  "events": [
    {
      "event_id": "audit:req-123",
      "timestamp": "2026-05-05T10:00:00Z",
      "kind": "tool_call",
      "severity": "info",
      "status": "ok",
      "message": "tools/call maya__open_scene",
      "tool": "maya__open_scene",
      "duration_ms": 48,
      "correlation": {
        "request_id": "req-123",
        "session_id": "session-1",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/tasks?limit=300
{
  "total": 1,
  "tasks": [
    {
      "task_id": "session-1:turn-7",
      "task_type": "agent_turn",
      "status": "completed",
      "title": "Export and validate shot asset",
      "goal": "Create scene, export asset, import into another DCC, validate result.",
      "final_result": "Produced preview render and validation report.",
      "started_at": "2026-05-05T10:00:00Z",
      "finished_at": "2026-05-05T10:00:08Z",
      "duration_ms": 8000,
      "app_types": ["maya", "blender"],
      "artifacts": [
        { "kind": "export", "name": "export asset", "request_id": "req-export" },
        { "kind": "render", "name": "render preview", "request_id": "req-render" }
      ],
      "validation_checks": [
        { "title": "validate imported asset", "status": "completed", "request_id": "req-validate" }
      ],
      "related": {
        "workflow_ids": ["session-1"],
        "request_ids": ["req-create", "req-export", "req-import", "req-render", "req-validate"],
        "trace_ids": ["trace-123"],
        "session_ids": ["session-1"]
      },
      "correlation": {
        "request_id": "req-validate",
        "workflow_id": "session-1",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/workflows?limit=200
{
  "total": 1,
  "workflows": [
    {
      "workflow_id": "session-1",
      "group_kind": "session",
      "title": "Layout Inspector: maya.abcdef01.scene__inspect",
      "status": "completed",
      "discovery": {
        "search_count": 1,
        "zero_result_count": 0,
        "selected_count": 3,
        "best_selected_rank": 2,
        "time_to_first_success_ms": 310,
        "search_ids": ["search-123"]
      },
      "steps": [
        { "kind": "search", "title": "search scene inspect", "status": "ok" },
        { "kind": "describe", "title": "maya.abcdef01.scene__inspect", "status": "ok" },
        { "kind": "load_skill", "title": "load_skill scene", "status": "ok" },
        { "kind": "call", "request_id": "req-123", "title": "maya.abcdef01.scene__inspect", "status": "ok" }
      ]
    }
  ]
}

// GET /admin/api/calls  (requires AuditMiddleware)
{
  "total": 42,
  "calls": [
    {
      "request_id": "req-123",
      "method": "tools/call",
      "tool": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "session_id": "session-1",
      "transport": "mcp",
      "agent_id": "agent-42",
      "agent_name": "Layout Inspector",
      "agent_model": "gpt-5.4",
      "links": {
        "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
        "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
        "agent_trace_packet_url": "http://127.0.0.1:9765/v1/debug/agent-traces/req-123",
        "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123",
        "issue_report_url": "http://127.0.0.1:9765/admin/api/issue-report/req-123",
        "openapi_inspector_url": "http://127.0.0.1:9765/admin?panel=openapi",
        "openapi_spec_url": "http://127.0.0.1:9765/v1/openapi.json",
        "openapi_docs_url": "http://127.0.0.1:9765/docs",
        "stats_url": "http://127.0.0.1:9765/admin?panel=stats"
      },
      "token_accounting": {
        "response_format": "toon",
        "token_estimator": "dcc-mcp-byte4-v1",
        "original_tokens": 120,
        "returned_tokens": 54,
        "saved_tokens": 66,
        "savings_pct": 55.0
      },
      "success": false,
      "error": "backend timeout",
      "timestamp": "2026-05-05T10:00:00Z"
    }
  ]
}

// GET /admin/api/traces?limit=200
{
  "total": 1,
  "traces": [
    {
      "request_id": "req-123",
      "tool": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "transport": "mcp",
      "agent_id": "agent-42",
      "span_count": 3,
      "slowest_span_name": "backend.execute",
      "slowest_span_ms": 45,
      "input_bytes": 42,
      "output_bytes": 96,
      "token_accounting": {
        "response_format": "json",
        "token_estimator": "dcc-mcp-byte4-v1",
        "original_tokens": 24,
        "returned_tokens": 24,
        "saved_tokens": 0,
        "savings_pct": 0.0
      },
      "links": {
        "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
        "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
        "agent_trace_packet_url": "http://127.0.0.1:9765/v1/debug/agent-traces/req-123",
        "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123",
        "issue_report_url": "http://127.0.0.1:9765/admin/api/issue-report/req-123",
        "openapi_inspector_url": "http://127.0.0.1:9765/admin?panel=openapi",
        "openapi_spec_url": "http://127.0.0.1:9765/v1/openapi.json",
        "openapi_docs_url": "http://127.0.0.1:9765/docs",
        "stats_url": "http://127.0.0.1:9765/admin?panel=stats"
      },
      "total_ms": 48,
      "success": true,
      "status": "ok"
    }
  ]
}

// GET /admin/api/traces/req-123
{
  "request_id": "req-123",
  "method": "tools/call",
  "tool_slug": "maya.abcdef01.maya__open_scene",
  "dcc_type": "maya",
  "transport": "mcp",
  "agent_context": {
    "agent_id": "agent-42",
    "agent_name": "Layout Inspector",
    "model": "gpt-5.4",
    "reasoning_summary": "Need scene topology before editing."
  },
  "links": {
    "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
    "trace_api_url": "http://127.0.0.1:9765/admin/api/traces/req-123",
    "agent_trace_packet_url": "http://127.0.0.1:9765/v1/debug/agent-traces/req-123",
    "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123",
    "issue_report_url": "http://127.0.0.1:9765/admin/api/issue-report/req-123",
    "openapi_inspector_url": "http://127.0.0.1:9765/admin?panel=openapi",
    "openapi_spec_url": "http://127.0.0.1:9765/v1/openapi.json",
    "openapi_docs_url": "http://127.0.0.1:9765/docs",
    "stats_url": "http://127.0.0.1:9765/admin?panel=stats"
  },
  "total_ms": 48,
  "ok": true,
  "spans": [
    { "name": "backend.execute", "duration_ns": 45000000, "ok": true, "attributes": {} }
  ],
  "input": { "mime_type": "application/json", "truncated": false, "original_size": 42, "content": "{...}" },
  "output": { "mime_type": "application/json", "truncated": false, "original_size": 96, "content": "{...}" }
}

// GET /v1/debug/agent-traces/req-123
{
  "schema_version": "dcc-mcp.admin.agent-trace-packet.v1",
  "lookup_id": "req-123",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "request_id": "req-123",
  "request_ids": ["req-123"],
  "status": "ok",
  "tool": "maya.abcdef01.maya__open_scene",
  "dcc_type": "maya",
  "transport": "mcp",
  "total_ms": 48,
  "span_count": 1,
  "payload_tokens": {
    "token_estimator": "dcc-mcp-byte4-v1",
    "input_tokens": 11,
    "output_tokens": 24,
    "total_tokens": 35,
    "missing_payload_tokens": false
  },
  "response_token_accounting": {
    "response_format": "json",
    "returned_tokens": 24,
    "saved_tokens": 0
  },
  "postmortem": {
    "previous_call_count": 0,
    "gateway_event_count": 0
  },
  "links": {
    "admin_trace_url": "http://127.0.0.1:9765/admin?panel=traces&trace=req-123",
    "agent_trace_packet_url": "http://127.0.0.1:9765/v1/debug/agent-traces/req-123",
    "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123",
    "issue_report_url": "http://127.0.0.1:9765/admin/api/issue-report/req-123"
  },
  "privacy_note": "Agent trace packets omit request/response payload previews, prompts, scripts, and scene data. Use debug_bundle_url only for reviewed local diagnostics."
}

// GET /admin/api/debug-bundle/req-123
{
  "request_id": "req-123",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "request_ids": ["req-123"],
  "trace": { "request_id": "req-123", "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736", "spans": [] },
  "traces": [{ "request_id": "req-123", "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736" }],
  "audit": { "request_id": "req-123", "success": true },
  "audits": [{ "request_id": "req-123", "success": true }],
  "related_activity": [],
  "postmortem": {
    "target": { "request_id": "req-123", "tool": "maya.abcdef01.maya__open_scene" },
    "previous_calls": [
      {
        "request_id": "req-122",
        "tool": "maya.abcdef01.maya__save_scene",
        "ok": true,
        "input": { "mime_type": "application/json", "truncated": false, "content": "{...}" }
      }
    ],
    "gateway_events": []
  },
  "links": {
    "agent_trace_packet_url": "http://127.0.0.1:9765/v1/debug/agent-traces/req-123",
    "debug_bundle_url": "http://127.0.0.1:9765/admin/api/debug-bundle/req-123",
    "issue_report_url": "http://127.0.0.1:9765/admin/api/issue-report/req-123",
    "openapi_inspector_url": "http://127.0.0.1:9765/admin?panel=openapi",
    "openapi_spec_url": "http://127.0.0.1:9765/v1/openapi.json",
    "openapi_docs_url": "http://127.0.0.1:9765/docs"
  },
  "hints": []
}

// GET /admin/api/issue-report/req-123
{
  "schema_version": "dcc-mcp.admin.issue-report.v1",
  "report_type": "github_issue_public_safe",
  "privacy_mode": "public-safe",
  "request_id": "req-123",
  "summary": {
    "title": "DCC-MCP request req-123 failed: open_scene",
    "status": "failed",
    "dcc_type": "maya",
    "tool_family": "open_scene",
    "total_ms": 48,
    "error": {
      "kind": "backend-unavailable",
      "present": true,
      "message_redacted": true
    },
    "response_token_accounting": {
      "response_format": "toon",
      "token_estimator": "dcc-mcp-byte4-v1",
      "returned_tokens": 54,
      "saved_tokens": 66,
      "savings_pct": 55.0
    },
    "token_accounting": {
      "response_format": "toon",
      "token_estimator": "dcc-mcp-byte4-v1",
      "returned_tokens": 54,
      "saved_tokens": 66,
      "savings_pct": 55.0
    },
    "payload_tokens": {
      "kind": "payload",
      "token_estimator": "dcc-mcp-byte4-v1",
      "input_tokens": null,
      "output_tokens": null,
      "total_tokens": null,
      "missing_payload_tokens": true
    },
    "token_accounting_contract": {
      "payload_tokens": "request and response payload preview estimates",
      "response_token_accounting": "response-format original/returned/saved response tokens"
    },
    "redaction_status": {
      "mode": "public-safe",
      "raw_payloads_excluded": true,
      "payload_previews_excluded": true,
      "local_urls_excluded": true,
      "absolute_paths_excluded": true,
      "private_identifiers_excluded": true
    },
    "postmortem": {
      "previous_call_count": 1,
      "gateway_event_count": 0
    }
  },
  "github_issue": {
    "title": "DCC-MCP request req-123 failed: open_scene",
    "body_template": "## Summary\n\nRequest `req-123` returned `failed`...",
    "suggested_labels": ["bug", "admin-telemetry"]
  },
  "links": {
    "admin_trace_path": "/admin?panel=traces&trace=req-123",
    "agent_trace_packet_path": "/v1/debug/agent-traces/req-123",
    "safe_issue_report_path": "/admin/api/issue-report/req-123",
    "raw_issue_report_path": "/admin/api/issue-report/req-123?mode=raw",
    "stable_safe_issue_report_path": "/v1/debug/issue-reports/req-123",
    "stable_raw_issue_report_path": "/v1/debug/issue-reports/req-123?mode=raw",
    "openapi_spec_path": "/v1/openapi.json",
    "docs_path": "/docs"
  },
  "raw_debug_bundle": {
    "available": true,
    "mode_query": "mode=raw",
    "admin_path": "/admin/api/issue-report/req-123?mode=raw",
    "stable_path": "/v1/debug/issue-reports/req-123?mode=raw"
  }
}

// GET /admin/api/issue-report/req-123?mode=raw
{
  "schema_version": "dcc-mcp.admin.issue-report.v1",
  "report_type": "github_issue_debug_json",
  "privacy_mode": "raw-local-evidence",
  "request_id": "req-123",
  "debug_bundle": { "request_id": "req-123" }
}

// GET /admin/api/stats?range=24h
{
  "range": "24h",
  "total_calls": 42,
  "success_rate": 0.98,
  "latency_ms": { "p50_ms": 12, "p95_ms": 48 },
  "top_agents": [{ "name": "Layout Inspector", "count": 12 }],
  "payload_token_usage": {
    "token_estimator": "dcc-mcp-byte4-v1",
    "total_input_tokens": 1200,
    "total_output_tokens": 900,
    "total_tokens": 2100,
    "calls_with_any_payload_tokens": 21,
    "calls_missing_payload_tokens": 21,
    "avg_total_tokens_per_call": 50.0,
    "avg_total_tokens_per_recorded_call": 100.0
  },
  "token_usage": {
    "total_original_tokens": 7500,
    "total_returned_tokens": 5400,
    "total_saved_tokens": 2100,
    "average_savings_pct": 28.0,
    "by_tool": [
      { "name": "maya.a1b2.render_preview", "calls": 8, "returned_tokens": 900, "saved_tokens": 700, "savings_pct": 43.75 }
    ],
    "by_instance": [
      { "name": "maya-a1b2", "calls": 14, "returned_tokens": 1800, "saved_tokens": 900, "savings_pct": 33.33 }
    ],
    "by_agent": [
      { "name": "Layout Inspector", "calls": 12, "returned_tokens": 1500, "saved_tokens": 820, "savings_pct": 35.34 }
    ],
    "by_transport": [
      { "name": "rest", "calls": 18, "returned_tokens": 2500, "saved_tokens": 1300, "savings_pct": 34.21 }
    ],
    "by_response_format": [
      { "name": "toon", "calls": 24, "returned_tokens": 3200, "saved_tokens": 2100, "savings_pct": 39.62 },
      { "name": "json", "calls": 18, "returned_tokens": 2200, "saved_tokens": 0, "savings_pct": 0.0 }
    ]
  }
}

// GET /admin/api/governance?limit=300
{
  "schema_version": "dcc-mcp.admin.governance.v1",
  "mode": {
    "admin_mutations": "disabled",
    "reason": "Admin is unauthenticated and read-only by default."
  },
  "policy": {
    "read_only": true,
    "unrestricted": false,
    "allowlists_active": { "dcc_types": true, "tool_slug_prefixes": true },
    "allowed_dcc_types": ["maya", "photoshop"],
    "allowed_tool_slug_prefixes": ["maya.a1b2"]
  },
  "traffic_capture": {
    "enabled": true,
    "mode": "aggregate",
    "production_guardrail": "capture only safe aggregate data unless explicitly configured",
    "redaction": { "paths": ["body.data.params.arguments.api_key"], "redacted_total": 8 }
  },
  "middleware": {
    "controls": [
      { "kind": "quota", "mode": "rate-limit", "summary": "100 calls / 60s" }
    ]
  },
  "stats": { "recent_allowed": 1200, "recent_policy_denied": 4, "recent_throttled": 3, "redacted_path_count": 8 },
  "recent_decisions": [
    {
      "request_id": "req-123",
      "outcome": "throttled",
      "tool": "maya.a1b2.scene__inspect",
      "traffic_capture": { "captured": 0, "skipped": 1, "reasons": ["filtered-by-rule"] },
      "privacy": { "redacted_paths": [] },
      "pressure": { "quota_active": true, "throttled": true }
    }
  ]
}

// GET /admin/api/workers
{
  "summary": { "live": 2, "stale": 0, "unhealthy": 0 },
  "workers": [
    { "instance_id": "a1b2c3d4-...", "dcc_type": "maya", "status": "available" }
  ]
}

// GET /admin/api/logs
{
  "total": 5,
  "logs": [
    {
      "timestamp": "2026-05-05T09:59:00Z",
      "level": "info",
      "message": "tools/call ok 12ms — maya__open_scene",
      "source": "audit",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "request_id": "req-123",
      "tool": "maya__open_scene",
      "success": true,
      "detail": "instance=abcdef01-2345-6789-abcd-ef0123456789"
    }
  ]
}
```

## Connecting AuditMiddleware

For the `/admin/api/calls` feed to be populated, add `AuditMiddleware` to the middleware chain:

```rust
use dcc_mcp_gateway::gateway::middleware::{AuditMiddleware, MiddlewareChain};

GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default())),
    ..GatewayConfig::default()
}
```

The `/admin/api/logs` feed is populated automatically from three bounded sources: the `EventLog` ring buffer (gateway election/eviction/probe events from issue #766), `*.log` files under `DCC_MCP_LOG_DIR` or the platform default log directory, and recent `AuditMiddleware` call rows. The `/admin/api/traces`, `/admin/api/stats`, and `/admin/api/workers` endpoints are populated from the dispatch `TraceLog`, `StatsAggregator`, and live gateway registry respectively.

Set `DCC_MCP_GATEWAY_AUDIT_DIR` to enable durable JSONL persistence. The gateway appends bounded admin call rows to `audit.jsonl` and dispatch traces to `traces.jsonl`, trims each file to both `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` and `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES`, and seeds the in-memory admin buffers from those files on restart. Payloads remain the same bounded/redacted `TracePayload` values used by the in-memory trace capture; persistence does not store unbounded raw request bodies.

## Dashboard Features

The HTML dashboard includes:
- **Debug Workbench**: the default first screen combines health, instances, calls, traces, stats, warning logs, and per-instance OpenAPI entry points so operators can triage gateway failures without jumping between panels.
- **Gateway owner identity**: the Health and Debug panels show the current `__gateway__` sentinel label from `gateway_name` / `DCC_MCP_GATEWAY_NAME`, plus any challenger candidates.
- **Left navigation**: Debug / Activity / Health / Instances / Tools / Tasks / OpenAPI Inspector / Calls / Traces / Stats / Skill paths / Integrations / Logs panels
- **Auto-refresh**: Panels poll their JSON endpoints every 5 seconds
- **DCC icons**: common hosts such as Maya/Autodesk, Blender, GIMP, Inkscape, Krita, Unity, and Unreal get recognizable icons, with a safe fallback for custom hosts.
- **Instance cards**: Per-instance status, heartbeat, and routing metadata
- **OpenAPI Inspector**: summarizes the gateway or selected instance `/v1/openapi.json` contract, filters REST operations by method/path/tag, and exposes copy/download links for the raw JSON plus the matching `/docs`.
- **Instance OpenAPI links**: Debug Workbench and instance cards expose `Inspector`, `spec`, and `docs` links generated from each backend `mcp_url`, so an operator can jump from MCP-level telemetry to the lower OpenAPI contract for that exact backend.
- **Calls table**: request ids, error previews, and trace-detail links; DCC is displayed from the resolved backend slug when available, otherwise from explicit call arguments such as `dcc` / `dcc_type`.
- **Trace drill-down**: `/admin/api/traces/{request_id}` exposes the full waterfall, optional agent/caller context, and bounded/redacted input/output payloads for one call.
- **Traffic panel**: `/admin/api/traffic` always reports `capture_status.state`
  so operators can distinguish genuine no traffic from `capture_disabled`,
  `capture_unavailable`, or filtered capture. When a traffic config includes
  `kind: admin_live`, the endpoint exposes the retained in-memory frame ring as
  metadata-only frames; `body.data` is omitted from the admin API/export by
  default while method, route, request/trace/session ids, sizes, skip reasons,
  and redaction paths remain visible.
- **Governance panel**: shows read-only state, allowlists, traffic capture mode/sinks, production guardrails, redaction path summaries, middleware rate-limit controls, and recent allowed/denied/throttled/capture decisions.
- **Logs panel**: groups normalized `contention`, `file`, and `audit` rows so operators can correlate routing events, rolling files, and tool calls in one timeline. File log reads are bounded to recent files and tail slices so the admin API does not scan unbounded historical logs.
- **Integrations panel**: read-only summary of enabled third-party integrations — Sentry DSN status (set/unset), active webhook count and name list, OTLP endpoint, and any pending-restart flags for configuration changes that require a server restart to take effect.
- **Durable audit option**: `DCC_MCP_GATEWAY_AUDIT_DIR` preserves the Calls and Traces panels across restarts without changing the JSON API shapes.
- **Dark theme**: Vite/React source with embedded runtime asset and no required runtime build step
- **Responsive**: narrow screens switch to a top navigation rail, and debug cards/charts keep a usable single-column width.

## Integrations Panel

The Integrations panel (`GET /admin/api/integrations`) displays a read-only
summary of the gateway's third-party integration configuration. The panel shows
which integrations are active and whether a server restart is pending for
environment-provided settings.

| Integration | Configuration mechanism | Admin panel shows |
|-------------|------------------------|-------------------|
| Sentry | `DCC_MCP_SENTRY_DSN` env var | DSN status (set/unset), environment, sample rate |
| Webhooks | `DCC_MCP_WEBHOOKS_CONFIG` env var → YAML file | Active webhook names, event patterns, and delivery stats |
| OTLP tracing | `OTEL_EXPORTER_OTLP_ENDPOINT` env var | Endpoint URL, service name, span sample rate |

The panel is **read-only** by design — all three integrations are configured
through environment variables or config files applied at server startup. The
panel flags pending-restart changes when the operator modifies an env var
through the host deployment tooling but has not yet restarted the gateway
process. See [gateway.md](gateway.md) for full configuration reference.

### Backend API

```json
// GET /admin/api/integrations
{
  "sentry": {
    "configured": true,
    "dsn_prefix": "https://***@o***.ingest.sentry.io",
    "environment": "production",
    "sample_rate": 1.0,
    "pending_restart": false
  },
  "webhooks": {
    "configured": true,
    "config_path": "/etc/dcc-mcp/webhooks.yaml",
    "active_webhooks": 2,
    "names": ["analytics-forwarder", "error-reporter"],
    "pending_restart": false
  },
  "otlp": {
    "configured": true,
    "endpoint": "http://localhost:4317",
    "service_name": "dcc-mcp-gateway",
    "pending_restart": false
  }
}
```

The response omits secrets: the DSN prefix is shown for identification but
the full DSN (including the secret key) is never exposed. When an integration
is unconfigured, its block contains `"configured": false` and no further
fields.

If a configuration change is detected (e.g. `DCC_MCP_SENTRY_DSN` was set or
cleared since process start), `pending_restart` is `true` and the panel
displays a visual indicator prompting the operator to restart the gateway.

### Routes

| Route | Content-Type | Description |
|-------|-------------|-------------|
| `GET /admin/api/integrations` | `application/json` | Read-only integration configuration summary |

### Stable agent-facing route

| Stable route | Mirrors |
|--------------|---------|
| `GET /v1/debug/integrations` | `/admin/api/integrations` |

## Security Note

The admin UI is **read-only** and has **no authentication** by default. It binds to the same host as the elected gateway, which defaults to `127.0.0.1`. For production:
- Keep it bound to localhost, or place behind a reverse proxy with IP allowlist/basic auth
- Disable when not needed: `--no-admin`, `DCC_MCP_NO_ADMIN=true`, or `cfg.admin_enabled = False`
- Treat the Governance panel as an inspection surface only; policy, capture, redaction, and quota changes must still happen through authenticated deployment configuration.
- Never expose directly to the public internet

## See also

- [middleware.md](middleware.md) — `AuditMiddleware` that feeds `/admin/api/calls`
- [observability.md](observability.md) — `EventLog` that feeds `/admin/api/logs`
- [gateway.md](gateway.md) — full gateway configuration reference (webhooks, Sentry, OTLP)
- [sentry.md](sentry.md) — Sentry error monitoring reference
