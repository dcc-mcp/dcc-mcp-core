# Adapter Runtime Contracts

Core exposes small, DCC-agnostic contracts for runtime material that agents
need while tools and jobs are running. Adapters keep host-specific collection
and safety policy; core standardizes the shapes and resource hand-off paths.

## Readiness publication and recovery

For an embedded `McpHttpServer`, publish `AdapterReadinessBinder` **before
`start()`**. The listener captures that probe at startup; changing its bits
is live, but publishing a different probe after startup does not replace the
running listener's probe. A successful setter is not a live-HTTP verification.

- `publish()` and `published` describe acceptance of the latest publication
  attempt. An explicit setter result of `False` means rejection; the native
  setter returns `None` on success. Missing setters and exceptions also fail
  publication. Failed attempts do not revoke a previously installed probe.
- The pure-Python `SidecarBackedSkillServer` facade returns `False`: it cannot
  transfer an in-process probe into the separate Rust server. Both the binder
  and `DccServerBase.set_readiness_probe()` preserve that rejection. A green
  local `binder.report_subset()` does not prove sidecar HTTP readiness.
  An adapter process using the embedded extension remains the embedded case,
  even when that adapter itself runs outside the native DCC process.
- `dcc_ready_probe` is sampled during binding and by
  `binder.refresh_dcc_ready()`, not automatically on each HTTP request or by
  a background timer. The adapter owns refresh scheduling. Callback
  exceptions set `dcc=False`; a later successful refresh can recover it.
- `bind_queue_dispatcher(server, dispatcher, require_first_pump=True)` needs
  the actual dispatcher. The binder posts a callback and waits for a pump; it
  does not attach the dispatcher or pump the queue itself. Wire the execution
  path before startup and avoid waiting on the only thread that can pump it.
- Base admission requires `process`, `dcc`, `skill_catalog`, and `dispatcher`.
  `host_execution_bridge` and `main_thread_executor` are separately reported
  execution capabilities. A base-ready result alone does not verify usable
  main-thread execution or a native DCC operation.

The shared base probe gates REST `/v1/call` and registered MCP `tools/call`
dispatch, including a recovery tool hosted behind that same gate. A red
`dcc` bit returns HTTP 503 / MCP `-32002` before those handlers execute.
Do not make the only way to restore readiness depend on such a tool. Keep
recovery on an independent, explicitly authorized path; do not mark the host
ready merely to bypass the gate. This contract does not add a per-tool bypass
or permit automatic dialog clicks or destructive confirmations.

Validate an isolated HTTP `ready → blocked → ready` transition as well as the
actual execution path. Loopback Core tests, a responding sidecar process, and
native DCC/GUI acceptance are different evidence and must be reported separately.

## Session events

Use `SessionEventBuffer` for bounded stdout/stderr/log/progress/checkpoint
events. Register it as an MCP resource:

```python
from dcc_mcp_core import SessionEventBuffer

events = SessionEventBuffer("maya-001", maxlen=1000, max_message_bytes=4096)
server.resources().register_session_event_buffer(events)
events.append("python", "stdout", "Created rig control", tool_call_id="req-1")
```

Clients read `events://session/maya-001?cursor=N&limit=100`. The response
includes `next_cursor`, so clients avoid duplicate events without needing a
live subscription. `drain=true` is available for clients that want
consume-on-read behavior.

## Artefact references

Use the existing `FileRef` / `ArtefactStore` path for large or binary outputs:

- `artefact://sha256/<hex>` never exposes adapter filesystem paths.
- Sidecars carry MIME, size, digest, display name, session/tool/job/correlation
  fields, expiry, and adapter metadata.
- Bounded stores can enforce max payload bytes, max retained entries, max total
  bytes, and default TTL.

Tool results should return the small `FileRef` object in context and let
clients fetch bytes through `resources/read`.

## Debug descriptors

Use `DebugSessionDescriptor` to publish optional attach metadata without
adding a hard debugger dependency to core. The descriptor supports
`unavailable`, `available`, `listening`, `client_connected`, and `error`
states, plus host/port, runtime/process identity, path mappings, log URI,
setup instructions, and adapter metadata.

Python adapters can use:

```python
from dcc_mcp_core import DebugSessionDescriptor

descriptor = DebugSessionDescriptor.listening("debugpy", "127.0.0.1", 5678)
```

Publish the resulting `descriptor.to_dict()` through a docs/custom resource or
an adapter-owned optional tool.

## UI Control automation contract

The `ui_control` contract is a schema and workflow, not a universal click bot.
Adapters may implement it with Qt, native accessibility APIs, webviews, or
DCC-specific UI APIs. The public tool names use `ui_control__*` because the
capability is intentionally broader than a DCC-only UI namespace: the same
contract can describe a DCC preferences dialog, an external launcher, a license
utility, or another adapter-owned application window.

The Core-facing schema lives in `dcc_mcp_core.adapter_contracts`; native
automation is supplied by the standalone `dcc-cua` manifest and Host protocol.
This keeps UI automation independent from the HTTP server layer.

Core shapes include:

- `UiControlNode` and `UiSnapshot` for bounded UI trees.
- `UiFindRequest` for locating controls by query, role, label, or object name.
- `UiActionRequest` for one bounded action such as click, set text, toggle,
  set checked, select option, or focus.
- `UiWaitCondition` and `UiWaitResult` for in-tool polling such as "wait until
  status text equals Applied" or "wait until the modal is gone".
- `UiActionResult` with structured errors such as `stale_control`, `denied`,
  `unsupported_action`, and optional screenshot/artefact refs.
