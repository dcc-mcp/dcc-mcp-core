---
name: dcc-mcp-creator
description: >-
  Infrastructure skill - guide developers and agents through creating or
  modernizing a full DCC-MCP adapter for Nuke, Blender, 3ds Max, Unreal,
  ZBrush, Houdini, Maya, and custom studio tools. Use when building server,
  dispatcher, gateway, packaging, and runtime integration. Not for authoring
  individual SKILL.md tool packages - use dcc-mcp-skills-creator.
license: MIT-0
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
    version: "0.19.69"
    search-hint: >-
      create DCC MCP adapter, Nuke MCP, DccServerBase, HostExecutionBridge,
      dispatcher, readiness, resources, gateway, Blender, 3ds Max, Unreal,
      ZBrush, Houdini, Maya, chunked main-thread jobs, cooperative cancellation
    tags: "adapter-development, host-runtime, dispatcher, gateway, nuke, blender, 3dsmax, unreal, zbrush"
    skill-reference-docs:
      - "references/*.md"
  openclaw:
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/dcc-mcp-creator/SKILL.md
---

# DCC-MCP Creator

Use this skill when you are creating a new DCC-MCP adapter or modernizing an
existing adapter repository: server composition, host-thread dispatch,
sidecar/gateway wiring, readiness, resources, project state, diagnostics,
install lifecycle, or cross-DCC verification.

For individual skill packages (`SKILL.md`, `tools.yaml`, scripts, groups, and
skill taxonomy), load `dcc-mcp-skills-creator` instead.

## CLI-First Control Path

Use the `dcc-mcp` skill and `dcc-mcp-cli` for discovery, validation, and live
DCC control whenever the agent can run shell commands. If the CLI is missing,
follow the consent-gated official installation instructions in `dcc-mcp`.
Before long-lived validation, run `dcc-mcp-cli update check`; use
`dcc-mcp-cli update apply` to stage the latest CLI for the next launch. This
does not replace a running server binary.

## Runtime Vocabulary

- DCC startup hook: adapter code running inside the host at application startup; it prepares env/instance data and launches the service path without blocking the DCC UI/main thread.
- Per-DCC service: one registered runtime row for one concrete DCC instance; Python `DccServerBase` and Rust sidecars both participate as per-DCC services.
- Sidecar: the Rust `dcc-mcp-sidecar` child launched through the stable `dcc-mcp-server sidecar` command; it bridges host RPC to MCP/REST and exits when the watched DCC dies.
- Gateway daemon: the one machine-wide `dcc-mcp-server gateway` process that owns routing, dynamic capability search/describe/call, and Gateway Admin.
- Guardian: a lightweight loop inside daemon-backed services that probes gateway `/health` and re-ensures the daemon through `gateway-launch.lock`; it is not a separate process.
- Service heartbeat: registry freshness for the service row only. Do not describe heartbeat as the gateway restart trigger.
- Service owner: the process that owns the registry sentinel and MCP endpoint; its `pid`/sentinel prove the service itself is alive.
- Bound DCC host: optional external process identified by `host_pid`; both owner and host must stay alive. Standalone/headless services intentionally have no bound host.

## Fast Workflow

1. Run `dcc-mcp-cli dcc-types` before creating a repository. If the DCC type is
   already cataloged, improve that adapter instead of creating a duplicate.
   Custom types remain supported; add the new adapter to `dcc-mcp-catalog.yml`
   and the compatibility matrix in the same core PR.
2. Classify the host integration:
   - Embedded Python host: Blender, 3ds Max Python, Houdini, Maya, Nuke.
   - External bridge host: ZBrush, Photoshop, Unity, custom tools.
   - Game/editor host with mixed Python or C++ bridge: Unreal, Unity.
