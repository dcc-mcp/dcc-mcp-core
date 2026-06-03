# DCC-MCP Gateway — agent workflow guide

**Scope:** This text is **platform-agnostic**. It explains how to use the **MCP gateway** well (tools, resources, prompts, REST twins). It does **not** teach a specific DCC SDK.

**Canonical copy:** `resources/read` with `uri=gateway://docs/agent-workflows` (matches the running gateway build).

---

## Use MCP the way the gateway expects

1. **`tools/list`** — Small, stable set: exactly **`search`**, **`describe`**, **`load_skill`**, and **`call`**. Treat it as an **index of gateway verbs**, not the full catalog of every backend action.
2. **Discover backend work** — `search(kind="tool")` → **`describe`** (read schema, descriptions, safety hints, **`affinity`**, **`execution`**, timeouts) → **`call`**. On REST, use `/v1/search`, `/v1/describe`, `/v1/call` (or the path-style `POST /v1/dcc/{dcc}/instances/{id}/call` from **REST clients** below). Skipping `describe` wastes retries and breaks validation. Preserve the returned `next_step.arguments.meta.search_id` (or the same object as MCP `_meta`) on `describe`, `load_skill`, and `call`; this lets the gateway measure selected rank and hit rate without storing full prompts.
3. **Chaining** — **`call({calls:[...]})`** / **`POST /v1/call_batch`** runs up to **25** ordered calls when you have several **different** validated steps. Prefer fewer, well-formed calls over chatty micro-steps.
4. **Skills vs tools** — `search(kind="skill")` / `load_skill` load packaged workflows on a host; `search(kind="tool")` resolves a **`tool_slug`** for the dynamic surface. Keep names straight; use `describe` before calling an unfamiliar slug. Unloaded hits carry `load_state`, `available_groups` when known, and `next_step` with both MCP and REST call shapes.
5. **Progressive groups** — gateway `load_skill` activates declared groups by default (`activate_groups=true` when omitted). Pass `activate_groups=false` for lazy loading, or `load_skill(..., tool_group="...")` when you only want one group.

### Telemetry correlation

When you use a `next_step` from `search`, keep its `meta.search_id` as REST
`meta.search_id` or MCP `_meta.search_id` on `describe`, `load_skill`, `call`,
and batched `call` requests. The gateway emits `gateway.search`,
`gateway.describe`, `gateway.load_skill`, `gateway.call`, and
`gateway.call_batch` OTLP spans with bounded `dcc_mcp.*` attributes, including
selected rank, score, match reasons, policy outcome, and success/failure kind.
Only explicit bounded `agent_context` fields are exported; do not send hidden
reasoning, secrets, or raw prompt bodies as telemetry metadata. Gateway MCP
also carries bounded `initialize.params.clientInfo` forward per
`Mcp-Session-Id` so later `tools/call` rows show client name/version; REST
clients that omit `client_platform` fall back to the first `User-Agent` product
token.

If a call is denied, throttled, or unexpectedly redacted, inspect
`GET /v1/debug/governance` before retrying. It reports the effective read-only
policy, DCC/skill/tool allowlists, traffic capture mode and redaction paths,
middleware quota pressure, and recent allow/deny/throttle/capture decisions.
Use `GET /v1/debug/traffic` when the admin Traffic panel shows no rows: its
`capture_status.state` distinguishes genuine `no_traffic` from
`capture_disabled`, `capture_unavailable`, or filtered capture, and retained
frames are metadata-only by default.

### Host / connector wrappers (common mistakes)

If your IDE or orchestration layer exposes **non-standard** tool names (for example `defer_execute_tool`, `get_sessions`, `tool_search`), they **must** map onto the gateway verbs above — those names are **not** part of `dcc-mcp-gateway`’s native `tools/list`.

| Wrong assumption | Use instead |
|------------------|-------------|
| `get_sessions` / “list MCP sessions” for routing | **`resources/read`** with `uri=gateway://instances` (MCP), or **`GET /v1/instances`**, **`GET /v1/context`** (REST). Rows carry `instance_id`, `dcc_type`, `mcp_url`. |
| Wrapper that only accepts `code` at the top level | Gateway **`call`** always needs **`tool_slug`** + optional **`arguments`** (see below). |
| Guessing `tool_slug` segments | Run **`search(kind="tool")`** (or **`POST /v1/search`**) after you know `dcc_type` / `instance_id` from **`gateway://instances`**. |

### Skills-first before `execute_python` / raw MEL (Maya-oriented)

For Maya behind the gateway, prefer **`search(kind="skill")`** → **`load_skill`** → a **typed** tool from `tools.yaml` (for example primitives / mesh / **interchange**):

- Many cubes / spheres / transforms → `maya-primitives` (not a giant `execute_python` loop).
- FBX / OBJ / scene save to disk → `maya-geometry` (`export_fbx`, `import_fbx`, `save_scene`, …) — **avoid** hand-rolled `FBXExport` MEL unless no skill fits.
- Only when no packaged tool exists: `maya-scripting` → `execute_python` / `execute_mel`, preferably **`file_path`** to a `.py` / `.mel` the **Maya process host** can read (see next section).

