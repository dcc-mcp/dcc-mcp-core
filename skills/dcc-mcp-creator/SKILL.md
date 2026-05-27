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
    version: "0.17.37"  # x-release-please-version
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
7. Choose the `dcc-mcp-server` run mode deliberately: no subcommand or `auto` for backwards-compatible first-wins auto-gateway, `serve --no-auto-gateway` when a separate daemon owns the gateway port, and `gateway` for a machine-wide daemon with no inline DCC execution.
8. If the adapter cannot share the gateway `FileRegistry`, register remotely through `POST /v1/instances/register`, refresh with `/heartbeat`, and deregister on shutdown; the gateway will expose the row as `source: "http"` and route it through the same `live_instances` contract.
9. Preserve gateway caller attribution when adding adapter wrappers or admin/debug routes: let MCP `initialize.params.clientInfo`, MCP `_meta.agent_context`, REST `meta.agent_context`, `x-dcc-mcp-*` headers, and safe `User-Agent` fallbacks flow through core rather than logging raw prompts or local machine data.
10. For lifecycle/memory/telemetry policy, use `register_lifecycle_hooks(...)`, `search_skills(..., session_id=...)`, `dispatch_session_start(...)`, `dispatch_before_tool_call(...)`, `dispatch_after_tool_call(...)`, and `dispatch_session_end(...)`; pair `MemoryRecorder(InMemoryMemoryStore()).install(hooks)` with those hooks when adapters need bounded memory summaries, failed-pattern avoidance, or session compaction, and disable the recorder for privacy-sensitive deployments. Open a focused core issue/RFC only when those public hooks cannot express the adapter boundary.
11. Add one executable smoke path: unit tests for construction plus either headless DCC, mock dispatcher MCP calls, gateway REST replay, or `just idle-memory-smoke` for standalone server idle/regression checks.
12. For gateway/admin observability, surface explicit state instead of silent zeroes: traffic panels should report disabled, unavailable, filtered, or genuine no-traffic states; skill panels should distinguish discovered, loaded, searched, selected, called, failed, and low-adoption skills; and admin-facing frames/paths should stay metadata-only or aliased unless an operator explicitly configures a private raw sink.
13. Preserve workflow observability: adapter calls should carry request, parent, trace, session, DCC, transport, and artifact/validation metadata so the Admin workflow graph can show Intent → Discovery → Skill Load → Tool Calls → Fallbacks → Artifacts → Validation → Report without raw log reading.
14. Preserve bounded `agent_context` task/session/turn metadata and artifact/validation-friendly tool names so Admin task outcomes can group workflows, calls, deliverables, and checks without reading raw payloads or local paths.

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