3. Read the relevant reference:
   - [ADAPTER_WORKFLOW.md](references/ADAPTER_WORKFLOW.md) for the build path.
   - [HOST_PATTERN_MATRIX.md](references/HOST_PATTERN_MATRIX.md) for host-specific wiring.
   - [CORE_ESCALATION_CHECKLIST.md](references/CORE_ESCALATION_CHECKLIST.md) before adding adapter-local glue.
    - [TESTING_AND_RELEASE.md](references/TESTING_AND_RELEASE.md) before validating or publishing.
    - **Python 3.7 policy**: native py37 is an LTS profile with no automatic calendar expiry. Verify the aggregate Python 3.7 gate is green and `requires-python = ">=3.7"` is unchanged before any release. `py37-lite` fallback does NOT satisfy release gates. Removal requires an accepted superseding ADR, a major release, and at least 180 days of notice.
    - [docs/guide/gateway.md](../../docs/guide/gateway.md) for gateway daemon lifecycle details.
    - [docs/guide/adapter-install-lifecycle.md](../../docs/guide/adapter-install-lifecycle.md) for sidecar launch/readiness details.
    - [docs/guide/adapter-release-checklist.md](../../docs/guide/adapter-release-checklist.md) for release train compliance.
    - [docs/guide/new-adapter-onboarding.md](../../docs/guide/new-adapter-onboarding.md) for new adapter scaffolding.
    - [docs/guide/adapter-compatibility-matrix.md](../../docs/guide/adapter-compatibility-matrix.md) for the per-DCC compatibility table.
4. Start from `DccServerBase` + `DccServerOptions.from_env(...)`.
   Classify the runtime lifetime explicitly:
   - Embedded adapter: the service owner is the DCC process; no separate host PID is needed.
   - Standard sidecar: pass `watch_pid=current_dcc_pid`; core publishes the sidecar owner and bound host as separate liveness signals.
   - Other out-of-process adapter: pass `dcc_pid=current_dcc_pid` so `McpHttpConfig.host_pid` binds discovery to the DCC lifetime.
   - Standalone/headless service: pass `instance_type="standalone"`, leave `dcc_pid` unset, and do not bind it to an optional GUI process. Runtime identity is independent from `standalone_main_thread`, which controls tool execution only.
5. Route host API calls through `HostExecutionBridge`; do not hand-roll a second script executor.
6. Keep DCC identity data-driven: `dcc_name`, `server_name`, env-var prefix, skill names, and gateway metadata.
   Leave the instance port unset so core resolves `DCC_MCP_<DCC>_PORT` or asks the OS for a free port.
