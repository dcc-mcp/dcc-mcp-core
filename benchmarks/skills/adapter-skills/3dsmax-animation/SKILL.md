---
name: 3dsmax-animation
description: >-
  Domain skill - inspect timeline settings, batch and verify transform keys,
  exchange anim-curves v1 data, bake simple animation curves, and control
  viewport playback in 3ds Max.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max animation batch keyframes values tangents playback timeline transform controllers bake curve import export"
    tags: "3dsmax, animation, keyframe, timeline, curves"
    tools: tools.yaml
    intent: "Inspect and manipulate 3ds Max animation keyframes, timeline, and playback controls."
    search_aliases: ["animation", "animation"]
    recall_context:
      app_type: "3dsmax"
      domain: "animation"
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
      exports: true
      imports: true
      file_output: true
      render: false
      targets: ["scene_node", "keyframe", "timeline", "file:animation_curve"]
    produces: ["keyframe_data", "animation_curve", "timeline_state"]
---

# 3ds Max Animation Tools

Inspect timeline state, set bounded transform-key batches, read values and
tangents, exchange `dcc-mcp/anim-curves@1` payloads, bake transform samples,
and start viewport playback. All tools touch the live scene through `pymxs`,
so they declare `affinity: main`.

Tool contracts live in `tools.yaml`. Mutating tools require explicit targets or
explicit `use_selection=true`, validate the complete request before mutation,
and return host-readback evidence with changed-key counts where applicable.

The anim-curves schema remains a thin adapter contract until the Core
verification package tracked by `dcc-mcp-core#2261` publishes the shared owner.
The legacy `export_animation_curves` / `import_animation_curves` tools remain
available for backward compatibility.
