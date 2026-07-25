# Debug — Introspection

How to query live DCC state for debugging purposes.

## Diagnostic tools reference

| Tool | What it reveals | When to use |
|------|----------------|-------------|
| `dcc_diagnostics__error_report` | Log errors, failed jobs, env snapshot | First call after any failure |
| `dcc_diagnostics__audit_log` | Sandbox allow/deny history | Permission errors, blocked calls |
| `dcc_diagnostics__tool_metrics` | Invocation counts, latency, failure rates | Slow or flaky tools |
| `dcc_diagnostics__screenshot` | Visual state capture | Visual confirmation, UI bugs |
| `dcc_diagnostics__process_status` | PID liveness | Tool hangs, process crashes |
| `dcc_introspect__eval` | Arbitrary Python execution | Deep state inspection |
| `dcc_introspect__search` | Tool catalog search | "What tools exist?" |
| `dcc_introspect__signature` | Tool parameter details | "What args does this tool take?" |

## Log file locations

| Platform | Default log dir |
|----------|----------------|
| Windows | `%TEMP%\dcc-mcp-logs\` |
| macOS | `$TMPDIR/dcc-mcp-logs/` |
| Linux | `/tmp/dcc-mcp-logs/` |

Log file naming: `dcc-mcp-<dcc_name>.<pid>.log`

## Querying the job database

The SQLite job-persistence DB lives alongside log files:
`<log_dir>/dcc-mcp-jobs.db`

```python
# Direct DB query (only when introspection tools unavailable)
import sqlite3
conn = sqlite3.connect("/tmp/dcc-mcp-logs/dcc-mcp-jobs.db")
cursor = conn.execute(
    "SELECT job_id, action, status, created_at, error_message "
    "FROM jobs WHERE status IN ('failed', 'interrupted') "
    "ORDER BY created_at DESC LIMIT 20"
)
for row in cursor:
    print(row)
conn.close()
```

## Python introspection inside a DCC

When the `dcc_introspect__eval` tool is available, these snippets reveal state:

```python
# What's selected?
# Maya:
import maya.cmds as cmds
cmds.ls(sl=True)

# Blender:
import bpy
bpy.context.selected_objects

# Current scene file
# Maya: cmds.file(q=True, sn=True)
# Blender: bpy.data.filepath

# Memory usage
import psutil
psutil.Process().memory_info().rss / 1024 / 1024  # MB

# Active skills
import dcc_mcp_core
# List loaded skills from server instance
```

## Gateway telemetry

```bash
# Session-scoped stats (only for gateway-routed calls)
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id <task-id>

# Job status polling
dcc-mcp-cli call <instance>.jobs_get_status \
  --json '{"job_id": "<job-uuid>"}'
```

## Anatomy of a failing tool call

```
1. Agent dispatches call("tool", {args})
2. Gateway receives → routes to instance
3. Adapter receives → resolves skill → runs script
4. Script executes: lazy imports → host API call → return
5. If error: script raises → adapter catches → returns error envelope
6. Gateway forwards error to agent

Diagnose at each layer:
- Gateway: check health, routing
- Adapter: check process_status, error_report
- Script: check audit_log, trace_tool
```
