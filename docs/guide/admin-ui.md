# Built-in Admin Dashboard

The gateway ships a zero-build, zero-dependency `/admin` web dashboard (issue #772). No `npm`, no CDN — a single inline HTML file served from the binary.

## Activation and Defaults

`/admin` is enabled by default on the elected gateway. This is intentional: the gateway and admin dashboard are part of the default local observability surface.

### `dcc-mcp-server` / `server.exe`

```bash
# Default: joins gateway election on :9765; elected process serves /admin
dcc-mcp-server --dcc maya

# Disable gateway entirely (also disables admin)
dcc-mcp-server --gateway-port 0

# Keep gateway but disable admin
dcc-mcp-server --no-admin

# Move admin under another prefix
dcc-mcp-server --admin-path /dcc-admin
```

Equivalent env vars:

| Env var | Default | Description |
|---------|---------|-------------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | Gateway election port. `0` disables gateway/admin. |
| `DCC_MCP_NO_ADMIN` | `false` | Disable the read-only Admin UI on the elected gateway. |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL prefix. |

### Python API

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=0, server_name="maya-mcp")
# Defaults for Python embedders:
# cfg.gateway_port == 9765
# cfg.admin_enabled is True
# cfg.admin_path == "/admin"

# Disable gateway/admin for an isolated local-only server:
cfg.gateway_port = 0

# Or keep gateway but hide admin:
cfg.admin_enabled = False

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

### Rust gateway API

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,          // default
    admin_path: "/admin".into(),  // default
    ..GatewayConfig::default()
};
```

When using `dcc-mcp-gateway` directly, compile with the `admin` Cargo feature. `dcc-mcp-http` and the shipped server binary enable this for their embedded gateway path.

## Routes

| Route | Content-Type | Description |
|-------|-------------|-------------|
| `GET /admin` | `text/html` | HTML dashboard (inline CSS + vanilla JS) |
| `GET /admin/api/instances` | `application/json` | Connected DCC instances |
| `GET /admin/api/tools` | `application/json` | Registered MCP tools |
| `GET /admin/api/calls` | `application/json` | Recent tool calls (requires `AuditMiddleware`) |
| `GET /admin/api/logs` | `application/json` | Gateway contention events |
| `GET /admin/api/health` | `application/json` | Service health summary |

## API Response Shapes

```json
// GET /admin/api/health
{
  "status": "ok",
  "uptime_secs": 3600,
  "instances_total": 3,
  "instances_ready": 2
}

// GET /admin/api/instances
{
  "total": 3,
  "instances": [
    { "id": "a1b2c3d4-...", "dcc_type": "maya", "status": "ready", "address": "127.0.0.1:9001" }
  ]
}

// GET /admin/api/calls  (requires AuditMiddleware)
{
  "total": 42,
  "calls": [
    { "tool": "maya__open_scene", "success": true, "timestamp": "2026-05-05T10:00:00Z" }
  ]
}

// GET /admin/api/logs
{
  "total": 5,
  "logs": [
    { "event": "election_won", "dcc_type": "maya", "timestamp": "2026-05-05T09:59:00Z" }
  ]
}
```

## Connecting AuditMiddleware

For the `/admin/api/calls` feed to be populated, add `AuditMiddleware` to the middleware chain:

```rust
use dcc_mcp_gateway::gateway::middleware::{AuditMiddleware, MiddlewareChain};

GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default())),
    ..GatewayConfig::default()
}
```

The `/admin/api/logs` feed is populated automatically from the `EventLog` ring buffer (gateway election/eviction/probe events from issue #766).

## Dashboard Features

The HTML dashboard includes:
- **Left navigation**: Instances / Tools / Calls / Logs / Health panels
- **Auto-refresh**: Each panel polls its JSON endpoint every 5 seconds
- **Dark theme**: Minimal inline CSS, no external fonts
- **Responsive**: CSS grid layout

## Security Note

The admin UI is **read-only** and has **no authentication** by default. It binds to the same host as the elected gateway, which defaults to `127.0.0.1`. For production:
- Keep it bound to localhost, or place behind a reverse proxy with IP allowlist/basic auth
- Disable when not needed: `--no-admin`, `DCC_MCP_NO_ADMIN=true`, or `cfg.admin_enabled = False`
- Never expose directly to the public internet

## See also

- [middleware.md](middleware.md) — `AuditMiddleware` that feeds `/admin/api/calls`
- [observability.md](observability.md) — `EventLog` that feeds `/admin/api/logs`
- [gateway.md](gateway.md) — full gateway configuration reference
