---
name: blender-animation
description: "Blender animation — keyframes, frame ranges, curve inspection, key deletion, and baking"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: animation
    version: "1.0.0"
    tags: [blender, animation, keyframes, curves, baking, timeline, NLA, action, fcurve]
    search-hint: "keyframe, animation, frame range, action, timeline, fcurve, delete keyframes, bake animation, NLA track, NLA strip, action editor, extrapolation"
    search-aliases: [animate, set keyframe, keyframe editor, f-curve, action editor, delete animation, bake to keyframes, timeline scrubbing, NLA editor, animation layer, strip blending, cycle animation, action strip]
    recall-context:
      app_type: blender
      domain: animation
      workflow_stage: animation
      task_category: mutate
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
      - type: scene_state
        predicate: has_open_scene
    side-effects:
      modifies: true
      targets: [action, fcurve, keyframe, scene_frame]
    produces: [action, fcurve_data, keyframe_list]
    requires: []
    tools: tools.yaml
---

# blender-animation

Blender animation keyframe and timeline skill. Use it for frame ranges,
current-frame changes, inserting keys, inspecting f-curves, deleting keyframes,
and baking transform samples. Prefer `blender-rigging` for rig construction and
`blender-pose-library` for reusable armature poses.

## NLA: arranging actions over time

The keyframe tools work on one action at a time. The NLA tools arrange several
actions on a timeline: an NLA **track** is a layer, and a **strip** places an
action on that layer at a frame.

| Tool | Description |
|---|---|
| `list_animation_actions` | List actions in the file, or one object's active and NLA actions |
| `list_nla_tracks` | List an object's tracks and the strips each one holds |
| `add_nla_track` / `remove_nla_track` | Create or remove a track (removing a track drops its strips) |
| `add_nla_strip` | Place an existing action on a track at a frame |
| `set_nla_strip` | Move, trim, scale, repeat, blend, mute, or set influence on a strip |
| `remove_nla_strip` | Remove one strip |
| `list_action_fcurves` | List the fcurves in an action with their key ranges |
| `set_action_fcurve_extrapolation` | Control behaviour before the first and after the last key |

Notes that matter in practice:

- `add_nla_strip` needs an action that already exists; it does not create one.
  Use `set_keyframe` or `bake_animation` to produce an action first.
- A strip named for a missing action fails, and nothing is created. Pass
  `track_name` to target a specific track; without it the first track is used,
  and one is created if the object has none.
- `blend_type` (`REPLACE` / `ADD` / `SUBTRACT` / `MULTIPLY`) decides how a strip
  combines with the tracks below it; `influence` scales the contribution.
- Extrapolation on both strips and fcurves controls the gaps: `HOLD` repeats the
  end value, `NOTHING` falls back to the underlying value.
- `set_nla_strip` rejects out-of-range `influence`, `scale`, and `repeat` rather
  than clamping: Blender would silently clamp them, and reporting success while
  giving a different value is worse than refusing.
- Removing a track or strip is destructive and cannot be undone.

**Blender 5.x has no `Action.fcurves`.** It was replaced by layered animation
(`action.layers[].strips[].channelbags[].fcurves`). `list_action_fcurves` and
`set_action_fcurve_extrapolation` return an explicit "fcurves unavailable" error
on 5.x instead of reporting an action with no curves. The NLA track and strip
tools are unaffected and work across every supported version.
