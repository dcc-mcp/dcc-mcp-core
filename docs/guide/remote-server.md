# Remote-First MCP Server Design Guide

> **[中文版](../zh/guide/remote-server)**

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
| `allowed_origins` | `["*"]` | Restrict to specific client origins in production |
| `spawn_mode` | `"dedicated"` | Always use `"dedicated"` for PyO3-embedded hosts |
| `api_key` | env var | Optional Bearer token auth (see [Auth](#auth)) |
| `enable_oauth` | `False` | OAuth 2.1 + CIMD auth (see [OAuth](#oauth-cimd)) |

```python
cfg = McpHttpConfig(
    host="0.0.0.0",
    port=8765,
    enable_cors=True,
    allowed_origins=["https://claude.ai", "https://cursor.sh"],
    spawn_mode="dedicated",    # always for DCC-embedded hosts
)
```

---

## Auth

### API Key (simplest)

For studio environments where OAuth is impractical:

```python
import os
cfg = McpHttpConfig(port=8765, host="0.0.0.0")
cfg.api_key = os.environ.get("DCC_MCP_API_KEY")  # Bearer token
```

Clients include `Authorization: Bearer <key>` in every request.

### OAuth 2.1 + CIMD (recommended for production)

See [CIMD OAuth guide](remote-server.md#oauth-cimd) below and issue #408.
Enable with `McpHttpConfig(enable_oauth=True)`.

---

## CORS Configuration

CORS headers are required whenever the MCP client runs in a browser
(Claude.ai, any web-based agent UI) or in Cursor / VS Code.

```python
cfg = McpHttpConfig(enable_cors=True)

# Production: restrict to known origins
cfg.allowed_origins = [
    "https://claude.ai",
    "https://cursor.sh",
    "https://vscode.dev",
]
```

When `enable_cors=True` and `allowed_origins` is empty (default), the server
sends `Access-Control-Allow-Origin: *` — convenient for development but
**not recommended for production**.

---

## Container / VPS Deployment

The minimal Docker setup for a public MCP server:

```dockerfile
FROM python:3.11-slim
RUN pip install dcc-mcp-core
COPY skills/ /opt/skills/
ENV DCC_MCP_SKILL_PATHS=/opt/skills
ENV DCC_MCP_API_KEY=change-me
EXPOSE 8765
CMD ["python", "-c", "
from dcc_mcp_core import create_skill_server, McpHttpConfig
import os, time
cfg = McpHttpConfig(host='0.0.0.0', port=8765, enable_cors=True)
cfg.api_key = os.environ.get('DCC_MCP_API_KEY')
server = create_skill_server('generic', cfg)
handle = server.start()
print(handle.mcp_url())
while True: time.sleep(60)
"]
```

Build and run:

```bash
docker build -t my-mcp-server .
docker run -p 8765:8765 -e DCC_MCP_API_KEY=secret my-mcp-server
```

---

## Example: Minimal Remote-Accessible Skill Server

See [`examples/remote-server/`](../../examples/remote-server/) for a
complete, deployable example that:

- Starts a publicly reachable MCP server on `0.0.0.0:8765`
- Enables CORS and API-key auth from environment variables
- Includes a minimal `hello-world` skill
- Ships a `Dockerfile` and `docker-compose.yml`

---

## Remote-First Checklist

Use this checklist when deploying a DCC adapter for remote access:

- [ ] Server is bound to `0.0.0.0` (not just `127.0.0.1`)
- [ ] Auth is configured: API key (`cfg.api_key`) or OAuth (`cfg.enable_oauth = True`)
- [ ] CORS is enabled (`cfg.enable_cors = True`) with restricted `allowed_origins` in production
- [ ] Tool descriptions follow the 3-layer behavioral structure (issue #341)
- [ ] Tools are grouped by user intent, not 1:1 with API endpoints
- [ ] `McpHttpConfig.spawn_mode = "dedicated"` for DCC-embedded hosts (Maya, Blender…)
- [ ] Port 8765 is open in firewall / security group
- [ ] TLS is terminated at a reverse proxy (nginx, Caddy, AWS ALB) for internet-facing deployments
- [ ] `DCC_MCP_API_KEY` is set as an environment variable — never hardcoded
- [ ] File logging is enabled (`enable_file_logging=True`, the default) for audit trails

---

## OAuth / CIMD

> Full guide: issue #408 — CIMD OAuth support is planned for a future release.

When `McpHttpConfig.enable_oauth = True`, the server will expose:

```
GET /.well-known/oauth-client-metadata
```

returning a CIMD document that enables automatic client registration with
no manual client ID setup. This is the recommended approach for
production cloud deployments.

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
