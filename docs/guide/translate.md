# translate subcommand — Bridge stdio MCP Servers to HTTP/SSE

The `translate` subcommand bridges any stdio MCP server to HTTP/SSE/Streamable-HTTP transport (issue #769).

## Use Cases

- Expose `filesystem`, `git`, `sqlite`, `brave-search`, or any other stdio-only MCP server over HTTP
- Connect Cursor, Claude Desktop, or any HTTP-first agent to a stdio MCP server
- Run multiple stdio MCP servers behind a single gateway endpoint
- Test stdio MCP servers through standard HTTP tooling

## Quick Start

```bash
dcc-mcp-server translate \
  --stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" \
  --app-type filesystem \
  --port 3333
```

## CLI Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--stdio <cmd>` | required | Shell command to launch the stdio MCP server |
| `--app-type <type>` | `external` | Application type label for gateway registration |
| `--expose-streamable-http <bool>` | `true` | Expose Streamable HTTP at `/mcp` |
| `--expose-sse <bool>` | `false` | Also expose legacy SSE at `/sse` |
| `--port <N>` | `0` (OS-assigned) | HTTP listen port |
| `--host <addr>` | `127.0.0.1` | Listen address |
| `--no-register` | `false` | Skip FileRegistry / gateway registration |
| `--restart-on-exit <bool>` | `true` | Restart the stdio process if it exits; pass `false` to disable supervisor mode |
| `--max-restarts <N>` | `10` | Max supervisor restart attempts before giving up; `0` = unlimited |
| `--gateway-port <N>` | `9765` | Gateway port for registration competition; `0` disables gateway/admin |
| `--no-admin` | `false` | Disable the Admin UI on the elected gateway |
| `--admin-path <path>` | `/admin` | Admin UI URL prefix |
| `--stale-timeout-secs <N>` | `30` | Gateway election stale timeout |
| `--registry-dir <path>` | auto | Custom registry directory |

## Examples

### Filesystem MCP server

```bash
dcc-mcp-server translate \
  --stdio "npx -y @modelcontextprotocol/server-filesystem /home/user/projects" \
  --app-type filesystem \
  --port 4000
```

### Git MCP server with supervisor restart

```bash
dcc-mcp-server translate \
  --stdio "uvx mcp-server-git --repository /path/to/repo" \
  --app-type git \
  --port 4001 \
  --max-restarts 10
```

### Standalone (no gateway registration)

```bash
dcc-mcp-server translate \
  --stdio "python -m my_mcp_server" \
  --no-register \
  --port 4002
```

## Cursor / Claude Desktop Configuration

Point your AI client at the default Streamable HTTP endpoint:

```json
// .cursor/mcp.json or claude_desktop_config.json
{
  "mcpServers": {
    "filesystem": {
      "url": "http://localhost:4000/mcp",
      "transport": "streamable-http"
    },
    "git": {
      "url": "http://localhost:4001/mcp",
      "transport": "streamable-http"
    }
  }
}
```

For legacy SSE clients, start the bridge with `--expose-sse true` and point them at `/sse`:

```json
{
  "mcpServers": {
    "filesystem": {
      "url": "http://localhost:4000/sse"
    }
  }
}
```

## Implementation Notes

- **Async actor model**: one Tokio task owns the child process stdin/stdout with mpsc channel communication
- **Concurrent requests**: request/response ID tracking supports multiple in-flight calls
- **Notifications**: JSON messages without `id` field are forwarded to all connected SSE clients
- **Supervisor**: exponential back-off between restarts (cap: 30 seconds)
- **Gateway registration**: when `--no-register` is absent, the bridge registers as a DCC instance in the gateway election

## See also

- [gateway.md](gateway.md) — gateway registration and election
- [tunnel-relay.md](tunnel-relay.md) — remote relay for external/internet access
- [rest-api-surface.md](rest-api-surface.md) — per-DCC REST API surface
