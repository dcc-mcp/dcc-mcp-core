# Debug Recipes

Copy-pasteable diagnosis sequences for common DCC failure scenarios.

## Tool returned a vague error

```bash
# 1. Run error report (start here!)
dcc-mcp-cli call <instance>__dcc_diagnostics__error_report \
  --json '{"dcc_name": "maya", "tail_lines": 200}'

# 2. Check audit log for denials
dcc-mcp-cli call <instance>__dcc_diagnostics__audit_log \
  --json '{"filter": "denied", "limit": 50}'

# 3. Check tool metrics for patterns
dcc-mcp-cli call <instance>__dcc_diagnostics__tool_metrics \
  --json '{"sort_by": "failure_rate", "limit": 10}'

# 4. Capture visual state
dcc-mcp-cli call <instance>__dcc_diagnostics__screenshot \
  --json '{"format": "png"}'
```

## Tool hangs or times out

```bash
# Check process health
dcc-mcp-cli call <instance>__dcc_diagnostics__process_status \
  --json '{}'

# Check if tool has excessive latency
dcc-mcp-cli call <instance>__dcc_diagnostics__tool_metrics \
  --json '{"sort_by": "p95_ms", "limit": 5}'

# Trace with timeout
dcc-mcp-cli call <instance>__debug__trace_tool \
  --json '{"tool_slug": "maya.a1b2c3d4.slow_tool", "arguments": {}, "capture_before": true}'
```

## Sandbox blocks a legitimate call

```bash
# Check what's being denied
dcc-mcp-cli call <instance>__dcc_diagnostics__audit_log \
  --json '{"filter": "denied", "action_name": "<suspected_action>", "limit": 20}'

# Check sandbox policy for this action
dcc-mcp-cli call <instance>__debug__check_sandbox \
  --json '{"action_name": "<action_name>"}'
```

## Collect evidence for bug report

```bash
# Full evidence bundle
dcc-mcp-cli call <instance>__debug__collect_evidence \
  --json '{"dcc_name": "maya", "output_dir": "/tmp/bug-report", "include_screenshot": true}'

# Or manually:
# Logs
dcc-mcp-cli call <instance>__dcc_diagnostics__error_report \
  --json '{"tail_lines": 500}' > error_report.json

# Screenshot
dcc-mcp-cli call <instance>__dcc_diagnostics__screenshot \
  --json '{"format": "png", "save_path": "/tmp/screenshot.png"}'

# Stats (if gateway-routed)
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

## Inspect current DCC state

```bash
# Full state snapshot
dcc-mcp-cli call <instance>__debug__inspect_state \
  --json '{"dcc_name": "maya"}'

# Python-level introspection (if eval is available)
dcc-mcp-cli call <instance>__dcc_introspect__eval \
  --json '{"code": "import maya.cmds as cmds; print(cmds.ls(sl=True))"}'

# Search available tools
dcc-mcp-cli call <instance>__dcc_introspect__search \
  --json '{"query": "scene"}'
```

## Job history for a session

```bash
# Via CLI stats
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42

# Via Python (if gateway has REST API)
curl -s http://localhost:19293/v1/stats?session_id=task-42 | python -m json.tool
```
