---
name: nuke-scripting
description: >-
  Thin harness for last-resort Python execution inside Nuke. Prefer a typed
  Nuke skill whenever one fits; use this only for missing host operations.
license: MIT
compatibility: "Nuke 14+ Python 3.9; dcc-mcp-core 0.19.45+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "1.0.0"
    layer: thin-harness
    stage: compositing
    search-hint: "nuke execute python file_path materialize script last resort host scripting"
    tags: "nuke,compositing,script,destructive"
    tools: tools.yaml
---

# Nuke Scripting

Prefer `search_skills` and a typed Nuke tool. When no typed operation covers the
task, call `materialize_script` and pass its `file_path` to
`nuke_scripting__execute_python`. The executor accepts only Core materialized
Python owned by the current Nuke process and verifies its path, metadata, hash,
length, and expiry before execution.

Operators can disable this escape hatch with either
`DCC_MCP_NUKE_DISABLE_EXECUTE_PYTHON=1` or
`DCC_MCP_NUKE_DISABLE_ARBITRARY_SCRIPT=1`.
