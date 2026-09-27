---
name: nuke-node-graph
description: >-
  Host skill - inspect and edit a Nuke node graph through bounded typed CRUD.
  Use for non-destructive node creation, exact topology changes, and static
  knob readback. Not for Python execution, animated knobs, or rendering.
license: MIT
compatibility: "Nuke Python API; dcc-mcp-core 0.19+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: nuke
    version: "0.0.0"
    layer: domain
    stage: scene
    search-hint: "nuke node graph create delete connect input inspect knob topology compositing"
    tags: "nuke, compositing, nodes, graph"
    tools: tools.yaml
---

# Nuke Node Graph

Inspect with `nuke_script__list_nodes` before mutating. Create one node at a
time, connect exact input indices, and read back changed knobs. The surface
never clears the graph and rejects executable or sensitive knob classes.

Save the script explicitly only after the graph assertions pass.
