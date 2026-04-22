---
name: dcc-diagnostics
description: >-
  Infrastructure skill — DCC-agnostic observability primitives: generate error
  reports, capture screenshots, query audit logs, inspect tool performance
  metrics, and monitor process health. Works in any DCC environment (Maya,
  Blender, Houdini, Unreal, etc.) or standalone Python. Call
  dcc_diagnostics__error_report first whenever a tool fails with a vague error
  message. Not for primary task execution — use a domain skill for actual DCC
  operations.
license: MIT
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.1.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "error report, error log, failure, debug, maya error, blender error, houdini error, tool failed, mcp tool execution failed, screenshot, capture, audit log, metrics, performance, process monitor, diagnostics, health check, observability, job failed, job history, log file"
  dcc-mcp.tags: "diagnostics, observability, error, screenshot, audit, metrics, debug, infrastructure"
  dcc-mcp.tools: tools.yaml
---

# DCC Diagnostics

Cross-DCC observability and debugging tools powered by `dcc-mcp-core`.

All tools work in any DCC environment (Maya, Blender, Houdini, Unreal, 3ds Max)
or standalone Python — no DCC-specific APIs required.

## Recommended debugging workflow

When a DCC tool fails with a vague error, call tools in this order:

```
1. dcc_diagnostics__error_report   ← start here: log lines + failed jobs + env
2. dcc_diagnostics__audit_log      ← sandbox-level denials and recent invocations
3. dcc_diagnostics__tool_metrics   ← identify consistently slow or failing tools
4. dcc_diagnostics__screenshot     ← capture current visual state for confirmation
5. dcc_diagnostics__process_status ← check if the DCC process is still alive
```

## Tools

### `dcc_diagnostics__error_report`

**Start here when any tool fails.** Collects a single-response diagnostic bundle:

- **Log tail**: last N lines of `dcc-mcp-<dcc>.*.log`, extracting ERROR/WARNING lines
- **Failed jobs**: recent failed/interrupted entries from the SQLite job-persistence DB
- **Process snapshot**: PID, platform, Python version, active `DCC_MCP_*` env vars
- **Diagnosis hints**: actionable text explaining what is wrong and how to fix it

> Requires `DccServerBase(enable_file_logging=True, enable_job_persistence=True)`.
> Both are on by default since dcc-mcp-core v0.14.6.

### `dcc_diagnostics__screenshot`

Capture the current screen or a specific window as a PNG/JPEG image.
Backed by the `dcc_mcp_core.Capturer` class which uses:

- **Windows**: DXGI Desktop Duplication API (<16ms per frame)
- **Linux**: X11 XShmGetImage
- **Fallback**: Mock synthetic backend (headless/CI)

### `dcc_diagnostics__audit_log`

Query the sandbox audit log from `dcc_mcp_core.SandboxContext`.
Returns recent tool invocations with outcome (success/denied) and timestamps.

### `dcc_diagnostics__tool_metrics`

Inspect per-tool performance counters from `dcc_mcp_core.ToolRecorder`:
invocation count, success rate, average latency, P95/P99 percentiles.

### `dcc_diagnostics__process_status`

Check process health via `dcc_mcp_core.PyProcessMonitor`.
Lists tracked PIDs and their liveness status.

## Usage with any DCC MCP server

```python
import os
os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/dcc-diagnostics"

from dcc_mcp_maya import start_server  # or dcc_mcp_blender, etc.
handle = start_server(port=8765)
# dcc_diagnostics__error_report and all other tools are now available
```