### `tool_slug` shape (and why it is not slash-separated yet)

- Every `search(kind="tool")` hit includes **`rank`**, **`tool_slug`**, and bounded **`match_reasons`**. Treat `tool_slug` as an opaque **routing token**: copy it **verbatim** into `describe` and `call` (and into REST `POST /v1/describe` / `/v1/call` bodies as the `tool_slug` field).
- Format: **`<dcc_type>.<instance_prefix_or_uuid>.<backend_tool>`** — three dot-separated routing segments. Example: `maya.277685a7.maya_primitives__create_sphere`. This encodes the same tuple a path-style URL would use (`/<dcc>/<instance>/<backend_tool>`); the final `backend_tool` segment is the client-safe MCP name such as `project_save` or `maya_primitives__create_sphere`.
- **Common agent mistake:** calling `call` with only `code` / `python` / `mel` at the **top level**. That shape belongs to **specific backend tools** inside **`arguments`**, only when their schema says so — the gateway wrapper **always** requires **`tool_slug`** plus optional **`arguments`** / **`meta`**.

---

## Resources and prompts (read before guessing)

- **`resources/list`** on the gateway — Merges **gateway-native** pointers with **per-backend** entries. **Copy URIs exactly** (including `dcc://…` or instance-prefixed forms). Do not rewrite or strip prefixes; the gateway routes `resources/read` by URI.
- **`resources/read`** — Primary way to pull **registry views**, **diagnostics**, **catalog**, **this guide**, and **host-published help** (documentation blobs, scene snapshots, cmd help, etc., when the adapter exposes them).
- **`prompts/list`** / **`prompts/get`** — Use when the gateway aggregates prompt templates from backends; same rule: use **returned names and URIs as-is**.

**Gateway-native pointers you should know:**

| URI | Purpose |
|-----|---------|
| `gateway://instances` | Live DCC rows: `dcc_type`, `mcp_url`, health — use when routing or when the user names a product (Maya, Photoshop, Blender, …): map the name to **`dcc_type`** / a concrete instance, not to extra `tools/list` entries. |
| `gateway://instances/{id}` | One row (full UUID or unique prefix). |
| `gateway://diagnostics/*` | Gateway/backend health signals for operators and agents. |
| `gateway://instances` rows | Each instance may include a `diagnostics` object (`readiness` bits from `/v1/readyz`, `last_error` from the most recent failed gateway-proxied call). Readiness includes `process`, `dcc`, `skill_catalog`, `dispatcher`, `host_execution_bridge`, and `main_thread_executor`. |

When a gateway-proxied `call` / `POST /v1/call` fails with `thread-affinity-violation`, read `error.backend` for the selected instance id, direct `mcp_url` vs `gateway_mcp_url`, readiness (`process` / `dcc` / `skill_catalog` / `dispatcher` / `host_execution_bridge` / `main_thread_executor`), and the backend's structured `context` before retrying.
| `gateway://catalog` | Public package index (optional discovery). |
| `gateway://docs/agent-workflows` | **This** guide — re-fetch when instructions drift in a long session. |

**Help and documentation from DCC hosts:** Adapters often expose **read-only** resources (e.g. command help, API signatures, scene snapshots). They appear in **`resources/list`** with stable schemes. Always **`resources/read`** the **exact** URI from the list — do not fabricate paths. If a `describe` response points to follow-up resources, prefer those over web search for DCC-accurate text.

**Debugging an agent chain:** When Admin telemetry is enabled, prefer
`GET /v1/debug/workflows` for a session-level view of `search` -> `describe`
-> `load_skill` -> `call`. The payload reuses retained search telemetry,
dispatch traces, and audit rows, so it shows selected rank, zero-result
searches, time-to-first-success, and per-step trace/debug-bundle/issue-report
links without exposing hidden reasoning or raw prompts.

---

## Efficiency (without a separate “bulk” playbook)

- One **`describe`** per new slug; cache the schema mentally for the rest of the task.
- One **`search(kind="tool")`** with a tight `query` and correct **`dcc_type`** / **`instance_id`** when the user names a host or you see multiple live rows in `gateway://instances`.
- Prefer **structured arguments** (paths, flags, IDs) in a single **`call`** when the schema supports it, instead of many tiny speculative calls.
- Use **`call({calls:[...]})`** only when you truly have a **short sequence of different** tools—not as a default hammer.

---

## Execution hints: affinity, async, timeouts

`describe` (and tool metadata) may declare **`affinity`** (e.g. main thread vs worker), **`execution`** (sync vs async), and **timeout** hints. **Follow them:** main-thread tools must not be “worked around” from the client; async tools may return job handles—poll or subscribe as the schema says. Ignoring affinity or timeouts produces flaky failures that look like gateway bugs but are contract violations.

---

## REST clients

Where the gateway mounts them, mirror MCP with **`POST /v1/search`**, **`/v1/describe`**, **`/v1/call`**, **`/v1/call_batch`**, and **`/v1/resources*`** / **`/v1/prompts*`**. Same discovery order and same URI hygiene as MCP.

