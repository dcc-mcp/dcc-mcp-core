---
name: dcc-mcp-skill-developer
description: >-
  Infrastructure skill - guide agents through designing, implementing, testing,
  and reviewing DCC-MCP adapter skill packages for Maya, Blender, 3ds Max,
  Houdini, Photoshop, ZBrush, Unreal, Unity, and custom studio hosts. Use when
  adding or changing SKILL.md, tools.yaml, scripts, server wiring, or adapter
  skill taxonomy in dcc-mcp-* repositories. Not for driving a live DCC scene -
  use domain skills or dcc-cli-gateway for that.
license: MIT
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "1.0.0"
    search-hint: >-
      develop dcc-mcp skill, adapter skill authoring, tools.yaml, SKILL.md,
      Maya Blender 3ds Max, affinity, execution, stage taxonomy, gateway
    tags: "skill-authoring, adapter, maya, blender, 3dsmax, future-dcc"
    skill-reference-docs:
      - "references/*.md"
---

# DCC-MCP Skill Developer

Use this skill when you are changing or creating DCC-MCP adapter skill packages.
It distills patterns from dcc-mcp-maya, dcc-mcp-blender, and dcc-mcp-3dsmax
into a faster authoring loop.

## Fast Workflow

1. Classify the work: new host adapter, new domain skill, new infrastructure
   skill, or porting an existing skill to another DCC.
2. Read only the reference you need:
   - [ADAPTER_PATTERNS.md](references/ADAPTER_PATTERNS.md) for server and
     composition-root patterns.
   - [SKILL_AUTHORING_CHECKLIST.md](references/SKILL_AUTHORING_CHECKLIST.md)
     for SKILL.md, tools.yaml, and scripts.
   - [HOST_MATRIX.md](references/HOST_MATRIX.md) for Maya, Blender, 3ds Max,
     and future DCC differences.
   - [TESTING_MATRIX.md](references/TESTING_MATRIX.md) for unit, lint, gateway,
     E2E, and VRS coverage.
3. Prefer existing adapter helpers before adding new abstractions.
4. Keep DCC identity parameterized: `dcc_name`, `dcc_type`, environment prefixes,
   skill names, and search examples.
5. Make every tool declaration explicit: `source_file`, `execution`, `affinity`,
   safety annotations, and `timeout_hint_secs` for async tools.
   Published MCP tool names must be client-safe
   `^[A-Za-z0-9_-]{1,64}$`; use underscores instead of dotted tool names.
6. Treat `metadata.dcc-mcp.depends` as soft during discovery. Composition
   skills may remain searchable with `status: pending_deps` while host-specific
   dependencies are injected later through `DCC_MCP_*_SKILL_PATHS`. `load_skill`
   auto-loads discovered dependencies first and returns a clear missing-dep
   error only when a dependency is still absent.
