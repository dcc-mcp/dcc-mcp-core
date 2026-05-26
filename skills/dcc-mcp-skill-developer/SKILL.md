---
name: dcc-mcp-skill-developer
description: >-
  Compatibility skill - legacy guide for designing, implementing, testing, and
  reviewing DCC-MCP skill packages. Prefer dcc-mcp-skills-creator for new skill
  package work. Not for full adapter server/runtime work - use dcc-mcp-creator.
license: MIT
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "1.0.0"
    search-hint: >-
      legacy dcc-mcp skill developer, skill package authoring, tools.yaml,
      SKILL.md, affinity, execution, stage taxonomy
    tags: "skill-authoring, compatibility, maya, blender, 3dsmax, future-dcc"
    skill-reference-docs:
      - "references/*.md"
---

# DCC-MCP Skill Developer

Compatibility entrypoint. Prefer `dcc-mcp-skills-creator` for new DCC-MCP
skill package work, and use `dcc-mcp-creator` for full adapter repository or
server/runtime work.

Use this skill when you are changing or creating DCC-MCP adapter skill packages.
It distills patterns from dcc-mcp-maya, dcc-mcp-blender, and dcc-mcp-3dsmax
into a faster authoring loop.

## Fast Workflow

1. Classify the work. For a full host adapter or server/runtime change, switch
   to `dcc-mcp-creator`; continue here only for domain skills, infrastructure
   skills, or porting an existing skill to another DCC.
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
   Use bounded `metadata.dcc-mcp.search-aliases` and per-tool
   `search_aliases` in `tools.yaml` for domain synonyms, localized phrases, or
   common user wording. Do not stuff long prose or schema dumps into tags or
   summaries just to improve search recall.
6. Treat `metadata.dcc-mcp.depends` as soft during discovery. Composition
   skills may remain searchable with `status: pending_deps` while host-specific
   dependencies are injected later through `DCC_MCP_*_SKILL_PATHS`. `load_skill`
   auto-loads discovered dependencies first and returns a clear missing-dep
   error only when a dependency is still absent.
   Use inline `metadata.dcc-mcp.runtimes` or a sibling `runtimes.yaml` for
   optional runtime capabilities such as `usd-core`, USD command-line tools,
   SDK wheels, or host environment variables. Keep descriptors declarative
   (`python_package`, `python_extra`, `binary`, `env_var`, or `feature`) with
   actionable guidance; discovery surfaces `available` / `degraded` /
   `missing` states without importing tool scripts or running installers.
   When a skill has reusable examples, recipes, or workflow metadata, declare
   them with sibling-file references (`metadata.dcc-mcp.examples`,
   `metadata.dcc-mcp.recipes`, `metadata.dcc-mcp.workflows`). Core can derive
   MCP prompts from those files for `prompts/list`; promote them to explicit
   `metadata.dcc-mcp.prompts` entries when you need stable names, arguments, or
   curated prompt copy. Prompt diagnostics surface missing files and parse
   failures, so adapter tests should assert diagnostics for empty prompt lists
   instead of treating `0 prompts` as self-explanatory.
   At adapter startup, expose optional metadata-driven tools with
   `register_metadata_driven_tools(server, skills=loaded_skills, dcc_name=...)`
   instead of copying recipes/reference-docs import wrappers. Omit `skills`
   only when the helper should own the `scan_and_load_lenient(...)` pass; pass
   explicit `skills` when startup already scanned roots.
   Skill scripts that need JSON/YAML codecs, file/path helpers, LZ4 payload
   compression, result helpers, argument normalization, schema validation, or
   cancellation checks should import them from `dcc_mcp_core.skills_helper`.
   Keep legacy top-level imports working for old skills, but write new examples
   against `skills_helper`.
   Prefer `load_json_file`, `load_yaml_file`, `dump_json_file`, and
   `dump_yaml_file` from the same namespace when a script needs explicit UTF-8,
   source-aware parse errors, byte guards, or mapping-root validation.
   Use `ensure_within_root`, `atomic_write_text` / `atomic_write_bytes`,
   `file_digest` / `bytes_digest`, and `compress_bytes` / `decompress_bytes`
   for bounded local session files. Use `FileRef` and `artefact_put_file` /
   `artefact_get_bytes` instead when a file must cross tool or MCP resource
   boundaries.
