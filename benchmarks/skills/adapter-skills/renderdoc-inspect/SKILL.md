---
name: renderdoc-inspect
description: >-
  Domain skill — Replay an existing RenderDoc capture through RenderDoc's official replay API and
  inspect its actions, resources, pipeline state, shaders, constant buffers, texture, and buffer
  data. Use for structured inspection of a .rdc you already have. Not for launching a capture — use
  renderdoc-capture. Not for chunk-level offline triage without replay — use renderdoc-analysis.
license: MIT
compatibility: "RenderDoc 1.45+; dcc-mcp-core 0.20.14+; qrenderdoc beside renderdoccmd"
allowed-tools: "python"
metadata:
  dcc-mcp:
    dcc: renderdoc
    layer: domain
    version: "0.1.0"
    search-hint: "RenderDoc inspect draw actions resources pipeline state shader constant buffer texture buffer replay"
    tags: "renderdoc,replay,inspect,pipeline-state,shader,constant-buffer,graphics-debugging"
    tools: tools.yaml
    depends: "dcc-diagnostics"
---

# RenderDoc Inspect

These tools replay an existing `.rdc` through RenderDoc's own replay API, so they expose what the
RenderDoc UI can show rather than only what `renderdoccmd` can serialise. Every tool takes
`capture_file` and never modifies it; only `get_texture_data` and `get_buffer_data` write to disk,
and only to a destination you name.

## Backends and capability gating

Two backends provide RenderDoc capabilities:

- **Baseline** — `renderdoccmd`, headless and always preferred for capture, conversion, thumbnails,
  and the remote server. It can only *replay* a capture, never read data out of one.
- **Deep replay** — `renderdoc.pyd`, which carries actions, resources, pipeline state, shaders, and
  readback. It is not importable by the adapter's own interpreter, so this skill reaches it through
  the Python interpreter bundled with `qrenderdoc`; `qrenderdoc` must therefore sit beside
  `renderdoccmd`.

`replay_capabilities` reports which backends are reachable and which capability groups they unlock
(`inspect`, `debug`, `perf`, `ext`). When the deep backend is missing, the deep tools fail with one
explicit message naming the missing backend and how to enable it, instead of crashing.

## Working order

1. `replay_capabilities` — confirm the deep backend is reachable.
2. `describe_capture` — capture size, resource counts, and per-capture replay flags.
3. `list_actions` — find an event ID; `name_filter` and `flag_filter` do the searching.
4. `get_action` / `get_pipeline_state` — what one event is and what it bound.
5. `get_shader_info` — reflection, source, disassembly, and constant buffer values.
6. `get_resource_usage`, `get_texture_data`, `get_buffer_data` — the data behind a resource.

Replay runs in the Python interpreter bundled with `qrenderdoc`. On headless Linux, run the adapter
under Xvfb or provide another working X/Wayland display; the official archive does not include Qt's
`offscreen` platform plugin.

Large results are paged with `offset` and `limit`; keep `limit` small enough to read.
