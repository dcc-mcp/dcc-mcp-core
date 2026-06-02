---
name: dcc-mcp-creator
description: >-
  Infrastructure skill - guide developers and agents through creating or
  modernizing a full DCC-MCP adapter for Nuke, Blender, 3ds Max, Unreal,
  ZBrush, Houdini, Maya, and custom studio tools. Use when building server,
  dispatcher, gateway, packaging, and runtime integration. Not for authoring
  individual SKILL.md tool packages - use dcc-mcp-skills-creator.
license: MIT-0
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "0.17.50"  # x-release-please-version
    search-hint: >-
      create DCC MCP adapter, Nuke MCP, DccServerBase, HostExecutionBridge,
      dispatcher, readiness, resources, gateway, Blender, 3ds Max, Unreal,
      ZBrush, Houdini, Maya
    tags: "adapter-development, host-runtime, dispatcher, gateway, nuke, blender, 3dsmax, unreal, zbrush"
    skill-reference-docs:
      - "references/*.md"
  openclaw:
    homepage: https://github.com/loonghao/dcc-mcp-core/blob/main/skills/dcc-mcp-creator/SKILL.md
---

# DCC-MCP Creator

Use this skill when you are creating a new DCC-MCP adapter or modernizing an
existing adapter repository: server composition, host-thread dispatch,
sidecar/gateway wiring, readiness, resources, project state, diagnostics,
install lifecycle, or cross-DCC verification.

For individual skill packages (`SKILL.md`, `tools.yaml`, scripts, groups, and
skill taxonomy), load `dcc-mcp-skills-creator` instead.

## Fast Workflow

1. Classify the host integration:
   - Embedded Python host: Blender, 3ds Max Python, Houdini, Maya, Nuke.
   - External bridge host: ZBrush, Photoshop, Unity, custom tools.
   - Game/editor host with mixed Python or C++ bridge: Unreal, Unity.
2. Read the relevant reference:
   - [ADAPTER_WORKFLOW.md](references/ADAPTER_WORKFLOW.md) for the build path.
   - [HOST_PATTERN_MATRIX.md](references/HOST_PATTERN_MATRIX.md) for host-specific wiring.
   - [CORE_ESCALATION_CHECKLIST.md](references/CORE_ESCALATION_CHECKLIST.md) before adding adapter-local glue.
   - [TESTING_AND_RELEASE.md](references/TESTING_AND_RELEASE.md) before validating or publishing.