7. When changing adapter server wiring or caller examples, keep Admin telemetry
   useful: pass optional `agent_context` / `caller_context` summaries through
   MCP `_meta`, REST `meta`, or `x-dcc-mcp-agent-*` headers when the caller is
   an agent. Include only explicit summaries, plans, observations, and
   correlation ids; never ask tools to expose hidden chain-of-thought. Preserve
   Admin `links` fields in examples so every trace/debug bundle, OpenAPI
   Inspector/spec link, or issue-report JSON export can be copied as a complete
   URL into a follow-up agent, LLM evaluation prompt, or GitHub issue. When an
   adapter example surfaces an `mcp_url`, make sure the Admin Dashboard can
   derive per-instance OpenAPI Inspector, spec JSON, and docs links from it.
   For machine consumers, prefer the stable gateway `/v1/debug/*` routes and
   `GET /v1/openapi.json` over scraping `/admin` HTML or dashboard internals;
   the shipped server paths enable the required gateway `admin` feature and
   Admin telemetry runtime state, while minimal direct `dcc-mcp-gateway` builds
   or runtimes started with Admin disabled may omit those debug routes.
   For REST discovery, describe, call, and batch examples, keep `/v1/search`,
   `/v1/describe`, `/v1/call`, and `/v1/call_batch` legacy JSON-compatible by
   default and request compact TOON only explicitly with
   `Accept: application/toon`, `response_format: "toon"`, or `compact: true`;
   surface the `x-dcc-mcp-*` token accounting headers when teaching agents how
   to budget discovery, schema, invocation, and batch payloads. Compact batch
   examples may also read per-result `token_accounting` metadata.
   Subscribe to the shared `EventBus` when an adapter or studio integration
   needs programmatic lifecycle hooks: use `skill.*` events for load/unload
   visibility and `tool.*` events for dispatch/completion/failure metrics
   instead of scraping logs or wrapping every handler manually.
   For policy enforcement, register `EventBus.before(...)` only on vetoable
   lifecycle points (`skill.loading`, `tool.dispatched`,
   `resource.subscribed`, `client.initialize`); keep callbacks fast and return
   `EventBus.veto(reason, code)` instead of raising for expected denials.
   For standalone `dcc-mcp-server` deployments, prefer
   `DCC_MCP_WEBHOOKS_CONFIG` when those lifecycle events need to leave the
   process: webhook delivery is asynchronous, bounded, filterable by dotted
   envelope paths, and reports exhausted retries as `webhook.delivery_failed`.
   For local traffic debugging, prefer the gateway `traffic.frame` capture
   stream (`DCC_MCP_TRAFFIC_CAPTURE=jsonl:<path>` for quick capture, or
   `DCC_MCP_TRAFFIC_CONFIG=traffic_capture.yaml` for SQLite/filter/redact
   capture) over ad-hoc print logging. Redactions are write-time exact JSON
   paths such as `body.data.params.arguments.api_key`, and capture files can
   contain prompts, scene paths, and tool arguments, so keep them as local
   debugging artifacts unless redaction has been applied.
   For gateway-facing examples, use the canonical four-tool MCP workflow:
   `search` / `describe` / `load_skill` / `call`. `call` accepts either one
   `tool_slug` with `arguments` or an ordered `{calls:[...]}` batch, matching
   REST `/v1/call` and `/v1/call_batch`. Hidden legacy names (`search_tools`,
   `describe_tool`, `call_tool`, `call_tools`, lease helpers) may appear in old
   clients, but new docs and skills should not advertise them in `tools/list`
   workflows.
8. For adapter install, uninstall, or upgrade flows, use
   `dcc_mcp_core.install_lifecycle` before importing Rust-backed public API:
   query/stop registered sidecars, inspect install roots, classify locked
   native artifacts, and call `safe_remove_tree` / `safe_replace_tree` from a
   process that has not loaded `dcc_mcp_core._core`. Publish package versions
   in registry metadata (`dcc_mcp_core_version`, `dcc_mcp_server_version`,
   `adapter_version`); `ServiceEntry.version` is the DCC application version.
   Test launchers should mark temporary instances with public metadata such as
   `owner`, `session`, and `safe_stop_url` when they need automation to target
   and stop only that instance.
   Stop helpers must respect FileRegistry sentinel locks before trusting PID
   liveness so installer code never terminates a reused PID from a stale row.
9. For embedded main-thread tools, wire `HostExecutionBridge` with the same
   host `QueueDispatcher` / `BlockingDispatcher` that the DCC timer or
   headless driver ticks. `DccServerBase` will then register both
   `set_in_process_executor` and HTTP `attach_dispatcher` before server start,
   so MCP `tools/call` and REST `/v1/call` satisfy `thread_affinity: main`.
   Flip `ReadinessProbe.host_execution_bridge` and
   `ReadinessProbe.main_thread_executor` only after that bridge path is
   actually usable; smoke tests may require those bits via
   `dcc-mcp-cli wait-ready --require host_execution_bridge,main_thread_executor`.
10. Skill script entry points may use either modern `main(**params)` or legacy
    `main(params)` signatures; prefer `main(**params)` for new scripts and keep
    dict-style wrappers only for compatibility during adapter migrations.
