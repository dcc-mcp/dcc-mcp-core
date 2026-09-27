---
name: godot-batch-refactor
description: >-
  Domain skill — Find edited-scene nodes by class. Return scene signal connections. Set a property on matching edited-scene nodes. Search project text for a node path or name. Return dependencies for a scene resource. Update matching nodes across project scenes. Find project references to a script or resource. Detect circular PackedScene dependencies.
license: MIT
compatibility: "Godot 4.4+; dcc-mcp-core 0.19+"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: godot
    layer: domain
    version: "0.1.0"
    search-hint: "Godot batch-refactor find_nodes_by_type find_signal_connections batch_set_property find_node_references get_scene_dependencies cross_scene_set_property find_script_references detect_circular_dependencies"
    tags: "godot,batch-refactor,game-development"
    tools: tools.yaml
---

# Godot Batch Refactor

Use these editor-integrated tools after opening the target Godot project. Paths must remain under `res://`.
