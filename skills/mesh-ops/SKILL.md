---
name: mesh-ops
description: >-
  Cross-adapter polygon modeling recipe pack for the shared cross-DCC modeling
  vocabulary. Structured RECIPES.yaml entries for primitive creation,
  hard-surface edits, topology cleanup, UV generation, and material assignment.
  Reusable by every DCC adapter's mesh-ops skill (maya, blender, houdini,
  3dsmax, and custom hosts) before falling back to raw scripting.
license: "MIT"
compatibility: "dcc-mcp-core 0.20.29+, Python 3.7+"
allowed-tools: ["Read"]
metadata:
  dcc-mcp:
    dcc: any
    layer: domain
    stage: modeling
    version: "1.0.0"
    tags: [modeling, mesh, polygon, topology, recipe, cross-dcc]
    search-hint: >-
      modeling recipe, mesh recipe, create primitive, bevel edges, extrude
      faces, inset faces, boolean, edge loop, merge vertices, triangulate,
      cleanup mesh, combine meshes, mirror, loft sections, lathe profile,
      auto UV, UV projection, assign material, set pivot
    intent: "Look up a structured, DCC-agnostic modeling recipe before dispatching an adapter's mesh-ops tools."
    side-effects:
      modifies: false
      creates: false
      deletes: false
      targets: []
    produces: [modeling_recipe]
    requires: []
    recipes: RECIPES.yaml
---

# mesh-ops

Cross-adapter polygon modeling recipes for the shared cross-DCC modeling
vocabulary. Every recipe is a DCC-agnostic plan: a validated `inputs_schema`,
an ordered list of `steps` that dispatch the canonical `mesh_ops__<verb>` tool,
an `output_contract`, and a `toolset_profiles: [mesh-ops]` binding. Adapters
resolve `mesh_ops__<verb>` to their own `<adapter>_mesh_ops__<verb>` tool
(`maya_mesh_ops__*`, `blender_mesh_ops__*`, `houdini_mesh_ops__*`,
`3dsmax_mesh_ops__*`, and custom hosts).

## How to use

1. `recipes__list` with `skill: mesh-ops` to see the available recipes.
2. `recipes__get` a recipe to read its `inputs_schema`, `steps`, and
   `output_contract`.
3. `recipes__validate` candidate inputs against the recipe schema before
   dispatching.
4. `recipes__apply` to obtain the execution plan, then dispatch the returned
   `steps` through the owning adapter's mesh-ops tools.

## Cross-adapter contract

- `dcc: any` — a recipe is not tied to one host; the same plan applies in
  every DCC adapter that implements the shared modeling verb.
- `toolset_profiles: [mesh-ops]` — the recipe belongs to the mesh-ops toolset
  carried by each adapter's mesh-ops skill.
- `steps[].tool` uses the canonical `mesh_ops__<verb>` name, never an
  adapter-specific slug.
- `inputs_schema` is a Draft 2020-12 schema validated by Core without a
  runtime `jsonschema` dependency.

Read `RECIPES.yaml` for the full recipe list.