7. Use core helpers for skill discovery, `MinimalModeConfig`, project tools, resources, diagnostics, context snapshots, install lifecycle, and gateway failover before writing adapter-local wrappers. Python `DccServerBase.collect_skill_search_paths()` includes marketplace-installed skills under `~/.dcc-mcp/marketplace/<dcc>` (or `DCC_MCP_MARKETPLACE_INSTALL_ROOT/<dcc>`) when the directory exists, so adapters should not add a second marketplace path convention. Hermetic adapter tests should set `DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS=1`; this excludes implicit local/platform defaults, marketplace installs, and Admin custom paths while explicit, bundled, and environment-provided skill paths remain active.
   - For Windows visual UI fallback, reuse the bundled `ui-control` skill and the
     isolated `dcc-mcp-ui-control-host.exe`; do not instantiate
     `ComputerUseSession` or add adapter-local screenshot/`SendInput` wrappers.
     Keep `ui_control__snapshot` and `ui_control__act` in the same long-lived adapter
     process so one thin named-pipe client retains the opaque host capability.
     The per-logon-session host owns the screenshot/UIA observations, visible
     banner, Esc stop token, input owner, confirmation, and native input.
     `ui-control` declares `requires_in_process: true` while keeping
     `affinity: any`; register `HostExecutionBridge` before skill loading.
     Never weaken this into per-call subprocess execution or route it onto a
     blocked DCC UI thread.
     A minimized or hidden exact HWND is recovered only through the host-owned
     `get_window_state` / `restore_window` / `show_window` /
     `activate_window` actions. They need no snapshot, must retain the existing
     capability-bound PID/HWND, and must never become desktop discovery or
     adapter-local Win32/input fallbacks.
   - Keep structured DCC skills, host APIs, and adapter scripts ahead of
     `ui-control`. Agents should make an explicit, agent-directed transition into the scoped
     `snapshot` → one `act` → `snapshot` loop only when an operation is
     unsupported, no suitable tool exists, or semantic UI Automation cannot
     reach the required control. Re-observe after every action.
   - Keep raw pointer and keyboard input operator-controlled. The adapter may
     document `DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT`, but it must not enable the
      ceiling itself. Before raw input can start, the adapter/operator must set
      `DCC_MCP_UI_CONTROL_UIA_PROCESS_ID` or `DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE` to
      the adapter's own DCC target; request scope may only narrow that trusted
      PID/HWND. Require a visible unlocked desktop and matching Windows
      integrity level, preserve the click-through border/banner/pointer feedback, and preserve
      `user_interrupted` without automatic retry, `session_id` changes, or fallback. Once Esc stops an
      session, only `ui_control__snapshot(resume_computer_use=true)` may request a
      resume, and the isolated host must still obtain trusted user confirmation
      before clearing the latch. Always call `ui_control__stop_computer_use` when
      the workflow ends.
      Never transition or retry through another UI/input path after a policy,
      authorization, authentication, security, confirmation,
      `desktop_unavailable`, or `user_interrupted` result.
      Keep mutating UI Control tools annotated as destructive. The optional
      `intent` may only raise the native host's independent UIA/input
      classification. Do not introduce a model-supplied `confirmed`/`approved`
      flag or environment bypass.
   - For plug-in setup outside a window, reuse
     `ui_control__system_operation` instead of adding adapter-local PowerShell,
     `reg.exe`, `mklink`, or generic file-system tools. The host catalog named
     by `DCC_MCP_UI_CONTROL_SYSTEM_GRANTS_FILE` is operator-owned, and the
     adapter selects only its grant id through
     `DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID`. Keep operations exact, typed,
     idempotent, confirmation-gated, and free of credentials. Do not treat
     `elevation_required` as permission to automate UAC or another shell path.
8. Use CLI profiles (`dcc-mcp-cli gateway ...`, `list/search/describe/call`) as the user UX; treat `dcc-mcp-server` modes as runtime plumbing. Read `docs/guide/gateway.md` before changing daemon, guardian, sentinel, registry, or idle-timeout behavior.
   `gateway://instances` is agent-safe by default and returns only live,
   routable rows. Use `?include_stale=true`, `?include_dead=true`, or
   `?view=all` only for explicit diagnosis; never route a call from those
   expanded operator views without re-validating live readiness.
   Once an instance is selected, reuse `gateway://instances/{instance_id}` or
   `GET /v1/instances/{instance_id}/context` for live process/machine
   performance, scene/documents, loaded skills, and canonical follow-up routes.
9. Use `dcc_mcp_core.install_lifecycle.build_sidecar_command(...)` / `launch_sidecar(...)` for sidecar startup and readiness. Read `docs/guide/adapter-install-lifecycle.md` before changing host RPC, dispatch readiness, launch stdio, `watch_pid`, or `instance_id` handling.
   - The sidecar MCP listener is dispatch-only. A py37-lite factory can expose local skill metadata, but it cannot advertise or activate declarative skills through the gateway. Require a native py37 wheel for that path, or provide a separate discovery MCP URL; never report lite `load_skill` success without an executable catalog.
