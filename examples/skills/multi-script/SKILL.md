---
name: multi-script
description: >-
  Example skill — demonstrates a skill with multiple script types (Python,
  Shell, Batch). Use as a reference when writing cross-platform skills. Not
  intended for production use — this is an authoring reference only.
license: MIT
compatibility: Python 3.7+; bash required on Linux/macOS; cmd on Windows
allowed-tools: Bash Read
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "multi-script, cross-platform, python shell batch, script types, authoring reference"
  dcc-mcp.tags: "example, multi-language, cross-platform"
  dcc-mcp.tools: tools.yaml
---

# Multi-Script Skill

A reference skill demonstrating **cross-platform, multi-language script support**
in `dcc-mcp-core`.

## Supported Script Extensions

| Extension | Language | Platform |
|-----------|----------|----------|
| `.py` | Python | All |
| `.sh` | Shell/Bash | Linux, macOS |
| `.bat` | Batch/CMD | Windows |
| `.ps1` | PowerShell | Windows |
| `.mel` | MEL | Maya |
| `.ms` / `.mcr` | MaxScript | 3ds Max |
| `.js` | JavaScript | Node.js |

## Tools

### `multi_script__action_python`
Cross-platform Python implementation — works on all operating systems.

### `multi_script__action_shell`
Bash shell implementation for Linux and macOS.

### `multi_script__action_batch`
Windows Batch (CMD) implementation for Windows environments.

## How the dispatcher chooses a script

When a tool is called via `tools/call`, the `SkillCatalog` uses the
`source_file` field in each `ToolDeclaration` to dispatch to the correct
script. If `source_file` is empty, it matches scripts by name stem.

## Script convention

All scripts in this skill read JSON parameters from **stdin** and write
a JSON result to **stdout**:

```python
# action_python.py
import json, sys
params = json.load(sys.stdin)
msg = params.get("message", "hello")
print(json.dumps({"success": True, "message": f"Python says: {msg}"}))
```
