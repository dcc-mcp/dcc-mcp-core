---
name: designer-lighting
description: >-
  Host skill - bake lighting-response maps (normal, ambient occlusion, curvature,
  thickness, emissive) from a height or mask source in Substance 3D Designer.
license: MIT
compatibility: "Substance 3D Designer 12.1+; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_designer
    version: "0.8.1" # x-release-please-version
    layer: domain
    stage: authoring
    search-hint: "substance designer lighting normal ambient occlusion curvature thickness emissive height bake"
    tags: "substance, designer, lighting, normal, ambientOcclusion, curvature, thickness, emissive"
    tools: tools.yaml
---

# Designer Lighting

## Honest capability boundary

Substance 3D Designer is a procedural texture authoring host. It has **no
lights, no scene graph, and no light transport**. Nothing here places a light,
sets an intensity, or renders a lit preview.

What Designer does provide are converter nodes that derive lighting-response
maps from a height or mask field. Those maps encode surface response that a
downstream renderer (Blender, Unreal, Maya, Houdini) consumes when it performs
the actual lighting. Use this skill to author those maps, not to light a scene.

This skill is **experimental**: recipe availability is a runtime property, not a
documented guarantee. The 6 lighting recipes resolve against the live
node-definition inventory at call time, so treat the `available` flag reported
by `list_light_recipes` as the only source of truth for whether a recipe exists
in the running Designer build. Never assume a recipe name implies a node type.

## Recipes resolve against the live inventory

Node type URLs vary between Designer builds. Run `list_light_recipes` first: it
reports each recipe with the `type_url` it resolved to and an `available` flag.
A recipe that resolves to nothing fails closed with `RECIPE_UNAVAILABLE` and
lists the candidates it tried, without creating any node.

## Tool notes

- `bake_lighting_maps` resolves every requested map before creating any node.
  If a step fails, the nodes that call created are removed.
- Each derived node is connected from the single `source_node.source_property`
  you provide, then exported through the adapter's header-verified native map
  export, so every returned file is proven fresh and dimension-checked.
- Input and output ports are resolved by discovery, not assumption: texture-typed
  ports win, then conventionally named ports, then the first available port.
- The typical height-driven lighting chain is
  `normal_from_height` + `ambient_occlusion` + `curvature`.

`designer-effects` shares the same recipe-resolution mechanism for blur, warp,
levels, sharpen, edge detect, blend, and mask recipes.
