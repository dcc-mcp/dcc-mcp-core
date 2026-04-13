---
name: dcc-diagnostics
description: "DCC-agnostic diagnostics and observability tools — capture screenshots, query audit logs, inspect action performance metrics, and monitor process health. Works in any DCC environment (Maya, Blender, Houdini, Unreal, etc.) or standalone Python."
license: MIT
dcc: python
version: "1.0.0"
search-hint: "screenshot, capture, audit log, metrics, performance, process monitor, diagnostics, debug, health check"
tags: [diagnostics, observability, screenshot, audit, metrics, debug]
metadata:
  category: diagnostics
tools:
  - name: screenshot
    description: "Capture a screenshot of the current display or a specific window. Returns the image as a base64-encoded PNG. Useful for visual debugging — capture what's visible on screen when an error occurs."
    input_schema:
      type: object
      properties:
        format:
          type: string
          description: "Image format: 'png' (default), 'jpeg', or 'raw_bgra'"
          default: png
        scale:
          type: number
          description: "Scale factor 0.0-1.0 (default 1.0 = native resolution). Use 0.5 to halve the size."
          default: 1.0
        jpeg_quality:
          type: integer
          description: "JPEG quality 0-100 (default 85). Only used when format is 'jpeg'."
          default: 85
        window_title:
          type: string
          description: "Capture only the window whose title contains this substring. If omitted, captures the full screen."
        save_path:
          type: string
          description: "If provided, save the image to this file path in addition to returning base64."
        timeout_ms:
          type: integer
          description: "Maximum milliseconds to wait for a frame (default 5000)."
          default: 5000
    read_only: true
    idempotent: false
    source_file: scripts/screenshot.py

  - name: audit_log
    description: "Query the dcc-mcp-core sandbox audit log — list recent action invocations, filter by outcome (success/denied), or search by action name. Helps diagnose why an action was blocked or what the agent did recently."
    input_schema:
      type: object
      properties:
        filter:
          type: string
          description: "Filter entries: 'all' (default), 'success', 'denied', or 'error'"
          default: all
        action_name:
          type: string
          description: "Only return entries for this specific action name."
        limit:
          type: integer
          description: "Maximum number of entries to return (default 50)."
          default: 50
    read_only: true
    idempotent: true
    source_file: scripts/audit_log.py

  - name: action_metrics
    description: "Show performance metrics for registered actions — invocation counts, success rates, average and P95/P99 latencies. Use to identify slow or failing tools."
    input_schema:
      type: object
      properties:
        action_name:
          type: string
          description: "If provided, return metrics only for this action. Otherwise return all."
        sort_by:
          type: string
          description: "Sort results by: 'name', 'invocations' (default), 'avg_ms', 'p95_ms', or 'failure_rate'"
          default: invocations
        limit:
          type: integer
          description: "Maximum number of actions to return (default 20)."
          default: 20
    read_only: true
    idempotent: true
    source_file: scripts/action_metrics.py

  - name: process_status
    description: "Check the health of tracked DCC processes — list running PIDs, check if a specific process is alive, and inspect crash recovery policy. Use when a DCC tool stops responding."
    input_schema:
      type: object
      properties:
        pid:
          type: integer
          description: "Check status of a specific process ID. If omitted, returns summary of all tracked processes."
    read_only: true
    idempotent: true
    source_file: scripts/process_status.py
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
Returns recent action invocations with outcome (success/denied) and timestamps.

### `dcc_diagnostics__action_metrics`

Inspect per-action performance counters from `dcc_mcp_core.ActionRecorder`:
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