3. Start from `DccServerBase` + `DccServerOptions.from_env(...)`.
4. Route host API calls through `HostExecutionBridge`; do not hand-roll a second script executor.
5. Keep DCC identity data-driven: `dcc_name`, `server_name`, env-var prefix, skill names, and gateway metadata.
6. Use core helpers for skill discovery, `MinimalModeConfig`, project tools, resources, diagnostics, context snapshots, install lifecycle, and gateway failover before writing adapter-local wrappers.
7. Choose the `dcc-mcp-server` run mode deliberately: no subcommand, `auto`, and `serve` ensure the machine-wide gateway daemon, register this process as a backend, and keep a lightweight guardian running after registration; `serve --no-auto-gateway` is per-DCC only, `auto --legacy-gateway-election` restores the old embedded first-wins gateway, and `gateway` runs the machine-wide daemon with no inline DCC execution. Python `DccServerBase` adapters, Rust `dcc-mcp-server sidecar` processes with `gateway_port > 0`, and registered `dcc-mcp-server translate` bridges follow the same daemon-first contract at startup and keep the same daemon guardian running in daemon-backed mode so a surviving DCC or bridge can re-ensure the daemon after `/health` disappears; the shared `gateway-launch.lock` is reclaimed after `DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS` if a launcher crashes mid-startup, so adapters should share `DCC_MCP_REGISTRY_DIR` and not delete the lock manually. `dcc_diagnostics__gateway_failover` should report `daemon-backed` or `embedded-fallback` so operators can distinguish the mode.
8. When a DCC startup hook must spawn the per-DCC sidecar without importing native core, use `dcc_mcp_core.install_lifecycle.build_sidecar_command(...)` or `launch_sidecar(...)`; they produce the canonical `dcc-mcp-server sidecar` argv, keep stdio detached by default, stamp the shared registry/gateway env, and let the child ensure the daemon without blocking the DCC main process. Do not treat a `per-dcc-sidecar` registry row alone as proof that tools are callable: the adapter must expose a supported host RPC bridge to its DCC dispatcher/skills, and startup code or installers should use `sidecar_readiness_status(...)` / `wait_for_sidecar_ready(...)` for a bounded import-light verdict before claiming the plugin is ready. Pass `probe_tool="<dcc>_diagnostics__ping"` (or CLI `--probe-tool`) when the adapter needs proof that one real `tools/call` reaches the host dispatcher; probe failures keep the verdict unready until timeout. Operators should see `dispatch_ready=True` from those helpers, `query_runtime_state(...)`, `GET /admin/api/workers`, or raw `metadata.dispatch_status=ready` plus `metadata.mcp_url`; failed host RPC startup stays registered with `dispatch_ready=False`, `metadata.dispatch_status=unavailable`, and `failure_stage` / `failure_reason` for diagnostics. A failed sidecar may still publish `metadata.mcp_url` as a diagnostic endpoint: `/v1/readyz` must report `dispatcher=false`, and `tools/call` must return a structured `transport-error`; do not route gateway traffic through it until `dispatch_status=ready`. Maya `commandport://` sidecars return a structured `sidecar-dispatcher-unavailable` backend envelope when `dcc_mcp_maya` is present but its sidecar dispatcher is missing, so treat that as a partial adapter install rather than a gateway routing failure. If a ready backend later returns terminal `host-died`, gateway removes that instance from the capability index plus its FileRegistry or HTTP registration row immediately; do not design adapters around waiting for stale heartbeat cleanup after a terminal host failure.
9. If the adapter cannot share the gateway `FileRegistry`, register remotely through `POST /v1/instances/register`, refresh with `/heartbeat`, and deregister on shutdown; the gateway will expose the row as `source: "http"` in `gateway://instances` / `GET /v1/instances`, preserve `instance_short` and `mcp_url`, and route it through the same `live_instances` contract.
10. For same-LAN convenience discovery, build with `mdns` and pair adapter-side `--advertise-mdns` with gateway-side `--discover-mdns`; treat this as a multicast discovery hint only, keep auth/TLS policy explicit, and prefer HTTP registration or relay for routed/subnet-crossing production deployments.
11. For NAT or routed-subnet deployments, run the tunnel agent with stable `instance_id`, `capabilities_fingerprint`, `adapter_version`, and `scene` metadata, then configure the standalone gateway with `--relay-source ADMIN_URL=PUBLIC_BASE_URL`; the gateway will expose active tunnels as `source: "relay"` rows with relay details in `source_meta` after probing `/v1/healthz` through `<PUBLIC_BASE_URL>/tunnel/<tunnel_id>/mcp`.
12. Preserve gateway caller attribution when adding adapter wrappers or admin/debug routes: let MCP `initialize.params.clientInfo`, MCP `_meta.agent_context`, REST `meta.agent_context`, `x-dcc-mcp-*` headers, and safe `User-Agent` fallbacks flow through core rather than logging raw prompts or local machine data.
13. For lifecycle/memory/telemetry policy, use `register_lifecycle_hooks(...)`, `search_skills(..., session_id=...)`, `dispatch_session_start(...)`, `dispatch_before_tool_call(...)`, `dispatch_after_tool_call(...)`, and `dispatch_session_end(...)`; pair `MemoryRecorder(InMemoryMemoryStore()).install(hooks)` with those hooks when adapters need bounded memory summaries, failed-pattern avoidance, or session compaction, and disable the recorder for privacy-sensitive deployments. Open a focused core issue/RFC only when those public hooks cannot express the adapter boundary.
14. Add one executable smoke path: unit tests for construction plus either headless DCC, mock dispatcher MCP calls, gateway REST replay, mDNS same-LAN discovery smoke, relay-source smoke, or `just idle-memory-smoke` for standalone server idle/regression checks.
15. For gateway/admin observability, surface explicit state instead of silent zeroes: traffic panels should report disabled, unavailable, filtered, or genuine no-traffic states; skill panels should distinguish discovered, loaded, searched, selected, called, failed, and low-adoption skills; and admin-facing frames/paths should stay metadata-only or aliased unless an operator explicitly configures a private raw sink.
16. Preserve workflow observability: adapter calls should carry request, parent, trace, session, DCC, transport, and artifact/validation metadata so the Admin workflow graph can show Intent → Discovery → Skill Load → Tool Calls → Fallbacks → Artifacts → Validation → Report without raw log reading.
17. Preserve bounded `agent_context` task/session/turn metadata and artifact/validation-friendly tool names so Admin task outcomes can group workflows, calls, deliverables, and checks without reading raw payloads or local paths.

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
