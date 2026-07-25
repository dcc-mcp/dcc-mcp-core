# Verify — Introspection

How to query live DCC state for verification purposes.

## Discovery surfaces

| Surface | Command | What it tells you |
|---------|---------|-------------------|
| CLI list | `dcc-mcp-cli list` | All registered instances, dcc_type, status |
| CLI doctor | `dcc-mcp-cli doctor` | Profile, registry dir, readiness counts, daemon |
| CLI health | `dcc-mcp-cli health` | Gateway HTTP health, version |
| Instance status | `dcc-mcp-cli list --output json` → `direct_control` | dispatch_status, ready, retryable |
| Search | `dcc-mcp-cli search --query "X"` | Available tools matching query |
| Describe | `dcc-mcp-cli describe <slug>` | Tool schema, annotations, safety hints |

## Interpreting dispatch_status

| `dispatch_status` | `ready` | Meaning |
|-------------------|---------|---------|
| `Ready` | true | Instance is callable |
| `Pending` | false | Booting or loading; retryable |
| `Failed` | false | Dispatch failed; not retryable |
| `Unknown` | varies | Status not yet reported; see `recommended_next_action` |

## Interpreting ServiceStatus

| Status | Meaning |
|--------|---------|
| `Available` | Transport healthy (TCP open, MCP responding) |
| `Busy` | Instance is working; retry after current job |
| `Booting` | Still initializing; wait and retry |
| `Unreachable` | Cannot connect; check logs/restart |
| `ShuttingDown` | Graceful shutdown in progress |
| `Stale` | No heartbeat; registry will remove |

## Querying the gateway directly

```bash
# Gateway version and health
dcc-mcp-cli health

# Gateway profiles
dcc-mcp-cli gateway list

# Gateway-level instance list (compact)
dcc-mcp-cli list --gateway <name>
```

## Querying the DCC instance directly

```python
# Via introspection tools (if available)
dcc_introspect__eval  # Run arbitrary Python expression
dcc_introspect__search  # Search available tools
dcc_introspect__signature  # Get tool signature details
```

## Python introspection helpers

When connected to a DCC instance, these Python snippets reveal live state:

```python
# List all registered tools
import dcc_mcp_core
# Access tool registry via the server instance

# Check loaded modules
import sys
sorted(m for m in sys.modules if 'dcc' in m.lower())

# Check env vars
import os
{k: v for k, v in os.environ.items() if 'DCC_MCP' in k}
```
