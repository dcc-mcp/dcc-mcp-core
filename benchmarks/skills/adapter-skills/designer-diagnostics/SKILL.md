---
name: designer-diagnostics
description: >-
  Host diagnostics skill - prove that the active Substance 3D Designer
  application can answer a typed, main-thread readiness probe.
license: MIT
compatibility: "Substance 3D Designer 12.1+; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_designer
    version: "0.8.1" # x-release-please-version
    layer: bootstrap
    stage: bootstrap
    search-hint: "substance designer diagnostics ping readiness version"
    tags: "substance,designer,diagnostics,ping,readiness"
    tools: tools.yaml
---

# Designer Diagnostics

Use the read-only ping to prove that a live Designer application can execute a
typed tool on its host main thread. This skill never authors or changes a graph.
