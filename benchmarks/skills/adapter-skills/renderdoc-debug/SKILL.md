---
name: renderdoc-debug
description: >-
  Domain skill — Debug one RenderDoc capture at pixel and thread granularity: pick the draw that
  wrote a pixel, walk that pixel's full modification history, step pixel, vertex, and compute
  shaders, and export post-VS mesh data as OBJ or JSON. Use when a capture replays but the
  question is why one pixel, vertex, or thread has the value it has. Not for capture or conversion
  — use renderdoc-capture. Not for listing actions, resources, or pipeline state — use
  renderdoc-inspect.
license: MIT
compatibility: "RenderDoc 1.45+; dcc-mcp-core 0.20.14+; qrenderdoc beside renderdoccmd"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: renderdoc
    layer: domain
    version: "0.1.0"
    search-hint: "RenderDoc debug pixel history pick pixel debug vertex debug thread post-VS mesh export shader stepping"
    tags: "renderdoc,debug,pixel-history,pixel-pick,shader-debug,post-vs,graphics-debugging"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# RenderDoc Debug

These tools answer *why* a capture produced the value it did. They replay an existing `.rdc`
through RenderDoc's replay API and never modify it; only `export_mesh` writes to disk, and only
to a destination you name.

Every tool in this skill needs the **deep replay backend** (`renderdoc.pyd`, reached through the
Python runtime bundled with `qrenderdoc`). The headless `renderdoccmd` baseline cannot read
anything back out of a capture, so it cannot substitute here.

## Capability gating

Call `debug_capabilities` first. It reports:

- whether the deep backend is reachable, and how to enable it when it is not;
- when given `capture_file`, RenderDoc's own per-capture replay flags — `pixel_history`,
  `shader_debugging`, `post_vs_data` — which vary per capture and per driver; `post_vs_data`
  is derived from the replay mode when RenderDoc does not advertise the flag itself;
- per tool, whether that tool is usable right now and which flag it depends on.

When the deep backend is missing, every tool returns a structured
`unsupported_backend` result naming the missing backend and how to enable it. None of them raise,
and none of them return empty data silently.

| Tool | Needs | Answers |
| --- | --- | --- |
| `pick_pixel` | `pixel_history` | the last draw that passed depth/stencil at this pixel, and its value |
| `pixel_history` | `pixel_history` | every event that touched this pixel, with pass/fail reason |
| `debug_pixel` | `shader_debugging` | step-by-step pixel shader trace |
| `debug_vertex` | `shader_debugging` | step-by-step vertex shader trace for one vertex/instance |
| `debug_thread` | `shader_debugging` | step-by-step compute shader trace for one workgroup/thread |
| `export_mesh` | `post_vs_data` | post-VS geometry as OBJ or JSON |

## Working order

1. `debug_capabilities` — confirm the backend and the per-capture flags.
2. `renderdoc_inspect__list_actions` — find the event ID you care about.
3. `pick_pixel` — confirm the last draw that passed depth/stencil at that pixel, and read the
   pixel's current contents.
4. `pixel_history` — see every write to that pixel and which one failed.
5. `debug_pixel` / `debug_vertex` / `debug_thread` — step the shader that produced the value.
6. `export_mesh` — dump post-VS geometry when the problem is geometric, not per-pixel.

`pick_pixel` attributes the pixel through `PixelHistory`, because RenderDoc's `PickPixel` returns
only a `PixelValue` — the pixel's contents, with no event identity attached. `debug_vertex` takes
exactly four selectors (`vertex_index`, `instance`, `index`, `view`); fold the draw's vertex and
instance offsets into them yourself, using the offsets `renderdoc_inspect__get_action` reports.

Debug traces can be long. Start with `detail: summary` to learn the step count, then ask for
`detail: trace` with a `max_steps` you can actually read.

Replay runs in the Python interpreter bundled with `qrenderdoc`. On headless Linux, run the
adapter under Xvfb or provide another working X/Wayland display; the official archive does not
include Qt's `offscreen` platform plugin.
