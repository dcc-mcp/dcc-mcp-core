# Remote-First MCP Server Design Guide

> **[中文版](../zh/guide/remote-server.md)**

This guide explains when to choose a remote MCP server over a local socket,
how to deploy `create_skill_server()` so it is reachable from cloud-hosted
agents (Claude.ai, Cursor, ChatGPT, VS Code), and what CORS / auth
configuration is required.

---

## Why Remote?

A remote MCP server is the only configuration that works across **web,
mobile, and cloud-hosted agents** simultaneously.

| Path | Best for | Limits |
|------|----------|--------|
| Direct API calls | Small, non-reused integrations | M×N integration problem at scale |
| CLI / local socket | Developer workstations, embedded plugins | Unreachable from mobile/web/cloud without a shell |
| **Remote MCP server** | Cloud-hosted agents, maximum reach | Small one-time setup investment |

Once deployed, a remote server becomes a **compounding layer**: every new
MCP spec capability (Elicitation, MCP Apps, OAuth, Tool Search…) lands
automatically in every compatible client without changes to the server's
tool implementations.

---

## Quick Start: Minimal Remote Server

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

cfg = McpHttpConfig(
    port=8765,
    host="0.0.0.0",           # bind to all interfaces — required for remote access
    server_name="maya-mcp",
    enable_cors=True,          # required for web / browser clients
)

server = create_skill_server("maya", cfg)
handle = server.start()
print(handle.mcp_url())        # "http://0.0.0.0:8765/mcp"
```

> **Tip**: `host="0.0.0.0"` is required for any non-localhost client.
> Keep it behind a firewall or use auth (see below) when exposed to the internet.

---

## McpHttpConfig Options for Remote Deployment

| Property | Remote default | Notes |
|----------|---------------|-------|
| `host` | `"0.0.0.0"` | Bind to all interfaces (default `"127.0.0.1"` = localhost only) |
| `port` | `8765` | Must be accessible through firewall / NAT |
| `enable_cors` | `True` | Required for browser and Claude.ai web clients |
| `spawn_mode` | `"dedicated"` | Always use `"dedicated"` for PyO3-embedded hosts |
| Edge auth | reverse proxy | Terminate TLS and enforce auth before traffic reaches `/mcp` |
| OAuth | external gateway | Use a standards-compliant OAuth proxy until native MCP OAuth lands |

```python
cfg = McpHttpConfig(
    host="0.0.0.0",
    port=8765,
    enable_cors=True,
    spawn_mode="dedicated",    # always for DCC-embedded hosts
)
```

---

## Auth

Native `McpHttpServer` / gateway auth enforcement is not implemented yet.
The Python `ApiKeyConfig` and `OAuthConfig` helpers are declarative helpers for
tool authors and future server wiring; setting `cfg.api_key` or
`cfg.enable_oauth` is not a supported runtime security boundary today.

For internet-facing deployments, put the MCP endpoint behind a reverse proxy
or a dedicated OAuth gateway and keep the DCC process itself bound to
localhost. Avoid HTTP Basic Auth on `/mcp`: browser-based clients often render
that as a generic "login required" prompt, and it is easy to confuse with MCP
OAuth. If Basic Auth is needed for observability, scope it to `/metrics` only.

### Bearer Token at the Edge

For studio environments where OAuth is impractical:

```nginx
map $http_authorization $mcp_authorized {
    default 0;
    "Bearer change-me" 1;
}

server {
    listen 443 ssl;
    server_name mcp.example.com;

    location /mcp {
        # Prefer njs/lua/auth_request for production; this compact example
        # shows the contract only.
        if ($mcp_authorized = 0) { return 401; }
        proxy_pass http://127.0.0.1:8765;
        proxy_http_version 1.1;
        proxy_buffering off;
        proxy_cache off;
    }

    # Keep metrics separate so Basic auth challenges never apply to /mcp.
    location /metrics {
        auth_basic "dcc-mcp metrics";
        auth_basic_user_file /etc/nginx/.htpasswd;
        proxy_pass http://127.0.0.1:8765;
    }
}
```

Clients include `Authorization: Bearer <key>` in every request.

### OAuth 2.1

Use an external MCP-aware OAuth proxy/gateway for production OAuth today. Native
OAuth protected-resource metadata, `WWW-Authenticate: Bearer
resource_metadata=...`, and token validation for `/mcp` are tracked as future
work in issue #408.

---

## CORS Configuration

CORS headers are required whenever the MCP client runs in a browser
(Claude.ai, any web-based agent UI) or in Cursor / VS Code.

```python
cfg = McpHttpConfig(enable_cors=True)