7. When changing adapter server wiring or caller examples, keep Admin telemetry
   useful: pass optional `agent_context` / `caller_context` summaries through
   MCP `_meta`, REST `meta`, `x-dcc-mcp-agent-*`, `x-dcc-mcp-actor-*`, or
   `x-dcc-mcp-client-*` headers when the caller is an agent. Include only
   explicit summaries, plans, observations, actor identity
   (`actor_id`, `actor_name`, optional `actor_email_hash`), agent identity
   (`agent_id`, `agent_name`, `agent_kind`, `agent_version`,
   `model_provider`, `model_version`, optional legacy `model`), client
   identity (`client_platform`, `client_os`, `client_host`), auth subject,
   turn correlation (`session_id`, `turn_id`), hashes, character counts, and
   correlation ids; never ask tools to expose hidden chain-of-thought, raw user
   input, or raw agent replies. Treat `source_ip` and `forwarded_for` as
   server-derived fields only: caller metadata and examples must not spoof
   them, and adapters should let the HTTP/gateway boundary attach those values.
   Treat Admin `attribution_trust` / `agent_context.trust` as gateway-computed
   evidence labels (`self_reported`, `header`, `auth`, `server_derived`,
   `trusted_proxy`), not client-supplied metadata; adapter examples should
   demonstrate caller fields, not forged trust fields.
   Preserve
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
   Use `/v1/debug/workflows` when examples or tests need a session-level view
   of the whole `search` -> `describe` -> `load_skill` -> `call` chain; it is a
   read-only projection over retained search telemetry, dispatch traces, and
   audit rows, including selected rank, zero-result searches, and
   time-to-first-success with model/turn summaries, without exposing hidden
   reasoning or raw prompts.
   Gateway `GET /v1/openapi.json` is gateway-specific: it documents only the
   mounted aggregating routes and intentionally omits per-DCC-only resources,
   prompts, jobs, and adapter-local `/v1/dcc/{dcc_type}/call`. Use a concrete
   adapter's own `mcp_url` / `/v1/openapi.json` when examples need those
   per-DCC endpoints.
   When an adapter publishes host-owned MCP resources, use the public
   `DccServerBase.resources()` handle or the convenience helpers
   `register_resource_producer(...)`, `set_scene_resource(...)`, and
   `notify_resource_updated(...)`; never reach into `server._server` for the
   inner HTTP resource registry. Prefer module-level helpers such as
   `register_docs_resource(...)` or `register_adapter_instruction_resources(...)`
   when they match the resource shape, and reserve custom producers for
   adapter-owned URI schemes.
   For REST discovery, describe, call, and batch examples, treat compact TOON as
   the gateway default for `/v1/search`, `/v1/describe`, `/v1/call`, and
   `/v1/call_batch`; show legacy JSON opt-out with `Accept: application/json`
   or `response_format: "json"`, and mention the temporary deployment knob
   `DCC_MCP_GATEWAY_RESPONSE_FORMAT=json` when compatibility windows matter.
   For MCP examples, compact-capable clients request TOON through
   `params._meta.response_format="toon"` or `params._meta.compact=true` after
   `initialize` advertises
   `capabilities.experimental["dcc-mcp"].compactResponses`; legacy clients that
   omit that metadata stay JSON, and `params._meta.response_format="json"`
   opts out per request. Keep the outer JSON-RPC envelope JSON-compatible
   (`jsonrpc`, `id`, `result`, `error` unchanged). `tools/call` compact
   examples must preserve the MCP `CallToolResult` shape and put TOON under
   text content with `mimeType: "application/toon"`;
   surface the `x-dcc-mcp-*` token accounting and observability headers when
   teaching agents how to budget and correlate discovery, schema, invocation,
   and batch payloads. Gateway REST search/describe/load/batch bodies include
   `request_id`, `trace_id`, and `index_generation`; `/v1/call` keeps the
   backend envelope body compatible and exposes those values through
   `x-dcc-mcp-request-id`, `x-dcc-mcp-trace-id`, `traceparent`, and
   `x-dcc-mcp-index-generation`. Gateway search responses also carry
   `search_id`, `ranker_version`, `index_generation`, and per-hit `rank`; when
   examples follow a search result into `describe`, `load_skill`, `call`, or
   batch `call`, preserve the generated `next_step.arguments.meta.search_id`
   (or the same object as MCP `_meta`) so search-quality telemetry can
   correlate selected rank, hit-rate, and time-to-first-success without storing
   full prompts. Compact batch examples may also read
   per-result `token_accounting` metadata, and batch request items may carry an
   optional `id` that is echoed next to the numeric result `index`.
   Gateway traces, audit rows, logs, and stats retain the same token accounting
   fields (`response_format`, estimator, bytes, tokens, saved tokens, and
   savings percent) for both compact and legacy JSON traffic; tests should
   assert those fields without depending on full retained payload bodies.
   Admin health exposes the active default response format and token estimator,
   and default issue-report exports preserve bounded token accounting summaries
   so debugging artefacts carry cost context without raw response bodies.
   Gateway OTLP spans mirror the same agent workflow chain with bounded
   attributes: `gateway.search`, `gateway.describe`, `gateway.load_skill`,
   `gateway.call`, and `gateway.call_batch` use `openinference.span.kind` plus
   `dcc_mcp.*` fields for actor id/name/email hash, agent
   id/name/kind/version/model provider/model version/reasoning effort, client
   platform/os/host, auth subject, server-derived source ip/forwarded-for,
   turn id/task/tags, bounded user-intent and reply summaries, user/reply
   hashes and character counts, DCC route, `search_id`, selected
   rank/score/match reasons, policy outcome, and success/error kind. Adapter
   docs and examples should preserve those correlation fields but must not put
   hidden reasoning, secrets, raw prompts, raw agent replies, or unbounded
   request bodies into `agent_context` metadata.
   Raw prompt/reply capture belongs only in an explicitly configured traffic
   capture policy with redaction, sampling, retention, and clear Admin
   visibility.
   Gateway search uses a hybrid ranker in default fuzzy mode: weighted lexical
   matches over tool names, skill names, tags, summaries, author-declared
   aliases, and bounded schema-field tokens take precedence, while fuzzy
   fallback keeps typo tolerance. Search
   hits may include bounded `match_reasons` such as `tool_lexical`,
   `alias_lexical`, `schema_lexical`, `summary_fuzzy`, `schema_fuzzy`, and
   `multi_token_lexical`; use those for debugging relevance, but keep agent
   logic driven by `tool_slug`, `next_step`, and `describe` rather than
   hard-coding a single reason string. Full `input_schema` stays behind
   `describe`; the search path may carry only bounded internal
   `metadata.dcc.searchAliases` / `metadata.dcc.searchTokens` hints.
   Gateway capability policy is a deployment boundary, not an adapter-local
   convention. Read-only gateway mode still allows discovery and describe, but
   denies `load_skill`, `unload_skill`, tool-group changes, and backend calls
   unless the capability record declares `annotations.readOnlyHint = true`.
   DCC, skill, and canonical `tool_slug` allowlists filter search results and
   return stable `policy-denied` errors with `policy.reason` on describe, call,
   load, and batch result items. Adapter docs and examples should teach callers
   to respect that surface instead of working around it, and tool declarations
   must keep read-only annotations accurate because the gateway enforces them.
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
   capture plus optional `admin_live` in-memory inspection) over ad-hoc print
   logging. Redactions are write-time exact JSON
   paths such as `body.data.params.arguments.api_key`, and capture files can
   contain prompts, scene paths, and tool arguments, so keep them as local
   debugging artifacts unless redaction has been applied. Use
   `GET /v1/debug/traffic` to inspect retained `admin_live` frames and
   `GET /v1/debug/traffic/export` to download the retained window as JSONL.
   Use
   `dcc-mcp-server capture replay <capture> --target <gateway>/mcp` to replay
   captured client requests after a skill or prompt change, and
   `dcc-mcp-server capture diff <before> <after>` to compare observable
   traffic between two runs. When validating
   annotations, read-only mode, allowlists, quota pressure, or redaction
   behavior through the gateway, inspect `GET /v1/debug/governance` rather than
   guessing from a single failed call; it shows the effective policy, capture
   mode, redaction paths, middleware controls, and recent governance decisions.
   For gateway-facing examples, use the canonical four-tool MCP workflow:
   `search` / `describe` / `load_skill` / `call`. `call` accepts either one
   `tool_slug` with `arguments` or an ordered `{calls:[...]}` batch, matching
   REST `/v1/call` and `/v1/call_batch`. Hidden legacy names (`search_tools`,
   `describe_tool`, `call_tool`, `call_tools`, lease helpers) may appear in old
   clients, but new docs and skills should not advertise them in `tools/list`
   workflows.
   Unloaded gateway search hits now carry `load_state`, `available_groups`
   when the backend knows them, and `next_step` with both MCP and REST call
   shapes. Keep group metadata concise (`name`, short `description`, `tools`,
   `default-active`) so agents can decide whether to call `load_skill` directly
   or explicitly activate a heavier `tool_group` after the lazy default load.
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
   Prefer `AdapterReadinessBinder.bind_inline(...)`,
   `bind_headless(...)`, or `bind_queue_dispatcher(...,
   require_first_pump=True)` to publish and flip readiness bits. For custom
   wiring, flip `ReadinessProbe.host_execution_bridge` and
   `ReadinessProbe.main_thread_executor` only after that bridge path is
   actually usable; smoke tests may require those bits via
   `dcc-mcp-cli wait-ready --require host_execution_bridge,main_thread_executor`.
   Interactive UI adapters should subclass `HostUiDispatcherBase` instead of
   carrying local job-entry, queue, cancellation, timeout, and shutdown code.
   Implement `poke_host_pump()` and override only extension hooks such as
   `format_exception_error`, `format_timeout_error`, `on_job_queued`,
   `on_job_started`, `on_job_finished`, or constructor `label=...` for
   host-specific diagnostics. Use `queue_size()` and `active_count()` for
   health checks and pump stats instead of reaching into private queues.
   Use `HostPumpController` when the adapter needs reusable timer lifecycle
   around a `HostUiDispatcherBase` / `drain_queue(budget_ms)` pump. Core owns
   install/uninstall idempotency, `schedule_soon`, active/idle backoff,
   budget/overrun stats, and shutdown snapshots; the adapter supplies a tiny
   `HostPumpTimerAdapter` for the host primitive. Map Maya script jobs or
   `executeDeferred`, 3ds Max .NET/rollout timers, Blender
   `bpy.app.timers`, or generic Qt `QTimer` onto that adapter instead of
   duplicating pump controller code. Use `ManualHostTimerAdapter` in adapter
   conformance tests and `ThreadedHostTimerAdapter` for standalone/headless
   smoke tests.
   Qt-bearing sidecar adapters should import
   `dcc_mcp_core.qt_dispatcher.start_qt_server` (or the
   `dcc_mcp_core.host.qt_dispatcher` alias) and pass only host-specific
   `dispatch_handler` / `session_info_provider` hooks. Do not vendor
   `_qt_dispatcher.py`, `qt_bridge.py`, or another JSON-line TCP server in the
   adapter; the Rust `qtserver://` bootstrap embeds the package mirror that CI
   keeps byte-for-byte aligned with the public dispatcher module.
   When that sidecar dispatches script-backed skill actions, compose
   `dcc_mcp_core.sidecar.SidecarActionDispatcher` into the Qt
   `dispatch_handler` instead of copying payload validation, action lookup, or
   error-envelope code. Adapters should supply `server_provider`,
   `action_resolver`, and either
   `SidecarActionDispatcher.maya_executor(execute_in_process)` for
   `execute_in_process(server, script_path, args, action_name)` hosts or
   `SidecarActionDispatcher.script_executor(run_skill_script)` for
   3ds Max-style script runners. Keep direct `HostRpcClient` implementations
   only for native host RPC paths that are not script-backed skill dispatch.
   Use `docs/guide/adapter-dispatcher-migration.md` and the fake adapter tests
   in `tests/test_dispatcher_migration_conformance.py` as the minimum migration
   contract before live DCC smoke tests. Cover successful dispatch, malformed
   payloads, missing server/source, executor errors, cancellation, timeout, and
   shutdown cleanup without vendoring core dispatcher files into adapters.
