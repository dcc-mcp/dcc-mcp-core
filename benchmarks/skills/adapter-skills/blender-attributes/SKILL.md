---
name: blender-attributes
description: "Blender custom property CRUD — get, set, list, delete, and rename custom attributes on objects"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: authoring
    version: "1.0.0"
    tags: [blender, attributes, custom-properties, object-data]
    search-hint: "custom property, attribute, object data, custom attribute, id property, metadata"
    search-aliases: [custom properties, object attributes, property management, attribute editor, object metadata, custom data]
    intent: "Read, create, update, delete, and rename custom properties on Blender objects, matching Maya's attribute editor surface."
    recall-context:
      app_type: blender
      domain: authoring
      workflow_stage: authoring
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
      targets: [object, id_property]
    produces: [attribute_data]
    requires: []
    tools: tools.yaml
---

# blender-attributes

Typed tools for Blender custom property CRUD — listing, reading, writing,
deleting, and renaming custom properties (ID properties) on objects. Mirror
the Maya `maya-attributes` skill surface for Blender.

## Tools

| Tool | Description |
|---|---|
| `list_attributes` | List all custom properties on an object |
| `get_attribute` | Read a single custom property value |
| `set_attribute` | Create or update a custom property with optional UI metadata |
| `delete_attribute` | Remove a custom property |
| `rename_attribute` | Rename an existing custom property |

## Common workflows

- **Inspect object metadata**: `list_attributes` -> `get_attribute`
- **Tag objects with pipeline data**: `set_attribute` with pipeline metadata
- **Clean up old properties**: `list_attributes` -> `delete_attribute`
