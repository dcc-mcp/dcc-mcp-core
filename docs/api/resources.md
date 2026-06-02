# Resources API

`dcc_mcp_core` — MCP Resources primitive for live DCC state.

## Overview

The Resources primitive lets MCP clients (LLMs and hosts) read **live DCC state** over the same
HTTP endpoint used for tools. It implements the [MCP 2025-03-26](https://modelcontextprotocol.io/specification/2025-03-26)
`resources` capability:

- `resources/list` — enumerate available URIs
- `resources/read` — fetch content by URI (text or base64 blob)
- `resources/subscribe` / `resources/unsubscribe` — opt into push notifications
- `notifications/resources/updated` — server-push over SSE when a subscribed URI changes

Resources are advertised in `initialize` as:

```json
{
  "capabilities": {
    "resources": { "subscribe": true, "listChanged": true }
  }
}
```

## Built-in Resources

`dcc-mcp-core` ships four built-in producers. Each is keyed by URI scheme.

| URI | MIME | Description | Notifications |
|-----|------|-------------|---------------|
| `scene://current` | `application/json` | Current `SceneInfo` snapshot (or placeholder) | Fires when `set_scene()` is called |
| `capture://current_window` | `image/png` | PNG of the active DCC window (base64 blob) | None (read-on-demand) |
| `audit://recent?limit=N` | `application/json` | Tail of the `AuditLog` (default 50, max 500) | Fires on every `AuditLog.record()` |
| `artefact://sha256/<hex>` | varies | Content-addressed artefact store (#349); bodies returned as base64 blobs with their declared MIME. Gated by `enable_artefact_resources`. | None (polling model) |

`capture://current_window` is only listed when a real window-capture backend is available
(currently Windows `HWND PrintWindow`). On other platforms it is hidden from `resources/list`.

## Gateway-Native Resources

When an agent is connected to the multi-DCC gateway, instance discovery is also
modeled as an MCP resource instead of a tool:

| URI | MIME | Description |
|-----|------|-------------|
| `gateway://instances` | `application/json` | Live DCC registry. Rows include `instance_id`, `dcc_type`, health/status fields, metadata, and `mcp_url` for direct sessions. |
| `gateway://instances?include_stale=false` | `application/json` | Same registry with stale-but-parseable rows hidden. |
| `gateway://instances?include_dead=true` | `application/json` | Rawer registry view including rows whose owner process has exited. |
| `gateway://instances/{instance_id}` | `application/json` | One instance selected by full UUID, `instance_short`, or a unique 4+ character UUID prefix. |
| `gateway://diagnostics/process` | `application/json` | Gateway process metadata plus live/stale/unhealthy instance counts; optional `?dcc_type=<type>` filter. |
| `gateway://diagnostics/audit` | `application/json` | Pending-call and resource-subscription summary. Backend audit logs remain per-instance. |
| `gateway://diagnostics/metrics` | `application/json` | Local gateway tool count, live backend count, timeout settings, and `publishes_backend_tools=false`. |
| `gateway://catalog` | `application/json` | Public adapter/skill/plugin package index; optional `?query=<keyword>` filter. |
| `gateway://catalog/{name}` | `application/json` | One public catalog entry selected by exact name. |
| `resources://gateway/events` | `application/jsonl` | Gateway contention and election event ring buffer. |

`resources/list` advertises only root pointers for gateway-native families:
`gateway://instances`, `gateway://diagnostics/process`,
`gateway://diagnostics/audit`, `gateway://diagnostics/metrics`, and
`gateway://catalog`. It does not enumerate every `gateway://instances/{id}` or
`gateway://catalog/{name}` URI. Read a single-entry URI directly when you
already know the id/name. The legacy gateway tools `list_dcc_instances`,
`get_dcc_instance`, `connect_to_dcc`, `dcc_catalog__search`, and
`dcc_catalog__describe` were removed; instance entries already carry `mcp_url`,
so no separate connect verb is required.


## Enabling / Disabling


```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.enable_resources = True              # default: True — advertise capability + built-ins
cfg.enable_artefact_resources = False    # default: False — artefact:// returns -32002 until enabled
```

When `enable_resources = False` the server does not advertise the capability and all four
`resources/*` methods return `-32601 method not found`.

## Wiring External State

A DCC adapter typically sets a scene snapshot and forwards audit events to the registry
**before** calling `server.start()`:

```python
from dcc_mcp_core import (
    AuditLog,
    McpHttpServer,
    McpHttpConfig,
    ToolRegistry,
)

registry = ToolRegistry()
# ... registry.register(...) ...

server = McpHttpServer(registry, McpHttpConfig(port=8765))

# 1. Push scene snapshots whenever the DCC scene changes:
server.resources().set_scene({
    "scene_path": "/projects/shot_010/main.ma",
    "fps": 24,
    "frame_range": [1001, 1240],
    "active_camera": "persp",
})

# 2. Hook the sandbox AuditLog so audit://recent fires notifications on every record:
audit = AuditLog()
server.resources().wire_audit_log(audit)

handle = server.start()
```

### Session/job events

Adapters that want a generic diagnostics stream can register a
`SessionEventBuffer` instead of inventing adapter-specific log tools:

```python
from dcc_mcp_core import SessionEventBuffer

events = SessionEventBuffer("maya-001", maxlen=1000, max_message_bytes=4096)
server.resources().register_session_event_buffer(events)

events.append(
    source="python",
    stream="stdout",
    level="info",
    message="Created preview mesh",
    tool_call_id="req-42",
    job_id="job-7",
)
```

Clients read by cursor:

```json
{"method": "resources/read",
 "params": {"uri": "events://session/maya-001?cursor=0&limit=100"}}
```

The JSON payload includes `events`, `next_cursor`, retained/dropped counts,
and per-event correlation fields (`session_id`, `tool_call_id`, `job_id`,
`correlation_id`). Passing `drain=true` removes returned-and-older events.

### USD project resources

Headless USD adapters should use the canonical `openusd://` family instead of
inventing host-specific URI shapes:

| URI | MIME | Purpose |
|-----|------|---------|
| `openusd://stage` | `model/vnd.usd.usda+text` or `model/vnd.usd.usdc` | Primary stage/root layer |
| `openusd://layers` | `application/json` | Manifest of layer resources |
| `openusd://assets` | `application/json` | Manifest of external asset dependencies |
| `openusd://materials` | `application/json` | Manifest of material resources |
| `openusd://validation` | `application/json` | Manifest of validation reports |
| `openusd://snapshots` | `application/json` | Manifest of generated snapshots |
| `openusd://packages` | `application/json` | Manifest of packaged handoffs such as USDZ |

```python
from dcc_mcp_core import register_usd_project_resources

provider = register_usd_project_resources(
    server,
    project_root="/show/shot010/usd",
    stage="/show/shot010/usd/shot.usda",
    layers=["/show/shot010/usd/lighting.usda"],
    validation={"name": "usdchecker.json", "content": {"status": "ok"}},
    project_label="shot010",
)
```

Every record returned from `provider.records` carries stable
`uri`/`name`/`description`/`mimeType` data for MCP `resources/list`, plus a
manifest entry with `kind`, `project_root_label`, and a `file_ref` for
filesystem-backed resources. This convention is DCC-agnostic: OpenUSD,
Houdini Solaris, Maya USD, Blender USD, Unreal, and Omniverse-style adapters
can all publish the same project concepts.

### Updating the scene snapshot

`set_scene()` replaces the snapshot atomically and emits a
`notifications/resources/updated` with `uri = "scene://current"` to every subscribed session.

```python
server.resources().set_scene(new_snapshot_dict)
```

Pass `None` to clear the snapshot (readers then receive a placeholder with `status: "no_snapshot"`).

## Example: Client Side

```python
import json, urllib.request

# Initialize + grab session id (omitted here)
# List resources
body = {"jsonrpc": "2.0", "id": 1, "method": "resources/list"}
req = urllib.request.Request(
    "http://127.0.0.1:8765/mcp",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json", "Mcp-Session-Id": session_id},
    method="POST",
)
with urllib.request.urlopen(req) as r:
    print(json.loads(r.read())["result"]["resources"])

# Read audit log
body = {
    "jsonrpc": "2.0", "id": 2,
    "method": "resources/read",
    "params": {"uri": "audit://recent?limit=10"},
}
# ... POST, parse result.contents[0].text as JSON ...

# Subscribe to scene updates
body = {
    "jsonrpc": "2.0", "id": 3,
    "method": "resources/subscribe",
    "params": {"uri": "scene://current"},
}
# ... POST, then open GET /mcp SSE stream to receive notifications/resources/updated ...
```

## Error Codes

| Code | Meaning |
|------|---------|
| `-32601` | Method not found — resources disabled (`enable_resources = False`) |
| `-32602` | Invalid params — missing or malformed `uri` |
| `-32002` | Resource not enabled (scheme recognised, backend disabled) — also reused when an `artefact://` URI is syntactically valid but not stored |
| `-32603` | Internal error — producer failed (capture backend error, etc.) |

## `artefact://` Scheme (issue #349)

Content-addressed artefact hand-off between tools and workflow steps.
See [`docs/guide/artefacts.md`](../guide/artefacts.md) for the full
`FileRef` + `ArtefactStore` guide. Quick reference:

- URI shape: `artefact://sha256/<hex>`.
- Default backend: `FilesystemArtefactStore` anchored at
  `<registry_dir>/dcc-mcp-artefacts` (or the OS temp dir).
- `resources/list` enumerates every stored artefact; entries carry the
  declared MIME and sidecar metadata.
- `resources/read` returns the raw bytes as a base64 blob.
- Python helpers: `artefact_put_file`, `artefact_put_bytes`,
  `artefact_get_bytes`, `artefact_list`.
- Enable with `McpHttpConfig.enable_artefact_resources = True`.
- `FileRef` sidecars can carry display names, session/tool/job/correlation
  fields, `expires_at`, and adapter-defined JSON metadata.

## Writing a Custom Producer (Rust)

```rust
use dcc_mcp_http::{ProducerContent, ResourceError, ResourceProducer, ResourceResult};
use async_trait::async_trait;

struct PlaybackProducer;

#[async_trait]
impl ResourceProducer for PlaybackProducer {
    fn scheme(&self) -> &str { "playback" }

    async fn list(&self) -> ResourceResult<Vec<McpResource>> {
        Ok(vec![McpResource {
            uri: "playback://current".into(),
            name: "Playback state".into(),
            description: Some("Current playback frame and range".into()),
            mime_type: Some("application/json".into()),
        }])
    }

    async fn read(&self, uri: &str) -> ResourceResult<Vec<ProducerContent>> {
        if uri != "playback://current" {
            return Err(ResourceError::NotFound(uri.into()));
        }
        Ok(vec![ProducerContent::Text {
            uri: uri.into(),
            mime_type: Some("application/json".into()),
            text: r#"{"frame": 42, "range": [1, 100]}"#.into(),
        }])
    }
}
```

Then register it on the server before `start()`:

```rust
server.resources().add_producer(Arc::new(PlaybackProducer));
```

## Lifecycle Guarantees

- Subscriptions are **per-session**. When a client terminates the session (`DELETE /mcp`)
  all its subscriptions are dropped automatically.
- `notifications/resources/updated` is best-effort — if the SSE channel is full or the client
  has disconnected, the notification is discarded (no queueing, no backpressure on the producer).
- Producers must be cheap to call — `resources/list` is invoked on every MCP client reconnect.
- Blob content (`capture://current_window`) is base64-encoded inside the JSON-RPC response;
  keep payloads under ~5 MB to avoid client-side decoding issues.

## See Also

- [HTTP API](./http.md) — `McpHttpServer`, `McpHttpConfig`
- [Sandbox API](./sandbox.md) — `AuditLog` (source of `audit://recent`)
- [Capture API](./capture.md) — `Capturer` (source of `capture://current_window`)
