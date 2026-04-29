---
name: maya-geometry
description: >-
  Domain skill — Maya geometry primitives: create spheres, cubes, cylinders;
  bevel, extrude, and merge polygon components. Use for individual geometry
  creation or editing operations inside Maya. Not for full asset export
  pipelines — use maya-pipeline for that. Not for USD scene inspection — use
  usd-tools for that.
license: MIT
compatibility: Maya 2022+, Python 3.7+
allowed-tools: Bash Read Write
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: domain
  dcc-mcp.search-hint: "create sphere, create cube, bevel edges, extrude faces, polygon modeling, Maya mesh, 3D primitives, rigging joint"
  dcc-mcp.tags: "maya, geometry, modeling, polygon, domain"
  dcc-mcp.tools: tools.yaml
  dcc-mcp.groups: groups.yaml
---

# Maya Geometry Tools

Tools for creating and modifying 3D geometry inside Maya.

## Tools

### `maya_geometry__create_sphere`
Create a UV sphere with configurable radius and subdivisions.

### `maya_geometry__bevel_edges`
Apply a bevel operation to currently selected polygon edges.

## Prerequisites

- Autodesk Maya 2022 or later
- Python interpreter accessible from Maya's script editor

## Notes

All scripts use Maya's Python API (`import maya.cmds as mc`). They must be
run inside a Maya process — either directly or via the DeferredExecutor bridge.