11. When an adapter must adjust discovered skill metadata before registration
    (for example disabling thread-affinity enforcement for a standalone path),
    use `catalog.get_skill(name)` to obtain a detached `SkillMetadata` copy,
    mutate its `tools` / `groups` / policy fields, then call
    `catalog.load_skill_object(skill)` or `server.load_skill_object(skill)`.
    Do not parse or rewrite `SKILL.md` / `tools.yaml` at adapter runtime.
    Keep `get_skill_info()` for serialized inspection only.
12. Add tests at the lowest executable layer, then one discovery/load/call or
    gateway REST path when behavior crosses MCP or REST boundaries.
13. For application UI automation, use the generic `app_ui__*` contract rather
    than DCC-specific names: snapshot -> find -> act -> wait_for -> verify.
    The Rust contract types live in `dcc-mcp-app-ui`; do not add Qt, OS
    accessibility, webview, PyO3, or HTTP runtime dependencies there.
    Prefer native DCC APIs first; use `app_ui` as a scoped, policy-controlled
    fallback. Keep raw coordinates and keyboard shortcuts disabled by default,
    keep `require_scoped_window` enabled unless an adapter explicitly opts into
    a documented whole-desktop fallback, return structured `stale_control` /
    `policy_disabled` / `timeout` errors, and redact typed text or screenshot
    bytes in audit records.
    Tool declarations must carry MCP safety annotations plus `execution`,
    `affinity`, and `timeout_hint_secs`; the gateway propagates those through
    MCP `search` / `describe` and REST `/v1/search` / `/v1/describe` so
    agents can discover UI risk and timeout contracts before calling.
    Gateway instance diagnostics expose `diagnostics.app_ui.status` as
    `available`, `unavailable`, or `disabled_by_policy`; adapters may publish
    policy status/reason in registry metadata with `app_ui.status` and
    `app_ui.reason`.
    Document workflow examples for modal dialogs, settings panels, semantic
    waits, and recovery from stale controls, missing windows, denied actions,
    and timeouts. Add a mock-backend workflow test or VRS trace whenever
    gateway `/v1/*` routing is part of the behaviour being changed.
    Backend-specific implementations, such as the bundled Chrome DevTools
    prototype, belong behind the skill/runtime layer and must preserve the
    same `app_ui__snapshot` -> `find` -> `act` -> `wait_for` contract.
    CDP-backed implementations should expose explicit presets for reusing an
    existing browser/webview session, launching an isolated test profile, or
    attaching to a host-specific endpoint such as AuroraView, Microsoft Edge,
    or `agent-browser`. Prefer reuse when a user expects existing cookies or
    tokens to remain available, and keep provider-specific launch logic in
    testable helpers so CI can cover endpoint and CLI discovery without a GUI.
    Windows UI Automation backends must also live behind this runtime layer.
    Require an explicit process/window scope, map UIA controls into the shared
    app_ui roles, keep raw coordinate and keyboard shortcut fallbacks disabled
    by default, and skip live UIA tests cleanly when Windows accessibility APIs
    are unavailable.

## Adapter Selection

- Use Maya patterns for mature stage taxonomy, main-thread dispatch,
  cancellation, resources, readiness, capability manifests, and strict skill
  linting.
- Use Blender patterns for a lean `DccServerBase` adapter scaffold and
  progressive loading helpers.
- Treat current 3ds Max skills as migration targets: preserve the pymxs domain
  logic, but modernize into nested `SKILL.md`, `tools.yaml`, and scripts.
- For future hosts, start from Blender's lean scaffold, then add Maya-style
  lifecycle hardening only when the host actually needs it.

## Non-Negotiables

- No top-level dcc-mcp extension keys in SKILL.md.
- No host API imports at module import time in skill scripts.
- No scene-touching tool without `affinity: main`.
- No `execution: async` without a realistic `timeout_hint_secs`.
- No dotted MCP tool names in `tools.yaml`, examples, or caller docs.
- No new generic helper crate or module when core or an adapter-local owner
  already exists.
- No installer or uninstaller import path that loads `dcc_mcp_core._core`
  before removing or replacing a bundled adapter payload.
- No raw `execute_python` or `execute_mel` as the primary UX when a typed tool
  can exist.
