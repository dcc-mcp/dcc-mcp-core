# Remote MCP Server Example

Minimal example of a publicly reachable MCP server using `dcc-mcp-core`.

## Quick Start

```bash
# Install dependency
pip install dcc-mcp-core

# Run with optional API key
DCC_MCP_API_KEY=secret python server.py
```

The server binds to `0.0.0.0:8765` and prints its MCP URL.

## Docker

```bash
docker build -t remote-mcp .
docker run -p 8765:8765 -e DCC_MCP_API_KEY=secret remote-mcp
```

## Docker Compose

```bash
DCC_MCP_API_KEY=secret docker-compose up
```

## Connect from MCP Clients

**Claude Desktop** (`claude_desktop_config.json`):
```json
{
  "mcpServers": {
    "remote-mcp": {
      "url": "http://localhost:8765/mcp",
      "headers": { "Authorization": "Bearer secret" }
    }
  }
}
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `DCC_MCP_HOST` | `0.0.0.0` | Bind address |
| `DCC_MCP_PORT` | `8765` | TCP port |
| `DCC_MCP_API_KEY` | _(none)_ | Bearer token auth (dev mode if unset) |
| `DCC_MCP_SKILL_PATHS` | _(none)_ | Extra skill directories |

## See Also

- [Remote-First MCP Server Design Guide](../../docs/guide/remote-server.md)
- [Production Deployment](../../docs/guide/production-deployment.md)
