---
name: blender-expressions
description: "Blender driver and expression system — add/edit/remove drivers, manage driver variables, evaluate expressions"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: animation
    version: "1.0.0"
    tags: [blender, driver, expression, animation, rigging, constraints]
    search-hint: "driver, expression, fcurve driver, scripted expression, driver variable, evaluate driver, add driver, remove driver"
    search-aliases: [animation driver, driven property, expression controller, driver namespace, driver variable target, RNA driver]
    intent: "Add, edit, and remove Blender drivers; manage driver variables; evaluate expression results. Equivalent to Maya expression editor and connection editor."
    recall-context:
      app_type: blender
      domain: animation
      workflow_stage: rigging
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
      targets: [driver, driver_variable, fcurve, animation_data]
    produces: [driver_info, driver_list, evaluated_value]
    requires: []
    tools: tools.yaml
---

# blender-expressions

Blender driver and expression skill. Use it to:

- **Add drivers** to any animatable property (`add_driver`) with types
  `SCRIPTED`, `AVERAGE`, `SUM`, `MIN`, `MAX`.
- **Edit driver expressions** without replacing the whole driver
  (`set_driver_expression`).
- **Manage driver variables** — add (`add_driver_variable`) and remove
  (`remove_driver_variable`) the named variables a scripted expression can
  reference.
- **Inspect all drivers** on an object or scene-wide (`list_drivers`).
- **Evaluate** the current driven value at the scene frame
  (`evaluate_driver_expression`).
- **Remove drivers** cleanly (`remove_driver`).

This skill maps roughly to Maya's *Expression Editor* and *Set Driven Key*
workflow. Use `blender-animation` for plain keyframes and `blender-rigging` for
bone/constraint setups.
