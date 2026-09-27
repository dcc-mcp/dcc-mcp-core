---
name: painter-diagnostics
description: Read-only readiness diagnostics for a running Substance 3D Painter adapter.
license: MIT
compatibility: "Substance 3D Painter Python API; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_painter
    version: "1.0.0"
    layer: host
    stage: diagnostics
    search-hint: "substance painter readiness ping main thread adapter health"
    tags: "substance,painter,diagnostics,readiness,ping"
    tools: tools.yaml
---

# Painter diagnostics

Use `ping` to prove that the embedded server can dispatch a fresh call on
Painter's main thread. A registry entry or reachable HTTP endpoint alone does
not prove that the host event loop is servicing adapter work.
