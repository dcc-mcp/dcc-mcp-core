---
name: dcc-diagnostics
description: >-
  Infrastructure skill — DCC-agnostic observability primitives: capture
  screenshots, query audit logs, inspect tool performance metrics, and monitor
  process health. Works in any DCC environment (Maya, Blender, Houdini, Unreal,
  etc.) or standalone Python. Use for debugging any skill failure or verifying
  DCC state. Not for primary task execution — use a domain skill for actual DCC
  operations.
license: MIT
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "screenshot, capture, audit log, metrics, performance, process monitor, diagnostics, debug, health check, observability"
  dcc-mcp.tags: "diagnostics, observability, screenshot, audit, metrics, debug, infrastructure"
  dcc-mcp.tools: tools.yaml
---

# DCC Diagnostics

Cross-DCC observability and debugging tools powered by `dcc-mcp-core`.

All tools work in any DCC environment (Maya, Blender, Houdini, Unreal, 3ds Max)
or standalone Python — no DCC-specific APIs required.

## Tools

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
# dcc_diagnostics__screenshot is now available as an MCP tool
```
