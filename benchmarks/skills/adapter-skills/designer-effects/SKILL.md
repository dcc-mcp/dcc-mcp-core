---
name: designer-effects
description: >-
  Host skill - apply discoverable procedural effect recipes (blur, warp, levels,
  sharpen, edge detect, blend, mask) to a Substance 3D Designer graph. Use when an
  Agent needs bounded effect-chain authoring instead of arbitrary node surgery.
license: MIT
compatibility: "Substance 3D Designer 12.1+; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_designer
    version: "0.8.1" # x-release-please-version
    layer: domain
    stage: authoring
    search-hint: "substance designer effects blur warp levels sharpen edge detect blend mask procedural chain"
    tags: "substance, designer, effects, filter, chain, procedural, authoring"
    tools: tools.yaml
---

# Designer Effects

Apply procedural effect recipes on top of an existing Designer graph.

## Honest capability boundary

Substance 3D Designer has no scene-level effect stack. An "effect" here is a
compositing or filter node inserted into the graph topology, created and wired
through the adapter's verified `create_node` and `connect_nodes` primitives.
There is no viewport effect, no post-process pass, and no shader authoring.

This skill is **experimental**: recipe availability is a runtime property, not a
documented guarantee. The 7 effect recipes resolve against the live
node-definition inventory at call time, so treat the `available` flag reported
by `list_effect_recipes` as the only source of truth for whether a recipe exists
in the running Designer build. Never assume a recipe name implies a node type.

## Recipe discovery instead of hardcoded node types

Node type URLs differ between Designer builds. Each recipe declares an ordered
list of candidate URLs and resolves the first one the live node-definition
inventory actually advertises.

- Run `list_effect_recipes` first. It reports every recipe with its resolved
  `type_url` and an `available` flag.
- A recipe that resolves to nothing fails closed with `RECIPE_UNAVAILABLE` and
  lists the candidates it tried. Nothing is created in that case.
- `apply_effect_chain` resolves every step before creating any node, so a
  missing type cannot leave a partial chain behind. Port lookup, every
  intermediate connection, the final target connection, and export are all inside
  one rollback unit: if any of them fails, the nodes created by that call are
  removed. The same guarantee applies to `apply_effect`.

## Tool notes

- `apply_effect` inserts one recipe node. Pass `target_node` and
  `target_property` to wire its output onward; omit them to leave the node
  unwired for manual inspection.
- `apply_effect_chain` wires `source_node.source_property` into step 1, then each
  step into the next, then the last step into the optional target. Ports are
  resolved by discovery, not by a fixed name: texture-typed ports win, then
  conventionally named ports (`input`/`output`, `source`, `result`), then the
  first available port. Do not assume a literal `input` or `output` id.
- Chain length is bounded to 12 steps. `spacing` spaces nodes horizontally in
  native graph coordinates from `origin`.
- Use `export_graph_state` from `designer-session` after a chain to verify the
  resulting topology.

## See also

`designer-lighting` uses the same recipe-resolution mechanism for lighting-map
recipes (normal, ambient occlusion, curvature, thickness, emissive).
