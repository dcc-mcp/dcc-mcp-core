---
name: zbrush-scripting
description: >-
  Thin-harness skill — controlled Python execution and session diagnostics inside
  ZBrush 2026.1+. Use when no typed ZBrush skill covers the task or you need
  version/subtool counts. Not for routine scene edits — prefer zbrush-scene or
  domain skills first.
license: MIT
compatibility: "dcc-mcp-zbrush 0.2+, ZBrush 2026.1+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: zbrush
    layer: thin-harness
    stage: bootstrap
    version: "1.0.0"
    tags: [zbrush, scripting, python, automation, bootstrap]
    search-hint: "execute python, run script, zbrush.commands, session info, version"
    tools: tools.yaml
---

# zbrush-scripting

Bootstrap escape hatch. Call typed tools from `zbrush-scene`, `zbrush-subtool`, or
`zbrush-interchange` before `execute_python`.

## When to use

- Verify MCP connectivity → `get_session_info`
- One-off SDK experiment with no typed tool yet → `execute_python`

## Safety

- All tools use `affinity: main` because ZBrush APIs are host-bound.
- `execute_python` is destructive and non-idempotent by default.
