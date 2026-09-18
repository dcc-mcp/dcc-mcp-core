---
name: mesh-ops
description: >-
  Cross-adapter polygon modeling recipe pack for the shared cross-DCC modeling
  vocabulary. Structured RECIPES.yaml entries for primitive creation,
  hard-surface edits, topology cleanup, UV generation, radial arraying, and
  material assignment. Reusable by every DCC adapter's mesh-ops skill (maya,
  blender, houdini, 3dsmax, and custom hosts) before falling back to raw
  scripting.
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
      faces, inset, boolean, edge loop, merge vertices, triangulate,
      cleanup mesh, combine meshes, mirror, loft sections, lathe profile,
      radial array, auto UV, UV projection, assign material, set pivot
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
a Draft 2020-12 `output_contract`, and a `toolset_profiles: [mesh-ops]`
binding.

## How to use

1. `recipes__list` with `skill: mesh-ops` to see the available recipes.
2. `recipes__get` a recipe to read its `inputs_schema`, `steps`, and
   `output_contract`.
3. `recipes__validate` candidate inputs against the recipe schema before
   dispatching.
4. `recipes__apply` to obtain the execution plan, then materialize and
   dispatch the returned `steps` (see "Dispatch contract" below).

## Cross-adapter contract

- `dcc: any` — a recipe is not tied to one host; the same plan applies in
  every DCC adapter that implements the shared modeling verb.
- `toolset_profiles: [mesh-ops]` — the recipe belongs to the mesh-ops toolset
  carried by each adapter's mesh-ops skill.
- `steps[].tool` uses the canonical `mesh_ops__<verb>` name, never an
  adapter-specific slug.
- `inputs_schema` is a Draft 2020-12 schema validated by Core without a
  runtime `jsonschema` dependency.

## Dispatch contract

`recipes__apply` returns the plan **verbatim**. Core does three things only:
validate `inputs` against `inputs_schema`, attach the `output_contract`, and
return the `steps`. It does **not** substitute `${param}` placeholders and does
**not** apply `inputs_schema` defaults. The caller must materialize each step:

```text
step.arguments[k] == "${name}"  ->  inputs[name] if present
                                    else inputs_schema.properties[name].default
```

Every placeholder therefore names an input property that is either listed in
`required` or carries a `default`, so a materialized step never still contains
a `${...}` string. The test suite asserts this for every recipe.

Resolve `mesh_ops__<verb>` through `tool_routing` in `RECIPES.yaml`:

- `tool_routing.tools[<verb>][<adapter>]` — the concrete tool that adapter
  publishes (`blender_mesh_ops__bevel_edges`, `maya_mesh_ops__mirror_mesh`,
  `houdini_materials__assign_material`, …).
- `null` — the adapter publishes no equivalent. Use
  `tool_routing.scripting_fallback[<adapter>]` and project the script result
  onto the `output_contract` yourself.

`tool_routing` resolves the tool, not the argument shape. Step arguments use
the canonical vocabulary above; projecting them onto an adapter's own
signature is that adapter's job — Maya's `combine_meshes` takes `objects` and
`name` where the canonical recipe says `object_names` and `new_name`, and
Maya's `loft_sections` takes `name` where the recipe says `output_name`. When
an adapter cannot express a canonical argument, use the scripting fallback
rather than dropping it.

## Output contract

`output_contract` is a Draft 2020-12 schema, not a label. After the tool runs,
project the observed result onto the declared receipt and validate it. The
contracts are deliberately fail-able:

- every mutating recipe requires `changed: true`, so a tool that reports
  success without touching the mesh cannot pass;
- `get_poly_count` requires `changed: false`, so a mutating implementation
  cannot pass a read-only recipe;
- component edits require the affected component list plus a post-operation
  element count;
- `auto_uv` requires `uv_count >= 1` **and** a SHA-256 `uv_digest`, so a mesh
  that ends up without UVs fails the contract — the failure mode that shipped
  an FBX with 31% of meshes un-UV'd.

Validate with `jsonschema.Draft202012Validator`, or with Core's dependency-free
validator (`dcc_mcp_core._runtime.recipe_schema._RecipeSchemaValidator`) on
Python 3.7 hosts.

## Adapter-local duplicates

Some adapters ship their own recipe pack (for example Maya's
`maya-mesh-ops`). `adapter_recipe_aliases` in `RECIPES.yaml` maps each
cross-adapter verb to the adapter-local recipe name. When a
`recipes__search` query matches both, prefer the adapter-local entry
(`dcc: <adapter>`) — its steps and contract are verified against that host's
real tools — and fall back to the cross-adapter entry (`dcc: any`) when the
adapter publishes no local recipe.

Read `RECIPES.yaml` for the full recipe list.
