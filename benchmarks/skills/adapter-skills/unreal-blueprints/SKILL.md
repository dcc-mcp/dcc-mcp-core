---
name: unreal-blueprints
description: >-
  Domain skill - Unreal Engine Blueprint editing: create Blueprint classes,
  add event/function nodes, connect nodes, add variables, and compile.
  Use when building or modifying Blueprint logic in the Unreal Editor.
  Not for level actor placement - use unreal-actors for that.
license: MIT
compatibility: Unreal Engine 5.0+, Python 3.9+
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: domain
    stage: authoring
    search-hint: "unreal blueprint node graph event function variable compile connect"
    tags: "unreal, blueprints, nodes, graph, scripting"
    tools: tools.yaml
---

# Unreal Blueprints

Tools for Blueprint class creation and node graph editing in the Unreal Editor.

Patterns derived from [chongdashu/unreal-mcp](https://github.com/chongdashu/unreal-mcp)
Blueprint node graph editing abstractions.

## Scripts

- `create_blueprint_class` - Create a new Blueprint class from a parent class.
- `add_event_node` - Add an event node (BeginPlay, Tick, etc.) to a Blueprint graph.
- `add_function_node` - Add a function call node to a Blueprint graph.
- `connect_nodes` - Connect two nodes in a Blueprint graph.
- `layout_blueprint_graph` - Automatically arrange graph nodes by connection depth.
- `add_variable` - Add a member variable to a Blueprint.
- `compile_blueprint` - Compile a Blueprint.
- `find_nodes` - Find nodes in a Blueprint graph by type or event.
- `add_component_to_blueprint` - Add a component to a Blueprint.

## Usage Examples

### Create a Blueprint and add BeginPlay logic

```python
# 1. Create the Blueprint
# MCP tool call: unreal_blueprints__create_blueprint_class
# params: {"blueprint_name": "BP_MyActor", "parent_class": "Actor"}

# 2. Add a StaticMesh component
# MCP tool call: unreal_blueprints__add_component_to_blueprint
# params: {"blueprint_name": "BP_MyActor", "component_type": "StaticMeshComponent", "component_name": "Mesh"}

# 3. Add BeginPlay event
# MCP tool call: unreal_blueprints__add_event_node
# params: {"blueprint_name": "BP_MyActor", "event_name": "ReceiveBeginPlay"}

# 4. Compile
# MCP tool call: unreal_blueprints__compile_blueprint
# params: {"blueprint_name": "BP_MyActor"}
```

Nodes created without an explicit `node_position` are laid out automatically.
`connect_nodes` also reapplies the graph layout after wiring nodes together.

### Connect nodes in a graph

```python
# MCP tool call: unreal_blueprints__connect_nodes
# params: {
#   "blueprint_name": "BP_MyActor",
#   "source_node_id": "node_123",
#   "source_pin": "then",
#   "target_node_id": "node_456",
#   "target_pin": "execute"
# }
```
