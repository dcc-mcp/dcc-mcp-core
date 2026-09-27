---
name: mobu-diagnostics
description: >-
  Host diagnostics for proving that the MotionBuilder API and embedded Python
  runtime are callable on the UI thread. Not for scene mutation or raw Python.
license: MIT
compatibility: "MotionBuilder 2023+; Python 3.9+; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: mobu
    version: "0.1.0"
    layer: diagnostics
    stage: host
    search-hint: "motionbuilder mobu diagnostics ping readiness embedded python"
    tags: "motionbuilder, mobu, diagnostics, readiness"
    tools: tools.yaml
---

# MotionBuilder Diagnostics

Use `ping` to prove that a live MotionBuilder process can execute a typed,
read-only call through the adapter's main-thread dispatcher.
