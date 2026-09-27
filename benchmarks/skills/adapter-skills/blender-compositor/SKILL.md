---
name: blender-compositor
description: "Blender compositing setup and node graph editing — enable the compositor, build trees, add nodes, connect sockets, and set values"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: render
    version: "1.0.0"
    tags: [blender, compositor, compositing, node-graph, nodes, post-processing, denoise, cryptomatte]
    search-hint: "compositor, compositing, node tree, render layers, composite node, denoise, glare, cryptomatte, file output, post process"
    search-aliases: [compositing, compositor node, compositor tree, enable compositor, connect compositor nodes, post processing, render layers, viewer node, glare, denoise, cryptomatte, z combine]
    intent: "Enable and author Blender compositor node graphs — create trees, add and delete nodes, connect or disconnect sockets, and set socket values."
    recall-context:
      app_type: blender
      domain: authoring
      workflow_stage: render
      task_category: mutate
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
      - type: scene_state
        predicate: has_open_scene
    side-effects:
      modifies: true
      creates: true
      targets: [compositor_node_tree, compositor_node, node_link]
    produces: [node_list, socket_info, link_list, node_type_catalog]
    requires: ["blender-node-graph"]
    tools: tools.yaml
---

# blender-compositor

Typed tools for authoring Blender's compositor. `blender-node-graph` stays the
read-only discovery surface (`list_all_node_graphs`, `list_compositor_nodes`,
`get_compositor_node_tree`); this skill adds the write half — enabling the
compositor, building a starter tree, and creating, wiring, and tuning nodes.

Node types are addressed by Blender id, UI label, or alias
(`CompositorNodeDenoise`, `Denoise`, and `denoise` all resolve to the same
node). Call `list_compositor_node_types` when unsure.

## Tools

| Tool | Description |
|---|---|
| `setup_compositor_tree` | Enable `scene.use_nodes` and optionally build a Render Layers -> Composite tree |
| `set_compositor_enabled` | Toggle `scene.use_nodes` without touching the graph |
| `clear_compositor_tree` | Remove every node and link from the compositor tree |
| `create_compositor_node` | Create a compositor node (id, label, or alias) |
| `delete_compositor_node` | Delete a compositor node by name |
| `connect_compositor_nodes` | Connect an output socket to an input socket |
| `disconnect_compositor_nodes` | Disconnect links by id or endpoints |
| `set_compositor_node_value` | Set an input socket default value |
| `get_compositor_node_value` | Read one socket value or all socket values |
| `list_compositor_node_links` | List every link in the compositor tree |
| `list_compositor_node_types` | Discover supported compositor node ids, labels, and aliases |

## Common workflows

- **Enable compositing**: `setup_compositor_tree` -> `list_compositor_nodes`
- **Denoise pass**: `setup_compositor_tree` -> `create_compositor_node(denoise)` -> `connect_compositor_nodes`
- **Cryptomatte matte**: `create_compositor_node(cryptomatte)` -> `connect_compositor_nodes` -> `set_compositor_node_value`
- **Tune a filter**: `create_compositor_node(glare)` -> `set_compositor_node_value` -> `list_compositor_node_links`

Every tool accepts an optional `scene_name`; the active scene is used when it is omitted.
