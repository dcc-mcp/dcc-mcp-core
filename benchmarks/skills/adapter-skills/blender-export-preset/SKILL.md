---
name: blender-export-preset
description: >-
  Blender export preset skill for saving, loading, listing, and deleting
  reusable scene-stored export options for interchange workflows.
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: interchange
    version: "1.0.0"
    tags: [blender, export, preset, interchange, pipeline]
    search-hint: >-
      export preset, save export options, list export presets, load export preset,
      delete export preset, repeatable export settings
    search-aliases: [export template, export profile, reuse export settings, saved export config, persistent export options]
    intent: "Manage reusable, scene-stored export presets — save, list, load, and delete named export configurations for consistent batch interchange."
    recall-context:
      app_type: blender
      domain: io
      workflow_stage: interchange
      task_category: mutate
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
      - type: scene_state
        predicate: has_open_scene
    side-effects:
      modifies: true
      targets: [scene_metadata]
    produces: [export_preset]
    requires: []
    tools: tools.yaml
---

# blender-export-preset

Scene-stored export preset management for repeatable interchange settings.
Presets are saved in Blender scene custom properties and can be reused by
`blender-interchange` batch exports.