- `UiControlPolicy` and `UiControlAuditRecord` for scoped action controls and
  privacy-preserving audit output.

Adapters must return structured errors instead of hanging when controls go
stale, and adapter-side safety policy still decides which actions are allowed.

Preferred agent loop:

1. `ui_control__snapshot` observes a scoped application window and returns a
   `snapshot_id`.
2. `ui_control__find` resolves a stable control id by query, role, label, or object
   name.
3. `ui_control__act` performs one action against that control id. Pass the
   `snapshot_id` when available so stale controls fail with `stale_control`
   instead of acting on the wrong target.
4. `ui_control__wait_for` polls inside one call until the expected UI state is true
   or returns `timeout` with structured details.
5. `ui_control__snapshot` verifies the final state.

Native application menu bars may use
`ui_control__act(action="invoke_menu", menu_path=[...])` without a prior
snapshot when the Host advertises `native_menu_path`. The path is resolved
inside the exact bound window and fails closed on missing or ambiguous levels.
The operation invalidates current observations; honor `verification_required`
and verify the popup or resulting application state with a fresh snapshot.

Use native DCC skills or APIs first. Use `ui_control__*` only when the behavior is
visible in the application UI but not exposed through a reliable host API.
Workflow examples and recovery patterns live in
[ui-control-workflows.md](ui-control-workflows.md).

Safety expectations:

- Snapshot/find tools are read-only and may run on any thread when the backend
  supports it.
- Mutating actions should declare conservative safety annotations, main-thread
  affinity when required by the host, and a timeout that reflects UI polling.
- Declare MCP `annotations`, `execution`, `affinity`, and
  `timeout_hint_secs` in `tools.yaml`. Gateway `search_tools` / `/v1/search`
  carry compact safety hints, and `describe_tool` / `/v1/describe` expose the
  full schema plus `_meta.dcc` affinity, execution, timeout, and risk hints.
- Gateway instance rows include `diagnostics.ui_control.status`: `available` when
  `ui_control__*` capabilities are indexed, `unavailable` when none are present,
  or `disabled_by_policy` when adapter registry metadata publishes
  `ui_control.status=disabled` (optionally with `ui_control.reason`).
- Policy should disable whole-desktop access by default. Scope to an
  adapter-owned process, window, or explicit allow-list. Keep
  `UiControlPolicy.require_scoped_window` enabled unless the user explicitly opts
  into a backend-specific whole-desktop fallback.
- Raw coordinate clicks and keyboard shortcuts are high risk. Keep them disabled
  unless an adapter explicitly opts in and documents the fallback.
- Audit records should include action kind, target control id/role/label when
  safe, before/after focus ids, success/failure, and a structured error code.
  Sensitive typed text and screenshot bytes should be redacted or returned only
  as artefact/resource references.

The bundled `ui-control` skill defaults to standalone `dcc-cua` 0.4.0 or newer.
The deterministic mock backend is explicit test infrastructure only. Set
`DCC_MCP_UI_CONTROL_BACKEND=chrome` to use the
experimental CDP backend and drive browser or webview search through the same
`ui_control__snapshot`, `ui_control__find`, `ui_control__act`, and `ui_control__wait_for`
tools. The CDP backend supports presets: `reuse` attaches to an existing
DevTools endpoint first so current browser tokens can be reused, `isolated`
launches a temporary Chrome profile, and `auroraview` attaches to AuroraView's
CDP endpoint using `DCC_MCP_UI_CONTROL_AURORAVIEW_CDP_PORT`,
`AURORAVIEW_CDP_PORT`, `DCC_MCP_UI_CONTROL_CDP_PORT`, or port `9222`. The same
runtime also supports `edge` for Microsoft Edge CDP and `agent-browser` for
Vercel's `agent-browser` CLI, which exposes its DevTools URL through
`agent-browser get cdp-url` and can be provisioned in CI with
`agent-browser install`.

The default `DCC_MCP_UI_CONTROL_BACKEND=cua` uses the standalone `dcc-cua`
CLI/Host for native applications on Windows, Linux, and macOS. Core probes the
official dcc-mcp install directory, versioned standalone installs, and `PATH`;
`DCC_MCP_CUA_BINARY` remains the explicit custom-layout override.
`DccServerBase` injects the resolved adapter PID/HWND/title from
`DccServerOptions` after public argument validation. Dedicated servers may use
`DCC_MCP_UI_CONTROL_PROCESS_ID` and `DCC_MCP_UI_CONTROL_WINDOW_HANDLE`
overrides. Request arguments may narrow this scope but cannot widen it, and a
title constraint is forwarded to the Host to resolve one window inside a
multi-window process before an exact capability is minted. The Host owns
platform accessibility, capture, visible control markers, input serialization,
and Escape interruption.

A request may also repeat `process_name` beside the trusted PID/HWND as
metadata or an additional constraint. That redundant field preserves the
trusted native scope when it is consistent with
`DCC_MCP_UI_CONTROL_PROCESS_NAME`; a conflict fails with `invalid_target`.
Process names and titles alone never establish the native capability boundary.

## Project-bound launch and readiness contract

Adapters that want agents to recover from a zero-instance state publish a
validated launch plan and stamp lifecycle metadata on their registry row. Core
owns the lifecycle; the adapter owns the plan, the metadata, and the readiness
values. The full adapter-side contract — plan schema, argv placeholders,
environment variables, metadata keys, readiness bits, and blocking states —
lives in [adapter-launch-plan-contract.md](adapter-launch-plan-contract.md).
