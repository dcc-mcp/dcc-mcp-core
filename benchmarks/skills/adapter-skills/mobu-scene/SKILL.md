---
name: mobu-scene
description: >-
  Host skill - inspect and explicitly save the active MotionBuilder scene. Use
  when checking scene metadata, model names, or saving a known FBX path. Not for raw Python execution.
license: MIT
compatibility: "MotionBuilder Python API; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: mobu
    version: "0.1.0"
    layer: domain
    stage: scene
    search-hint: "motionbuilder mobu scene models inspect save fbx"
    tags: "motionbuilder, mobu, animation, scene"
    tools: tools.yaml
---

# MotionBuilder Scene

Use these tools to inspect the currently open MotionBuilder scene, list its models, and save it to an explicit `.fbx` path.

`save_scene` changes files on disk. Confirm the intended target path before calling it.
