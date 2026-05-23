# Adapter Runtime Contracts

Core exposes small, DCC-agnostic contracts for runtime material that agents
need while tools and jobs are running. Adapters keep host-specific collection
and safety policy; core standardizes the shapes and resource hand-off paths.

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

## App UI automation contract

The `app_ui` contract is a schema and workflow, not a universal click bot.
Adapters may implement it with Qt, native accessibility APIs, webviews, or
DCC-specific UI APIs. The public tool names use `app_ui__*` because the
capability is intentionally broader than a DCC-only UI namespace: the same
contract can describe a DCC preferences dialog, an external launcher, a license
utility, or another adapter-owned application window.

The Rust schema lives in the `dcc-mcp-app-ui` crate so UI automation contracts
can evolve independently from the HTTP server layer. Python adapters continue
to import the matching dataclasses from `dcc_mcp_core.adapter_contracts`.

Core shapes include:

- `UiControlNode` and `UiSnapshot` for bounded UI trees.
- `UiFindRequest` for locating controls by query, role, label, or object name.
- `UiActionRequest` for one bounded action such as click, set text, toggle,
  set checked, select option, or focus.
- `UiWaitCondition` and `UiWaitResult` for in-tool polling such as "wait until
  status text equals Applied" or "wait until the modal is gone".
- `UiActionResult` with structured errors such as `stale_control`, `denied`,
  `unsupported_action`, and optional screenshot/artefact refs.
- `AppUiPolicy` and `AppUiAuditRecord` for scoped action controls and
  privacy-preserving audit output.

Adapters must return structured errors instead of hanging when controls go
stale, and adapter-side safety policy still decides which actions are allowed.

Preferred agent loop:

1. `app_ui__snapshot` observes a scoped application window and returns a
   `snapshot_id`.
2. `app_ui__find` resolves a stable control id by query, role, label, or object
   name.
3. `app_ui__act` performs one action against that control id. Pass the
   `snapshot_id` when available so stale controls fail with `stale_control`
   instead of acting on the wrong target.
4. `app_ui__wait_for` polls inside one call until the expected UI state is true
   or returns `timeout` with structured details.
5. `app_ui__snapshot` verifies the final state.

Use native DCC skills or APIs first. Use `app_ui__*` only when the behavior is
visible in the application UI but not exposed through a reliable host API.

Safety expectations:

- Snapshot/find tools are read-only and may run on any thread when the backend
  supports it.
- Mutating actions should declare conservative safety annotations, main-thread
  affinity when required by the host, and a timeout that reflects UI polling.
- Policy should disable whole-desktop access by default. Scope to an
  adapter-owned process, window, or explicit allow-list. Keep
  `AppUiPolicy.require_scoped_window` enabled unless the user explicitly opts
  into a backend-specific whole-desktop fallback.
- Raw coordinate clicks and keyboard shortcuts are high risk. Keep them disabled
  unless an adapter explicitly opts in and documents the fallback.
- Audit records should include action kind, target control id/role/label when
  safe, before/after focus ids, success/failure, and a structured error code.
  Sensitive typed text and screenshot bytes should be redacted or returned only
  as artefact/resource references.

The bundled `app-ui` skill defaults to a deterministic mock backend for tests
and adapter authoring. Set `DCC_MCP_APP_UI_BACKEND=chrome` to use the
experimental CDP backend and drive browser or webview search through the same
`app_ui__snapshot`, `app_ui__find`, `app_ui__act`, and `app_ui__wait_for`
tools. The CDP backend supports presets: `reuse` attaches to an existing
DevTools endpoint first so current browser tokens can be reused, `isolated`
launches a temporary Chrome profile, and `auroraview` attaches to AuroraView's
CDP endpoint using `DCC_MCP_APP_UI_AURORAVIEW_CDP_PORT`,
`AURORAVIEW_CDP_PORT`, `DCC_MCP_APP_UI_CDP_PORT`, or port `9222`. The same
runtime also supports `edge` for Microsoft Edge CDP and `agent-browser` for
Vercel's `agent-browser` CLI, which exposes its DevTools URL through
`agent-browser get cdp-url` and can be provisioned in CI with
`agent-browser install`.
