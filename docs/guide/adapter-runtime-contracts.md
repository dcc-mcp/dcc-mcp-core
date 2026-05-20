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

## UI automation contract

The UI contract is a schema, not a universal click bot. Adapters may implement
it with Qt, native accessibility APIs, webviews, or DCC-specific UI APIs.

Core shapes include:

- `UiControlNode` and `UiSnapshot` for bounded UI trees.
- `UiFindRequest` for locating controls by query, role, label, or object name.
- `UiActionRequest` for one bounded action such as click, set text, toggle,
  set checked, select option, or focus.
- `UiActionResult` with structured errors such as `stale_control`, `denied`,
  `unsupported_action`, and optional screenshot/artefact refs.

Adapters must return structured errors instead of hanging when controls go
stale, and adapter-side safety policy still decides which actions are allowed.
