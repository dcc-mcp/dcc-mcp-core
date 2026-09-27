---
name: houdini-animation
description: >-
  Pipeline skill — typed timeline, keyframe, channel, and cache-baking tools.
  Query/set time, FPS and ranges; set/get/delete/list keyframes; inspect
  channels and expressions; validate bounded transform-loop continuity;
  export/import channel JSON; bake channels and trigger bounded
  simulation/cache renders. The canonical timeline API.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 20.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [houdini, animation, keyframe, channel, timeline, loop, seam, validation, fps, bake, simulation, cache, pipeline]
    search-hint: "keyframe channel timeline frame range fps animation loop seam continuity validate bake simulation cache expression"
    tools: tools.yaml
---

# houdini-animation

Typed animation tools for agents. All tools are `affinity: main`.

## Tool groups

- **`timeline`:** `get_timeline`, `set_timeline` — the **canonical** timeline
  query/setter. For interactive timeline edits prefer `set_timeline` over
  `houdini_automation__set_frame_range` (which remains for file-based batch
  flows).
- **`keyframes`:** `set_keyframe`, `get_keyframes`, `delete_keyframes`
  (destructive).
- **`channels`:** `list_animated_parms`, `get_channel_info`, `export_channels`,
  `import_channels` (JSON round-trip).
- **`validation`:** `validate_loop_contract` — bounded, read-only OBJ world-
  transform continuity checks with driver summaries. It never inspects or cooks
  geometry and restores the original frame in `finally`.
- **`bake`:** `bake_channels` (async per-frame sample → constant keys, hard
  frame cap), `cache_simulation` (launch an isolated filecache/DOP/geometry ROP
  job by default, or report `written_files` + `elapsed_secs` in explicit
  foreground mode).

## Tracer-bullet flow

1. `set_timeline(frame_range=[1,48], fps=24)`
2. `set_keyframe(node_path="/obj/geo1", parm_name="tx", frame=1, value=0)`
3. `set_keyframe(node_path="/obj/geo1", parm_name="tx", frame=48, value=10)`
4. `get_keyframes("/obj/geo1", "tx")` → verify two keys
5. `validate_loop_contract(["/obj/geo1"], 1, 48)` → verify the periodic seam
6. `export_channels("/obj/geo1", ["tx"], "/tmp/tx.json")` → `import_channels` to retarget
7. `bake_channels("/obj/geo1", ["tx"], frame_range=[1,48])` to flatten expressions

For numeric keys, `set_keyframe` accepts optional `interpolation`: `bezier`,
`linear`, or `constant`. It controls the segment starting at that key; omit it
to preserve Houdini's default. Use `constant` for discrete one-frame switches.

## Loop range contract

`validate_loop_contract` treats `[start_frame, end_frame]` as **N unique
playback samples**. For the default step of one frame it temporarily samples
`start`, `start+1`, `start+2`, `end-1`, `end`, and the virtual continuation
`end+1`.

- `endpoint_duplicate_delta` reports `end` vs `start` for diagnosing assets
  authored with a duplicated endpoint; it is informational and does not decide
  pass/fail.
- `duplicate_endpoint_hold_risk` becomes true when that duplicated endpoint is
  within transform tolerances but the first linear or angular step is
  nontrivial. This diagnostic stays false for static nodes and is not a gate.
- `periodic_delta` compares virtual `end+1` vs `start` and drives positional,
  angular, scale, and matrix continuity checks.
- Velocity residuals compare the `end → end+1` step with `start → start+1`;
  acceleration residuals compare the neighboring three-sample stencils.

This avoids the one-frame hold caused by requiring `end == start` in a range
whose endpoints are both played. Supply `tolerances` to override the documented
defaults; requests remain bounded to 64 unique `/obj` paths and six transform
samples per resolved node.

`cache_simulation` uses synchronous dispatch metadata so its adapter-owned job
identity is returned directly. UI and headless Houdini default to isolated
`hython`; poll its `job_id` with
`houdini_render__get_render_job` and cancel it with
`houdini_render__cancel_render_job`. Pass `background=false` for intentional
foreground execution. Interactive background launch requires an explicitly
saved, clean HIP and never auto-saves the GUI scene. Headless isolation
requires an existing HIP and captures its current state in a job-owned
temporary snapshot without saving the source scene. Output-path parm names are
probed defensively so File Cache, DOP I/O, and Geometry ROPs are all tolerated.
