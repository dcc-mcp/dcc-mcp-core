---
name: forgecad-primitives
description: >-
  ForgeCAD-style third-party skill — create basic CAD primitives (cube,
  cylinder, cone) inside a ForgeCAD-like environment. Used by the E2E
  multi-service test suite to validate third-party skill ecosystem discovery,
  loading, and invocation.
license: MIT
compatibility: Python 3.7+
allowed-tools: Bash Read
metadata:
  dcc-mcp.dcc: forgecad
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: domain
  dcc-mcp.search-hint: "create cube, create cylinder, create cone, forgecad, cad primitives"
  dcc-mcp.tags: "forgecad, cad, geometry, primitives, domain"
  dcc-mcp.tools: tools.yaml
---

# ForgeCAD Primitives

Third-party domain skill providing basic CAD geometry creation for the
ForgeCAD environment.

## Tools

### `forgecad_primitives__create_cube`
Create a cube with configurable edge length.

```python
{"name": "forgecad_primitives__create_cube", "arguments": {"edge": 2.0, "marker": "exe-forgecad-d"}}
# → {"success": true, "shape": "cube", "edge": 2.0, "marker": "exe-forgecad-d"}
```

### `forgecad_primitives__create_cylinder`
Create a cylinder with configurable radius and height.

```python
{"name": "forgecad_primitives__create_cylinder", "arguments": {"radius": 1.0, "height": 3.0}}
# → {"success": true, "shape": "cylinder", "radius": 1.0, "height": 3.0}
```
