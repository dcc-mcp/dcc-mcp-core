---
name: houdini-constraints
description: >-
  Pipeline skill — typed animation constraint tools for object-level
  transform constraints. Create parent, blend, position, and orientation
  constraints using OBJ-level blend nodes and CHOP-driven channel
  referencing. List and delete existing constraints.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 20.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [houdini, constraints, parent, blend, position, orient, animation, pipeline]
    search-hint: "constraint parent blend position orient transform animation list delete"
    tools: tools.yaml
---

# houdini-constraints

Typed animation constraint tools for agents. All tools are `affinity: main`.

Constraints are implemented via OBJ-level **blend** nodes (the idiomatic
Houdini equivalent of Maya-style parent/position/orient constraints) and
CHOP-driven channel referencing for finer control.

## Tool groups

- **`parent`:** `create_parent_constraint` — drive an object's full transform
  from a single target.
- **`blend`:** `create_blend_constraint` — blend between multiple target
  transforms with per-target weights.
- **`position`:** `create_position_constraint` — constrain only translation
  channels.
- **`orientation`:** `create_orient_constraint` — constrain only rotation
  channels.
- **`inspect`:** `list_constraints` — list constraint nodes/relationships in
  the scene.
- **`manage`:** `delete_constraint` (destructive) — remove a constraint node.

## Tracer-bullet flow

1. `create_parent_constraint(driven_path="/obj/geo2", target_path="/obj/geo1")`
2. `list_constraints(context_path="/obj")` → verify the blend node exists
3. `create_blend_constraint(driven_path="/obj/geo3", target_paths=["/obj/geo1", "/obj/geo2"], weights=[0.3, 0.7])`
4. `delete_constraint(node_path="/obj/blend_geo2")`

Position/orient constraints use CHOP extraction to isolate transform
components, giving more granular control than a full parent constraint.
