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
  category: modeling
  dcc_vendor: Autodesk
groups:
  - name: modeling
    description: Polygon modeling primitives and edits (always active by default)
    default-active: true
    tools: [create_sphere, bevel_edges]
  - name: rigging
    description: Skeleton and deformation tools (activate with activate_tool_group)
    default-active: false
    tools: [create_joint]
tools:
  - name: create_sphere
    description: Create a polygon sphere with the given radius and subdivisions
    group: modeling
    input_schema:
      type: object
      properties:
        radius:
          type: number
          description: Sphere radius in Maya units
          default: 1.0
        subdivisionsX:
          type: integer
          description: Subdivisions around the equator
          default: 20
        subdivisionsY:
          type: integer
          description: Subdivisions from pole to pole
          default: 20
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/create_sphere.py
    next-tools:
      on-success: [maya_geometry__bevel_edges, maya_pipeline__export_usd]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]

  - name: bevel_edges
    description: Apply bevel to selected polygon edges
    group: modeling
    input_schema:
      type: object
      properties:
        offset:
          type: number
          description: Bevel offset distance
          default: 0.1
        segments:
          type: integer
          description: Number of bevel segments
          default: 1
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/bevel_edges.py
    next-tools:
      on-success: [maya_pipeline__export_usd]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]

  - name: create_joint
    description: Create a skinning joint at the current selection (rigging group — inactive by default)
    group: rigging
    input_schema:
      type: object
      properties:
        name:
          type: string
          description: Joint node name
          default: joint1
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/create_joint.py
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
