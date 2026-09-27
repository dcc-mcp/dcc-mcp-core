---
name: photoshop-script
description: Execute named Photoshop Actions
license: MIT
allowed-tools:
- Bash
- Read
metadata:
  dcc-mcp:
    dcc: photoshop
    version: 0.1.0
    layer: domain
    tags:
    - photoshop
    - action
    - batchplay
    search-hint: execute Photoshop action, action set, batch play, photoshop
    tools: tools.yaml
---
# photoshop-script

Execute named Photoshop Actions via the adobepy broker.

## Tools

- `execute_action` — Execute a named Photoshop Action from an Action Set
