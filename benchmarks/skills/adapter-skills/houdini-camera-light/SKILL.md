---
name: houdini-camera-light
description: >-
  Authoring skill — create and edit cameras and lights, frame the Scene Viewer,
  and report active camera/view state through typed tools. Pair with
  houdini-render for viewport capture and ROP render execution.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 20.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: authoring
    version: "1.0.0"
    tags: [houdini, camera, light, hlight, viewport, view, lookthrough, authoring]
    search-hint: "create camera light hlight intensity color exposure focal resolution frame view look through"
    tools: tools.yaml
---

# houdini-camera-light

Typed camera and light authoring for agents. All tools are `affinity: main`.
View tools (`frame_view`, `get_view_state`) are UI-aware: in a headless
`hython` session they return a structured `warnings` payload with
`framed: false` / `ui_available: false` instead of failing.
When both a node and camera are supplied, `frame_view` frames the node first
and activates the camera last so the returned `active_camera` is authoritative.
To protect the Houdini UI from driver-level crashes, `frame_view` refuses to
draw geometry above one million primitives by default and reports counts plus
`heavy_geometry_refused: true`. Generate a viewport proxy whenever possible;
`allow_heavy_geometry=true` is an explicit, auditable override.

## Tool groups

- **`camera`:** `list_cameras`, `create_camera`, `update_camera`.
- **`view`:** `frame_view`, `get_view_state`.
- **`light`:** `create_light`, `update_light`.

## Tracer-bullet flow

1. `create_camera(parent_path="/obj", name="shotcam", focal=50, resolution=[1920,1080])`
2. `create_light(light_type="distant", intensity=2.0, color=[1,0.95,0.9])`
3. `frame_view(camera_path="/obj/shotcam", node_path="/obj/geo1")`
4. `get_view_state()` → confirm `active_camera`
5. hand off to `houdini-render` (`capture_viewport` / `render`)

Light/camera parameter writes are defensive (set only when the parm exists), so
they tolerate `hlight::2.0` vs older `hlight` and Karma/Mantra camera variants.
