---
name: async-render-example
description: >-
  Example skill — demonstrates async execution affinity: long-running tools
  surface as deferredHint=true in MCP tools/list. Use as a reference when
  writing domain skills with render or simulation tools. Not intended for
  production use.
license: MIT
compatibility: Python 3.7+
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "async tool, long-running, deferred hint, timeout hint, render async, authoring reference"
  dcc-mcp.tags: "example, async, render, deferred"
tools:
  - name: render_frames
    description: "Pretend to render a frame range. Long-running; the server surfaces `deferredHint: true` and `_meta.dcc.timeoutHintSecs`."
    execution: async
    timeout_hint_secs: 600
  - name: quick_status
    description: "Return the current (fake) render status. Fast, sync."
    execution: sync
---

# Async Render Example (issue #317)

This skill demonstrates the `execution` and `timeout_hint_secs` fields added
for issue #317. Tools are declared with either `execution: sync` (default)
or `execution: async`; the MCP server derives the `deferredHint` annotation
from this value and surfaces `timeout_hint_secs` under
`_meta.dcc.timeoutHintSecs` on the tool definition (never inside
`annotations`).

## Behaviour

- `render_frames` → `execution: async` + `timeout_hint_secs: 600`
  → `tools/list` entry gets `"annotations": { "deferredHint": true, ... }`
  and `"_meta": { "dcc": { "timeoutHintSecs": 600 } }`.
- `quick_status` → no `execution` field → default `Sync`
  → `"annotations": { "deferredHint": false, ... }` and no `_meta`.

## Script convention

Each script reads JSON parameters from stdin and writes a JSON result to
stdout — the standard dcc-mcp-core skill script contract.
