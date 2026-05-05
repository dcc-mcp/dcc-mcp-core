# Built-in Admin Dashboard

The gateway ships a zero-build, zero-dependency `/admin` web dashboard (issue #772). No `npm`, no CDN — a single inline HTML file served from the binary.

## Activation

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,
    admin_path: "/admin".to_string(),  // default
    ..GatewayConfig::default()
};
```

Requires the `admin` Cargo feature:

```toml
[dependencies]
dcc-mcp-gateway = { features = ["admin"] }
```

Default: `admin_enabled = false` (opt-in for security).

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

The admin UI is **read-only** and has **no authentication** by default. For production:
- Place behind a reverse proxy with IP allowlist or basic auth
- Or disable when not needed: `admin_enabled: false`
- Never expose directly to the public internet

## See also

- [middleware.md](middleware.md) — `AuditMiddleware` that feeds `/admin/api/calls`
- [observability.md](observability.md) — `EventLog` that feeds `/admin/api/logs`
- [gateway.md](gateway.md) — full gateway configuration reference
