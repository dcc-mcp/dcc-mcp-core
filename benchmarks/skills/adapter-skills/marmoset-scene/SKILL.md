---
name: marmoset-scene
description: >-
  Domain skill - inspect, import models, create explicit PBR materials, control
  visibility, save, and render the active Marmoset Toolbag scene through typed
  host operations. Use for Toolbag scene and camera-output workflows. Not for
  arbitrary Python execution.
license: MIT
compatibility: "Marmoset Toolbag 4.03+ or 5.x; Python 3.9+; dcc-mcp-core 0.20.14+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: marmoset
    layer: domain
    version: "0.1.2"  # x-release-please-version
    stage: scene
    search-hint: "Marmoset Toolbag scene objects import model PBR material visibility save tbscene render camera image"
    tags: "marmoset,toolbag,scene,lookdev,rendering"
    tools: tools.yaml
---

# Marmoset Toolbag Scene

Inspect the scene before mutating it. File tools require absolute paths and do
not create missing parent folders. Treat render calls as monolithic Toolbag
operations: poll the returned Core job and do not retry after a transport
timeout until the output path and job status have been checked.