10. Skill script entry points may use either modern `main(**params)` or legacy
    `main(params)` signatures; prefer `main(**params)` for new scripts and keep
    dict-style wrappers only for compatibility during adapter migrations.
11. When an adapter must adjust discovered skill metadata before registration,
    install `server.set_skill_load_transform(fn)` before exposing the server to
    agents. The transform receives a mutable `SkillMetadata` and applies to
    direct Python `load_skill`, MCP `tools/call load_skill`, REST
    `/v1/load_skill`, and multi-skill/group activation paths. Raise from the
    transform to veto before tools are registered. Use
    `set_after_load_skill_hook(fn)` only for adapter bookkeeping after
    registration, and keep `get_skill()` / `load_skill_object()` for explicit
    one-off object loads. Do not parse or rewrite `SKILL.md` / `tools.yaml` at
    adapter runtime. Keep `get_skill_info()` for serialized inspection only.
    Do not use this hook to clear `enforce_thread_affinity` for batch hosts.
12. In mayapy/hython/batch hosts where no GUI dispatcher exists but the
    adapter has verified the in-process lane is safe for DCC API calls, pass
    `standalone_main_thread=True` to `DccServerOptions.from_env(...)` instead
    of mutating loaded skill metadata. This core-owned opt-in registers the
    inline in-process executor before discovery and lets MCP `tools/call` plus
    REST `/v1/call` satisfy `thread_affinity: main` tools. Do not use it for
    GUI sessions; wire `HostExecutionBridge(dispatcher=...)`,
    `QueueDispatcher`, or `BlockingDispatcher` there.
