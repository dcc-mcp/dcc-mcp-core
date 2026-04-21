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
