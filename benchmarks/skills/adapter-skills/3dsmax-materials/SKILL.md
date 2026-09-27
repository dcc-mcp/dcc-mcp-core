---
name: 3dsmax-materials
description: >-
  Domain skill - inspect, create, edit, and assign 3ds Max materials and
  bitmap maps on the main thread. Use when authoring Standard, Physical, or
  PBR-friendly materials, assigning node materials, and reporting texture
  connections.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max material create inspect assign bitmap texture map physical pbr roughness metalness missing paths"
    tags: "3dsmax, materials, shader, assignment, bitmap, pbr"
    tools: tools.yaml
    intent: "Create, inspect, edit, and assign 3ds Max materials and bitmap textures."
    search_aliases: ["materials", "materials"]
    recall_context:
      app_type: "3dsmax"
      domain: "materials"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: true
      modifies: true
      deletes: false
      exports: false
      imports: false
      file_output: false
      render: false
      targets: ["material", "bitmap", "scene_node"]
    produces: ["material:standard", "material:physical", "material:assignment", "bitmap_connection"]
---

# 3ds Max Material Tools

Inspect, create, edit, and assign 3ds Max materials in the current scene. All
tools touch the live scene through `pymxs`, so they declare `affinity: main`.

Tool contracts live in `tools.yaml`. `apply_material` uses current selection
when `node_names` is omitted.

## Verified writes

Every write in this skill is verified by readback and reported per attribute or
per node, so a caller can always tell "written" from "not written":

- `create_standard_material`, `create_physical_material`, and
  `create_pbr_material` return `applied` / `applied_count` / `errors` and fail
  when a requested attribute did not land.
- `apply_material` reports `skipped` (node names that could not be resolved)
  and `errors` (nodes whose assignment the host did not keep) and fails when
  either is non-empty; `applied_count` counts only verified assignments.
- `reset_material` verifies each reset and fails when a node still reports its
  previous material.

## Renderer-aware parameters and slots

`create_material_from_textures`, `assign_bitmap_texture`, and
`set_material_attributes` resolve canonical PBR controls onto the
renderer-native property and verify every write by readback:

| Canonical | VRayMtl | Arnold | Physical |
|---|---|---|---|
| roughness | `reflectionRoughness` (needs `brdf_useRoughness = true`), else `reflection_glossiness` inverted | `specular_roughness` | `base_roughness` |
| metalness | `metalness` | `metalness` | `base_metalness` |
| diffuse map | `texmap_diffuse` | `baseColorMap` | `base_color_map` |
| roughness map | `texmap_roughness` / `texmap_reflectionRoughness` | `specularRoughnessMap` | `base_roughness_map` |
| metalness map | `texmap_metalness` | `metalnessMap` | `base_metalness_map` |
| normal map | `texmap_bump` (Normal Bump wrapper) | `normalMap` | `bump_map` |
| bump map | `texmap_bump` | `bumpMap` | `bump_map` |
| displacement map | `texmap_displacement` | `displacementMap` | `displacement_map` |
| opacity map | `texmap_opacity` | `opacityMap` | `cutout_map` |

A parameter or slot the material class does not expose fails the tool call and
rolls the created material back; it is never reported as a silent success.
Generic attribute names that fall outside the renderer tables go through the
same readback check: a host that refuses every candidate, or silently accepts
one it never persists, is reported as an error rather than a success.

`assign_bitmap_texture` reports the resulting connections through the same
renderer-native slot names, so a write to `texmap_roughness` shows up in
`connections` instead of returning an empty list.

Texture-set files are matched on filename tokens (`basecolor`, `albedo`,
`diffuse`, `roughness`, `metalness`, `normal`, `bump`, `displacement`,
`height`, `opacity`, `alpha`, `specular`, `emission`, `reflection`, `ao`).