13. For ad-hoc script execution, prefer file-backed boundaries. Use
    `materialize_script(...)` (Python) or
    `ScriptMaterializationStore` (Rust) to create host-local scripts under the
    DCC/session-scoped root before calling `execute_python(file_path=...)` or
    equivalent host tools. When an adapter accepts inline `code`, call
    `normalize_file_backed_script_execution_params(...)` or
    `HostExecutionBridge.prepare_script_execution_params(...)` and choose a
    policy: `auto` to materialize inline code, `require` to reject inline code,
    or `off` only for temporary migration. Keep `write_temp_script()` only as a
    compatibility wrapper. Tool results and audit contexts should preserve the
    descriptor's `file_ref`, `file_path`, `sha256`, byte length, TTL, session,
    tool-call, correlation, and reuse metadata under
    `context.materialized_script`.
    `DccServerBase` exposes a discoverable `materialize_script` MCP/REST tool
    for agents; prefer that tool when the agent needs to create the host-local
    file before a later execution call. New script execution tool schemas must
    include `file_path` or `script_path` when they accept inline `code`;
    validation warns on unbounded inline-only code schemas.
14. Add tests at the lowest executable layer, then one discovery/load/call or
    gateway REST path when behavior crosses MCP or REST boundaries.
15. For application UI automation, use the generic `app_ui__*` contract rather
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
16. For headless USD/OpenUSD-style adapters, expose filesystem-backed project
    state with `register_usd_project_resources(...)` instead of inventing
    adapter-local URI shapes. Keep the canonical `openusd://stage`,
    `openusd://layers`, `openusd://assets`, `openusd://materials`,
    `openusd://validation`, `openusd://snapshots`, and `openusd://packages`
    families DCC-agnostic so Houdini Solaris, Maya USD, Blender USD, Unreal,
    and pure OpenUSD adapters can share tests and agent workflows.

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