`POST /v1/search`, `/v1/describe`, `/v1/call`, direct per-instance describe/call routes, and `/v1/call_batch` return compact TOON by default. REST agents that need legacy JSON can set `Accept: application/json` or `response_format: "json"`; operators can temporarily restore a JSON-first rollout with `DCC_MCP_GATEWAY_RESPONSE_FORMAT=json`. Use `response_format: "toon"` or `compact: true` to force compact output when an `Accept` header prefers JSON. Compact-capable responses include `x-dcc-mcp-token-estimator`, original/returned byte and token counts, and savings headers so an agent can budget repeated discovery, schema, and invocation calls. Compact batch responses also include per-result `token_accounting` metadata.

MCP agents request the same compact TOON payloads through request metadata instead of HTTP `Accept`: compact-capable clients should set `params._meta.response_format="toon"` or `params._meta.compact=true` after `initialize` advertises `capabilities.experimental["dcc-mcp"].compactResponses`, and can set `params._meta.response_format="json"` to opt out for one request. Legacy clients that omit the metadata keep normal JSON results. The outer JSON-RPC envelope stays JSON. `tools/call` keeps the MCP `CallToolResult` shape and adds `mimeType: "application/toon"` to compact text content; JSON-RPC errors stay normal `error` objects.

### Gateway handoff signals

When a gateway accepts a cooperative `/gateway/yield`, connected SSE clients receive `notifications/gateway/handoff` before the listener shuts down. Treat it as a short retry window: pause new requests until `deadline_unix_secs`, refresh `gateway://instances`, then retry through the same stable endpoint. The gateway also marks its `__gateway__` sentinel as `shutting_down`, so registry readers can distinguish graceful handoff from a crash-driven failover.

### Path-style invocation (optional; for curl / service accounts)

- **Gateway:** `POST /v1/dcc/{dcc_type}/instances/{instance_id}/call` with JSON `{ "backend_tool": "<name>", "arguments": {...}, "meta": {...} }` (aliases: `tool`, `action` for `backend_tool`). Same routing as `POST /v1/call` after composing the dotted `tool_slug`; use when you already know `dcc_type` + **`instance_id`** from **`GET /v1/instances`** or **`GET /v1/context`** (`instances` array mirrors `/v1/instances`).
- **Gateway:** `GET /v1/dcc/{dcc_type}/instances/{instance_id}/describe?backend_tool=...` (aliases `tool`, `action` query keys) — same JSON as `GET /v1/tools/{tool_slug}` without assembling the dotted slug.
- **Per-DCC HTTP server (skill-rest):** `POST /v1/dcc/{dcc_type}/call` with the same body — no instance segment because one process owns one session.

### Long `execute_python` / MEL without giant JSON strings

- **Long `execute_python` payloads** — Prefer a typed skill first. If no typed tool fits, materialize the script on the **DCC host filesystem** and call the backend tool with `file_path`. Core exposes `dcc_mcp_core.materialize_script(...)` / `dcc_mcp_core.script_materialization.materialize_script(...)`, returning `{file_ref, file_path, sha256, bytes, ttl, dcc_type, instance_id, session_id, tool_call_id, correlation_id}`. The default root is `~/.dcc-mcp/<dcc_type>/temp/<instance_id>/<session_id>/...` and can be overridden with `DCC_MCP_SCRIPT_MATERIALIZATION_ROOT`.
- **Agent API** — Most `DccServerBase` adapters expose a `materialize_script` tool. Discover it with `search_tools(query="materialize script")`, call it with `content`/`code`, then call the execution tool with the returned `file_path`. The response is descriptor metadata only; raw source is not echoed, and gateway trace input capture redacts script-source fields by default.
- **Policy boundary** — Core helpers use `script_materialization_policy = off | auto | require`. `auto` materializes inline `code` before execution, `require` rejects raw inline code unless a trusted `file_path` / `script_path` is supplied, and `off` is a compatibility escape hatch. Successful script execution should return `context.materialized_script` with path, FileRef, hash, byte length, and reuse metadata.
- **Compatibility wrapper** — `write_temp_file` (skill) and `write_temp_script(...)` still work, but they are compatibility helpers over the materialization store. Prefer the structured descriptor when you need audit, reuse, TTL cleanup, or FileRef metadata.
- **Remote agents** — A path written only on the agent laptop is not executable by a remote studio Maya/Photoshop/Blender process. Use an in-host writer/API or a synced workspace before passing `file_path`.
- **Maya adapter auto-spill** — Some adapter versions may copy very long **inline** `code` strings to a host-local temp file before `exec`, and return a legacy context key such as `host_spilled_inline_script_path`. Treat that as a migration alias; explicit materialize -> execute by path is the auditable contract.

### After a DCC crash or reconnect

The **`instance_id`** in the registry usually **changes**. Cached **`tool_slug`** values from an earlier `search` run may fail with `unknown-slug` / `instance-offline` / 404-style errors.

1. Refresh **`GET /v1/instances`** or **`resources/read` `gateway://instances`**.
2. Run **`search(kind="tool")`** / **`POST /v1/search`** again (or keep using path-style calls with the **new** `instance_id`).
3. Re-**`describe`** when you need schemas for tools you have not validated in this session.
