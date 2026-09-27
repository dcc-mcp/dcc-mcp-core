---
name: 3dsmax-display
description: >-
  Domain skill - manage 3ds Max display layers, node display state, and
  user-defined custom properties.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max display layers hidden frozen wire color object color viewport display mode custom user properties metadata layer properties renderable cast shadows receive shadows motion blur box mode back cull all edges ignore extents show trajectory xray display by layer inherit visibility visibility to reflections refractions"
    tags: "3dsmax, display, layers, layer-properties, custom-properties, metadata"
    tools: tools.yaml
    intent: "Manage 3ds Max display layers, node visibility, and user-defined custom properties."
    search_aliases: ["display", "display"]
    recall_context:
      app_type: "3dsmax"
      domain: "display"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: true
      modifies: true
      deletes: true
      exports: false
      imports: false
      file_output: false
      render: false
      targets: ["display_layer", "scene_node", "custom_property"]
    produces: ["display_layer", "layer_properties", "node_display_state", "custom_property"]
---

# 3ds Max Display And Metadata Tools

Manage display layers, inspect or change node display state, and read/write
user-defined node properties. Mutating tools require explicit node references
or explicit `use_selection=true` and report changed-node or changed-property
counts.

## Layer properties

`list_layers` accepts `include_properties=true` to read the full layer property
bag back, and `set_layer_properties` writes it. `set_layer_properties` takes one
`properties` object holding only the keys to change; an unrecognised key fails
the call and returns `supported`, so a typo never becomes a write the host
ignored.

Each write is confirmed by reading the layer back and classified the same way:
`applied` lists what the host kept, `unverified` lists what it took but that
cannot be read back, and `errors` lists what it refused or reported with a
different value. **A single entry in `errors` fails the whole call** — no tool
in this skill reports a change the host did not confirm. A property this host
does not expose at all is an error, not an omission.

`visible` maps to the layer's `on` property and `color` to its wire color; the
rest keep their 3ds Max names in snake_case. `motion_blur` is one of `none`,
`object`, or `image`.
