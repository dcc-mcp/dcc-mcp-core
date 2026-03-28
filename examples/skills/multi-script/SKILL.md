---
name: multi-script
description: "Demonstrates a skill with multiple script types (Python, Shell, Batch)"
tools: ["Bash", "Read"]
tags: ["example", "multi-language"]
dcc: python
version: "1.0.0"
---

# Multi-Script Skill

This skill demonstrates that `dcc-mcp-core` supports multiple script types
within a single skill package. The scanner will discover all files with
supported extensions under the `scripts/` directory.

## Supported Extensions

| Extension | Type |
|-----------|------|
| `.py` | Python |
| `.sh` | Shell |
| `.bat` | Batch |
| `.ps1` | PowerShell |
| `.mel` | MEL (Maya) |
| `.ms` | MaxScript |
| `.js` | JavaScript |
