---
name: write-temp-file
description: >
  Write a code string to a managed temp file and return the path.
  Use the returned file_path as execute_python(file_path=...) to avoid
  passing long code strings (and their escaping problems) in the
  execute_python tool call.
license: MIT
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    version: "1.0.0"
    layer: thin-harness
    tags: [utility, scripting]
---

Write code strings to managed temporary files for DCC script execution.