# Production: restrict origins at the reverse proxy until native allow-list
# configuration is available on McpHttpConfig.
```

When `enable_cors=True`, the server sends permissive CORS headers for browser
clients. Restrict allowed origins in the reverse proxy for production
deployments.

---

## Container / VPS Deployment

The minimal Docker setup for a public MCP server:

```dockerfile
FROM python:3.14-slim
RUN pip install dcc-mcp-core
COPY skills/ /opt/skills/
ENV DCC_MCP_SKILL_PATHS=/opt/skills
EXPOSE 8765
CMD ["python", "-c", "
from dcc_mcp_core import create_skill_server, McpHttpConfig
import os, time
cfg = McpHttpConfig(host='0.0.0.0', port=8765, enable_cors=True)
server = create_skill_server('generic', cfg)
handle = server.start()
print(handle.mcp_url())
while True: time.sleep(60)
"]
```

Build and run:

```bash
docker build -t my-mcp-server .
docker run -p 127.0.0.1:8765:8765 my-mcp-server
```

---

## Example: Minimal Remote-Accessible Skill Server

See [`examples/remote-server/`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/examples/remote-server) for a
complete, deployable example that:

- Starts a publicly reachable MCP server on `0.0.0.0:8765`
- Enables CORS and is intended to sit behind an edge auth proxy
- Includes a minimal `hello-world` skill
- Ships a `Dockerfile` and `docker-compose.yml`

---

## Remote-First Checklist

Use this checklist when deploying a DCC adapter for remote access:

- [ ] Server is bound to `0.0.0.0` (not just `127.0.0.1`)
- [ ] Auth is configured at the edge: Bearer-token proxy or OAuth gateway
- [ ] CORS is enabled (`cfg.enable_cors = True`), with origins restricted at the reverse proxy in production
- [ ] Tool descriptions follow the 3-layer behavioral structure (issue #341)
- [ ] Tools are grouped by user intent, not 1:1 with API endpoints
- [ ] `McpHttpConfig.spawn_mode = "dedicated"` for DCC-embedded hosts (Maya, Blender…)
- [ ] Port 8765 is open in firewall / security group
- [ ] TLS is terminated at a reverse proxy (nginx, Caddy, AWS ALB) for internet-facing deployments
- [ ] Secrets live in the reverse proxy / secret manager — never hardcoded
- [ ] File logging is enabled (`enable_file_logging=True`, the default) for audit trails

---

## OAuth / CIMD

> Full guide: issue #408 — native MCP OAuth support is planned for a future
> release.

Native support will expose:

```
GET /.well-known/oauth-protected-resource
GET /.well-known/oauth-client-metadata
```

and validate `Authorization: Bearer <token>` on `/mcp`. Until then, deploy a
standards-compliant OAuth proxy in front of dcc-mcp-core when cloud clients
require MCP OAuth.

Token injection via Claude Managed Agents Vaults: register OAuth tokens
once in a Vault; the platform injects and refreshes credentials
automatically for each MCP connection.

---

## TLS / HTTPS

`McpHttpServer` binds plain HTTP. Terminate TLS at a reverse proxy:

```nginx
server {
    listen 443 ssl;
    server_name mcp.example.com;

    ssl_certificate     /etc/letsencrypt/live/mcp.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/mcp.example.com/privkey.pem;

    location /mcp {
        proxy_pass http://127.0.0.1:8765;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        # SSE requires buffering disabled
        proxy_buffering off;
        proxy_cache off;
    }
}
```

---

## Connecting MCP Clients

Once deployed, add the server URL to your MCP client:

**Claude Desktop** (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "maya-mcp": {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer YOUR_API_KEY" }
    }
  }
}
```

**Cursor** (`~/.cursor/mcp.json`):
```json
{
  "mcpServers": {
    "maya-mcp": {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer YOUR_API_KEY" }
    }
  }
}
```

---

## See Also

- [Production Deployment](production-deployment.md) — Docker, systemd, k8s HA topologies
- [Gateway Election](gateway-election.md) — multi-instance gateway failover
- [Getting Started](getting-started.md) — local development setup
- [`docs/api/http.md`](../api/http.md) — full `McpHttpConfig` reference