10. Pass `instance_id` to sidecar launch helpers only when it is a real UUID for the DCC service. During early startup, omit it or pass `None`; `build_sidecar_command()` rejects cosmetic values such as `"unknown"` with `success=false` and `reason="invalid_instance_id"` so adapters do not spawn a child that can only fail with a CLI argument error.
11. Adapter supervisors that must stop the sidecar on plugin unload should call `launch_sidecar(..., return_process=True, detached=False)` instead of reimplementing `subprocess.Popen`; keep `return_process=False` for CLI/JSON paths because the process handle is not serializable.
12. If the adapter cannot share the gateway `FileRegistry`, register remotely through `POST /v1/instances/register`, refresh with `/heartbeat`, and deregister on shutdown; the gateway will expose the row as `source: "http"` in `gateway://instances` / `GET /v1/instances`, preserve `instance_short` and `mcp_url`, and route it through the same `live_instances` contract.
13. For same-LAN convenience discovery, build with `mdns` and pair adapter-side `--advertise-mdns` with gateway-side `--discover-mdns`; treat this as a multicast discovery hint only, keep auth/TLS policy explicit, and prefer HTTP registration or relay for routed/subnet-crossing production deployments.
14. For NAT or routed-subnet deployments, run the tunnel agent with stable `instance_id`, `capabilities_fingerprint`, `adapter_version`, and `scene` metadata, then configure the standalone gateway with `--relay-source ADMIN_URL=PUBLIC_BASE_URL`; the gateway will expose active tunnels as `source: "relay"` rows with relay details in `source_meta` after probing `/v1/healthz` through `<PUBLIC_BASE_URL>/tunnel/<tunnel_id>/mcp`.
15. Preserve gateway caller attribution when adding adapter wrappers or admin/debug routes: let MCP `initialize.params.clientInfo`, MCP `_meta.agent_context`, REST `meta.agent_context`, `x-dcc-mcp-*` headers, and safe `User-Agent` fallbacks flow through core rather than logging raw prompts or local machine data.
16. For lifecycle/memory/telemetry policy, use `register_lifecycle_hooks(...)`, `search_skills(..., session_id=...)`, `dispatch_session_start(...)`, `dispatch_before_tool_call(...)`, `dispatch_after_tool_call(...)`, and `dispatch_session_end(...)`; pair `MemoryRecorder(InMemoryMemoryStore()).install(hooks)` with those hooks when adapters need bounded memory summaries, failed-pattern avoidance, or session compaction. Memory injection is conservative and budgeted by default: search receives compact ranking hints, tool calls receive memory only when it matches the current `tool_name`, and session-start injection is opt-in. Use `SqliteMemoryStore()` only when longterm patterns should be durable, operator-managed in the Admin Memory tab, and included in memory hit-rate observability; disable the recorder for privacy-sensitive deployments. Open a focused core issue/RFC only when those public hooks cannot express the adapter boundary.
17. Add one executable smoke path: unit tests for construction plus either headless DCC, mock dispatcher MCP calls, gateway REST replay, mDNS same-LAN discovery smoke, relay-source smoke, or `just idle-memory-smoke` for standalone server idle/regression checks.
18. For gateway/admin observability, surface explicit state instead of silent zeroes: traffic panels should report disabled, unavailable, filtered, or genuine no-traffic states; skill panels should distinguish discovered, loaded, searched, selected, called, failed, and low-adoption skills; and admin-facing frames/paths should stay metadata-only or aliased unless an operator explicitly configures a private raw sink. Keep `ServiceEntry.version` as the DCC application version; use core-published `dcc_mcp_server_version` and `dcc_mcp_instance_type=gui|standalone` metadata for server regression and runtime-shape diagnostics instead of overloading DCC or adapter versions.
19. Preserve workflow observability: adapter calls should carry request, parent, trace, session, DCC, transport, and artifact/validation metadata so the Admin workflow graph can show Intent → Discovery → Skill Load → Tool Calls → Fallbacks → Artifacts → Validation → Report without raw log reading.
20. Preserve bounded `agent_context` task/session/turn metadata and artifact/validation-friendly tool names so Admin task outcomes can group workflows, calls, deliverables, and checks without reading raw payloads or local paths.
21. Preserve record-replay ownership boundaries: forward server-derived
    `agent_context.session_id`, keep UI Control logical ids connection/caller
    scoped, and write only redacted recording projections to existing
    `session_events`. Do not add adapter-local recorder state, a second
    database, or a replay authority flag. Generated workflows re-resolve
    current tools and schemas; semantic UI replay resolves fresh control ids;
    raw/visual fallback requires exact-window calibration and drift guards.

