# Verified Regression Suite (VRS)

JSONL traces plus a zero-dependency HTTP replayer to pin **live** gateway / DCC behaviour that unit tests miss.

**Process for contributors** (naming, when to skip, PR checklist) → root [`AGENTS.md`](../../AGENTS.md) section *Verified Regression Suite (VRS)*.

## Trace format

Each trace is a UTF-8 `.jsonl` file:

1. **Optional header line** — object with `"_vrs": {"version": 1, ...}` and metadata:
   - `trace_id` — short stable id (matches GitHub issue when applicable).
   - `skip_preflight` — optional object. If present, the replayer runs one HTTP step first; when `skip_when` matches, the whole trace exits **0** (skipped, not failed). Use this when no suitable DCC instance is registered, or when fewer than *N* live rows are present (see `less_than` below).

2. **Step lines** — each is a JSON object:
   - `id` — optional label for logs.
   - `http` — `{ "method": "GET"|"POST", "path": "/v1/...", "headers": { ... }, "json": { ... } }` (`json` and `headers` optional).
   - `expect` — assertions (all must pass), **or** `expect_any` — list of `expect` objects (at least one must pass).
   - `capture` — optional `{ "json_pointer": "/hits/0/tool_slug", "as": "slug" }` after a successful step; substitutes `{{capture:slug}}` in later bodies.
   - `sleep_ms` — optional delay step with no HTTP request; useful for observing health-loop effects.

### Expect fields

| Field | Meaning |
|-------|---------|
| `status` | HTTP status int, or list of allowed ints |
| `body_contains` | Substring that must appear in the raw body |
| `body_contains_all` | List of substrings that must all appear in the raw body |
| `json_subset` | Recursive partial match on parsed JSON (dict leaves must match) |

### `skip_preflight.skip_when`

| Field | Meaning |
|-------|---------|
| `json_pointer` | Value to read from the preflight JSON body (default `/total`). |
| `equals` | Skip when the resolved value equals this (same types as JSON). |
| `less_than` | Skip when the resolved value parses as an integer **strictly less than** this (e.g. `less_than: 3` skips when `/total` is 0, 1, or 2). |
| `body_contains` | Skip when the raw preflight body contains this substring. |
| `body_not_contains` | Skip when the raw preflight body does not contain this substring. |

### Substitution

Any string in `http.json` may contain `{{capture:NAME}}` filled from prior `capture.as` values.

## Running the replayer

```bash
python scripts/vrs_replay.py --base-url http://127.0.0.1:9765 \
  --trace tests/vrs/traces/gateway-smoke.jsonl
```

Or via Just:

```bash
just vrs-replay TRACE=tests/vrs/traces/gateway-smoke.jsonl BASE=http://127.0.0.1:9765
```

Environment:

| Variable | Default | Purpose |
|----------|---------|---------|
| `VRS_HTTP_TIMEOUT_SECS` | `120` | Per-request timeout |

Exit codes: **0** success or skip; **1** failure.

Dry-run (parse + print steps; no HTTP):

```bash
python scripts/vrs_replay.py --base-url http://127.0.0.1:1 --dry-run --trace tests/vrs/traces/<file>.jsonl
```

## Traces in this repo

