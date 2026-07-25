# App UI Workflows

This document surveys the application-level UI surfaces in dcc-mcp-core: the
built-in Admin Dashboard, the `ui_control` automation tool suite, and CLI
subcommands that interact with UI features. Each section links to the detailed
reference for deeper reading.

## Admin Dashboard

The gateway ships an embedded `/admin` web dashboard (React/Vite source in
`admin-ui/`). At runtime it is a single HTML payload served from the binary;
contributors edit the source under `admin-ui/`, and the build system embeds the
built asset during Cargo compilation.

The dashboard is enabled by default on the elected gateway. It provides:

- **Instances** — connected DCC adapter overview with version diagnostics
- **Tools** — registered tool/action catalogue with search and describe
- **Calls** — live and historical tool call log
- **Traces** — distributed trace viewer
- **Stats** — gateway-level metrics and throughput
- **Workers** — background worker pool status
- **Merged Logs** — aggregated log viewer across all gateway log files
- **Health** — readiness and liveness probes

See [admin-ui.md](admin-ui.md) for activation flags, environment variables,
Python/Rust API configuration, and the full feature walkthrough.

### Enabling and Disabling

```bash
# Default: gateway election on :9765; elected process serves /admin
dcc-mcp-server --app maya

# Disable gateway entirely (also disables admin)
dcc-mcp-server --gateway-port 0

# Keep gateway but disable admin
dcc-mcp-server --no-admin

# Move admin under another prefix
dcc-mcp-server --admin-path /dcc-admin
```

Equivalent environment variables:

| Env var | Default | Description |
|---------|---------|-------------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | Gateway election port. `0` disables gateway/admin. |
| `DCC_MCP_NO_ADMIN` | `false` | Disable the Admin UI on the elected gateway. |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL prefix. |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | unset | Optional JSONL directory for durable audit and trace persistence. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | Max JSONL rows retained per durable file. |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | Approx. 50 MiB byte cap per durable JSONL file. |
| `DCC_MCP_LOG_DIR` | platform log dir | Directory scanned by `/admin/api/logs` for `*.log` files. |

### Analytics Dashboard

A separate analytics dashboard provides KPI timeseries, heatmaps, top-tool
rankings, and CSV/JSONL export. See [analytics-dashboard.md](analytics-dashboard.md).

## UI Control Automation

When a DCC exposes state only through a window, modal dialog, webview, launcher,
or settings panel — rather than a typed API — the `ui_control` tool suite
provides a scoped fallback for interface-only automation.

The canonical workflow follows this loop:

1. **`ui_control__snapshot`** — capture the scoped DCC window, returns `snapshot_id`
2. **`ui_control__find`** — resolve a control by label, text, role, or name
3. **`ui_control__act`** — perform one action (click, set_text, drag, etc.)
4. **`ui_control__wait_for`** — poll inside one tool call until the UI reaches expected state
5. **`ui_control__snapshot`** — verify the final state
6. **`ui_control__stop_computer_use`** — release native input, remove visual effects

Always treat step 6 like a `finally` block — call it on success, failure,
cancellation, or abandonment.

See [ui-control-workflows.md](ui-control-workflows.md) for the full reference:
decision rules, evidence attribution, system configuration operations, recovery
patterns, and verification requirements.

### CLI Interface

The `dcc-mcp-cli ui-control` subcommand exposes the same capabilities through
the shell:

```bash
dcc-mcp-cli ui-control snapshot --instance-id <id> --json '{"session_id":"ui","process_id":1234}'
dcc-mcp-cli ui-control act --instance-id <id> --json '{"session_id":"ui","control_id":"ok","action":"click","snapshot_id":"<snapshot_id>"}'
dcc-mcp-cli ui-control record-clip --instance-id <id> --json '{"session_id":"pv","process_id":1234,"duration_ms":5000}'
dcc-mcp-cli ui-control stop --instance-id <id> --json '{"session_id":"ui"}'
dcc-mcp-cli ui-control system-operation --instance-id <id> --json '{"operation_id":"enable-remote-control"}'
```

## Related Documentation

| Document | Purpose |
|----------|---------|
| [admin-ui.md](admin-ui.md) | Full Admin Dashboard reference: activation, panels, audit, health |
| [analytics-dashboard.md](analytics-dashboard.md) | KPI dashboard, timeseries, heatmap, CSV/JSONL export |
| [ui-control-workflows.md](ui-control-workflows.md) | UI Control automation: full workflow reference, recovery, verification |
| [gateway.md](gateway.md) | Multi-DCC gateway: aggregation, tool routing, election |
| [gateway-diagnostics.md](gateway-diagnostics.md) | Gateway health, readiness, contention, failure diagnosis |
| [cli-reference.md](cli-reference.md) | Canonical CLI commands, flags, profiles |
| [observability-usage.md](observability-usage.md) | Agent, CLI, and Admin UI observability usage |