## Chunked Main-Thread Jobs

Use the shared chunked path when a main-affinity operation cannot finish within
one host UI tick. The adapter owns scheduling; skill code only defines bounded
steps:

```python
from dcc_mcp_core import chunked_job

@chunked_job(total=100)
def bake_frames():
    for frame in range(100):
        yield lambda frame=frame: bake_one_frame(frame)

# A declarative in-process tool returns this runner. HostExecutionBridge
# detects and submits it to HostUiDispatcherBase automatically.
return bake_frames()
```

- Declare `execution: async`, `affinity: main`, and
  `job_strategy: chunked`. The bridge rejects a declared chunked tool that
  returns a monolithic value.
- Yield one bounded host-API callable per step. A returned string becomes the
  progress message.
- `submit_chunked_runner()` advances at most one step per host pump tick, so
  unrelated UI work can run between steps.
- `cancel(request_id)` requests cancellation. The runner publishes
  `cancelled` only after the next checkpoint observes it; a running native DCC
  call or monolithic callback is not pre-empted.
- Do not add adapter-local generator pumps, timer loops, worker threads, or a
  second job registry.
- Test pending cancellation, cancellation during a step, monotonic progress,
  failure, exactly one terminal result, unrelated pump work, and at least two
  host labels.

Do not label an indivisible native call as chunked. Use
`job_strategy: monolithic` when the host API cannot yield, or
`job_strategy: isolated` when a process/service-owned operation can return a
durable job id. Isolated status must remain queryable after transport loss;
cancellation may remain process-owner scoped when reconstructing ownership
would be unsafe.

## Liveness and Crash Recovery

- Keep registry heartbeat and HTTP readiness independent of the DCC main
  thread. A readiness/transport timeout marks the instance `unreachable`; it
  must not erase a row whose owner lock/PID or remote TTL is still valid.
- Treat owner lock/PID death or remote TTL expiry as crash evidence. After a
  crash, the adapter cannot reconnect until the DCC or sidecar starts again.
- Preserve stable `dcc_type`, scene/project metadata, and adapter identity so
  agents can rediscover a replacement instance. Never reuse an old tool slug
  or direct MCP URL after the instance id changes.
- Enable core job persistence. On restart, in-flight core jobs become
  `interrupted` and remain queryable through the replacement instance's
  `jobs_get_status`; adapter-owned isolated jobs need their own durable status
  tool when they outlive the request transport.

## Example: New Nuke Adapter

When asked to create a Nuke MCP adapter, start by mapping the host lifecycle:
how Python is loaded, how the UI/main thread must be entered, what headless
mode is available, how plugins are installed, and which operations should be
bundled as default skills. Then scaffold the adapter around core primitives:

- `DccServerBase` for MCP/HTTP and skill catalog behavior.
- `DccServerOptions.from_env("NUKE")` or an adapter-specific equivalent for env-driven configuration.
- `HostExecutionBridge` plus a Nuke dispatcher for all Nuke API calls.
- Core project, readiness, resource, diagnostics, and gateway helpers before adapter-local glue.
- `dcc-mcp-skills-creator` for the first `nuke-*` skill packages.

## Non-Negotiables

- Do not touch a DCC API from a Tokio/HTTP worker thread.
- Do not parse or rewrite `SKILL.md`, `tools.yaml`, `groups.yaml`, or prompt/workflow files in adapter runtime code when core exposes a typed object or catalog API.
- Do not reach into `server._server` unless no public core API exists; if you must, file a core issue and keep the adapter shim small.
- Do not create Maya-only abstractions in shared core or adapter templates.
- Do not expose raw script execution as the primary user workflow when a typed skill can cover the task.
- Do not publish local paths, private machine names, or source-attribution markers in public issues or PR text.