| File | Needs live DCC? | Purpose |
|------|-----------------|--------|
| `traces/gateway-smoke.jsonl` | No | `GET /v1/healthz` + browse `POST /v1/search`. |
| `traces/core-1360-gateway-daemon-mode.jsonl` | No | `dcc-mcp-server gateway` daemon exposes gateway REST without a live DCC backend. |
| `traces/core-1483-gateway-runtime-supervisor.jsonl` | No | Gateway daemon exposes `/v1/readyz` lifecycle and recovery-count fields while `/v1/instances` remains empty without a registered backend (PIP-483). |
| `traces/core-1361-http-instance-registration.jsonl` | No | Gateway daemon accepts remote HTTP instance register → heartbeat → path-call resolution → deregister. |
| `traces/core-1363-relay-gateway-source.jsonl` | Relay + live backend | Gateway daemon configured with `--relay-source` exposes active tunnel rows with `source: "relay"` and routable `/tunnel/<id>/mcp` URLs. |
| `traces/core-1364-unified-instances.jsonl` | Relay + live backend | `resources/read gateway://instances` exposes merged relay + HTTP rows with `instance_short`, `source_meta`, and `by_source` counts. |
| `traces/core-1365-gateway-auth-negative.jsonl` | No (auth-enabled gateway) | Negative-path coverage of bearer-token + DCC-scope enforcement on `/v1/instances/register`; against the embedded auto-gateway (auth disabled) every step succeeds, against an auth-enabled daemon the rejection envelopes (`unauthorized`, `dcc_scope_mismatch`) are pinned. |
| `traces/gateway-search-no-matches.jsonl` | No | Search with improbable token → `total: 0`, empty `hits`. |
| `traces/gateway-rest-describe-bad-request-missing-tool-slug.jsonl` | No | `POST /v1/describe` with `{}` → `400`, `error.kind` = `bad-request`. |
| `traces/gateway-rest-describe-unknown-slug.jsonl` | No | Well-formed unknown slug → `404`, `error.kind` = `unknown-slug`. |
| `traces/gateway-rest-call-missing-tool-slug.jsonl` | No | `POST /v1/call` without `tool_slug` → `400`, `bad-request`. |
| `traces/gateway-rest-call-unknown-slug.jsonl` | No | Unknown slug on call path → `404`, `unknown-slug` (matches refresh-retry semantics). |
| `traces/core-1153-rest-compact-errors.jsonl` | No | Compact TOON negotiation on `/v1/describe` and `/v1/call_batch` preserves bad-request details; `/v1/call` can still force JSON. |
| `traces/core-1157-rest-compact-default.jsonl` | No | REST defaults to compact TOON, explicit JSON opt-out still works, and token accounting headers are present. |
| `traces/maya-215-execute-python-regression.jsonl` | Yes (Maya) | After harmless `execute_python`, triggers `TypeError` via bad `polySphere` arg; follow-up call must still succeed (maya#215 / #199 class). |
| `traces/maya-235-capture-then-playblast-survives.jsonl` | Yes (Maya) | `capture_viewport` → `playblast` → `capture_viewport`; the third call MUST still return a JSON envelope (not transport error / instance-offline) — generalises the maya#215 watchdog to any `affinity:main` async action (maya#235). |
| `traces/maya-export-fbx-describe-path-schema.jsonl` | Yes (Maya, minimal mode) | `export_fbx` MUST appear in `POST /v1/search` with `has_schema: true` before `load_skill`; `POST /v1/describe` MUST expose `inputSchema.properties.path` + `required: ["path"]`; `destination` alone MUST NOT succeed on `POST /v1/call`. |
| `traces/pip-577-describe-skip-no-schema.jsonl` | Yes (Maya) | Search hits with `has_schema: false` MUST point `next_step` directly at `POST /v1/call`, skipping `/v1/describe`. |
| `traces/core-992-describe-tool-preserves-input-schema.jsonl` | Yes (any DCC) | `describe_tool` MUST round-trip `inputSchema.properties` and report `record.has_schema: true` for tools that declared properties (regression of core#857 / #992). |
| `traces/core-992-call-tool-validates-arguments.jsonl` | Yes (any DCC) | `call_tool` against a typed tool MUST NOT report `validation_skipped: true` — paired with the describe trace above (core#992). |
| `traces/core-993-search-tools-includes-unloaded.jsonl` | Yes (live Maya in default minimal mode) | `POST /v1/search` MUST return hits for actions belonging to **unloaded** skills (core#993 / #858). |
| `traces/core-994-meta-tools-do-not-dominate-ranking.jsonl` | Yes (any DCC) | Domain queries top-1 MUST be a domain tool, never `project.*` / `dcc_capability_manifest` / `recipes__*` / `diagnostics__*` (core#994). |
| `traces/core-995-list-skills-respects-limit.jsonl` | Yes (any DCC) | `POST /v1/list_skills` MUST honour `limit` and `fields` and report a `truncated` flag (core#995). |
| `traces/core-996-instance-offline-carries-previous-status.jsonl` | No | `instance-offline` (or `unknown-slug`) error envelope MUST carry `previous_status` so agents can distinguish manual restart from crash (core#996). |
| `traces/core-1037-gateway-yield-unsupported-envelope.jsonl` | No | `POST /gateway/yield` unsupported/invalid optional-capability path MUST return a structured envelope that tells runners to poll instead of treating it as a crash. |
| `traces/core-1092-stable-debug-api.jsonl` | No | Stable `/v1/debug/*` routes expose agent diagnostics without scraping Admin HTML, including compact agent trace packets, compact TOON debug-bundle summaries, and public-safe issue reports. |
| `traces/core-1093-trace-context-debug-bundle.jsonl` | No | `X-Request-Id` and W3C `traceparent` stay distinct, and `/v1/debug/bundles/{trace_id}` can retrieve the retained trace. |
| `traces/core-1108-deregistered-history.jsonl` | Optional booting row | Stable debug route exposes recently auto-deregistered history and, when present, keeps port=0 booting diagnostics visible across debug health reads. |
| `traces/core-1124-3dsmax-main-affinity-host-bridge.jsonl` | Yes (3ds Max) | A main-affinity 3ds Max tool called through `/v1/call` must route through the attached host dispatcher instead of `THREAD_AFFINITY_UNAVAILABLE`. |
| `traces/core-1125-3dsmax-diagnostics-screenshot-dict.jsonl` | Yes (3ds Max) | After `load_skill`, bundled `dcc_diagnostics__screenshot` must return a normal dict envelope through gateway REST. |
| `traces/core-1133-app-ui-gateway-rest.jsonl` | Yes (any app_ui-capable instance) | `app_ui__snapshot` must be discoverable through gateway REST, describe must expose UI metadata, and call must preserve the structured envelope. |
| `traces/core-1134-app-ui-mock-workflow.jsonl` | Yes (any app_ui-capable instance) | `app_ui` mock workflow must support snapshot -> find -> act -> wait and structured stale/policy/timeout/missing-window errors through REST. |
| `traces/core-1652-load-skill-backend-failure.jsonl` | Yes (affected Maya/backend) | Gateway `load_skill` must surface backend `unknown-action` / `success:false` as a failure instead of decorating it as `loaded:true`. |
| `traces/gateway-multi-instance-stress.jsonl` | Yes (≥3 live instances) | Skips unless `GET /v1/instances` reports `total >= 3`; then bursts health/instances/readyz/context/search to catch registry/probe regressions under load. |

## CI policy (recommended)

- **PR CI**: dry-run over every `tests/vrs/traces/*.jsonl` (see root `ci.yml` Lint job).
- **Live**: run `gateway-smoke` + REST error traces against any test gateway when available.
- **Maya-specific**: run `maya-215-execute-python-regression` on nightly / studio runners with live Maya + gateway; use `skip_preflight` so absent hosts skip cleanly.

## Related issues

- maya [#215](https://github.com/dcc-mcp/dcc-mcp-maya/issues/215) — execute_python crash regression
- maya [#199](https://github.com/dcc-mcp/dcc-mcp-maya/issues/199) — prior crash report
- maya [#235](https://github.com/dcc-mcp/dcc-mcp-maya/issues/235) — playblast / capture_viewport stops MCP HTTP server
- core [#992](https://github.com/dcc-mcp/dcc-mcp-core/issues/992) — schema stripped between backend and gateway
- core [#993](https://github.com/dcc-mcp/dcc-mcp-core/issues/993) — search_tools excludes unloaded skill actions
- core [#994](https://github.com/dcc-mcp/dcc-mcp-core/issues/994) — search ranking dominated by meta-tools
- core [#995](https://github.com/dcc-mcp/dcc-mcp-core/issues/995) — list_skills returns full metadata for every skill
- core [#996](https://github.com/dcc-mcp/dcc-mcp-core/issues/996) — instance-offline envelope must carry restart cause
- core [#1037](https://github.com/dcc-mcp/dcc-mcp-core/issues/1037) — cooperative gateway yield fallback should be structured and non-alarming
- core [#1092](https://github.com/dcc-mcp/dcc-mcp-core/issues/1092) — stable `/v1/debug/*` API for agent diagnostics
- core [#1093](https://github.com/dcc-mcp/dcc-mcp-core/issues/1093) — first-class Trace Context for full-chain debug bundles
- core [#1124](https://github.com/dcc-mcp/dcc-mcp-core/issues/1124) — HostExecutionBridge registration must satisfy main-affinity tools/call routing
- core [#1125](https://github.com/dcc-mcp/dcc-mcp-core/issues/1125) — bundled diagnostics screenshot must return a dict through REST dispatch
- core [#1133](https://github.com/dcc-mcp/dcc-mcp-core/issues/1133) — app_ui gateway discovery and REST dispatch
- core [#1134](https://github.com/dcc-mcp/dcc-mcp-core/issues/1134) — app_ui DCC debugging workflow examples and REST traces
- core [#1365](https://github.com/dcc-mcp/dcc-mcp-core/issues/1365) — gateway must enforce authentication and scope-bound DCC registration
- core [#1652](https://github.com/dcc-mcp/dcc-mcp-core/issues/1652) — gateway load_skill must not report loaded=true when backend load fails
