---
name: unreal-cinematics
description: >-
  Domain skill — Sequencer shot and keyframe orchestration, camera cut tracks,
  and Movie Render Queue job setup. Use when creating or editing Level
  Sequences, authoring camera animation, or preparing an MRQ job in Unreal Engine 5.
  Not for Blueprint scripting (unreal-blueprints) or runtime playback
  (unreal-runtime).
license: MIT
compatibility: Unreal Engine 5.0+, Python 3.9+, UE 5.8+ for official MCP integration
allowed-tools: Read Bash Write
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: domain
    stage: authoring
    search-hint: "sequencer cinematic camera cut track keyframe movie render level sequence shot"
    tags: [unreal, sequencer, cinematic, camera, keyframe, movie, render, sequence]
    tools: tools.yaml
---

# unreal-cinematics (Authoring stage)

Create and edit Level Sequences, add camera cuts and actor bindings,
author transform keyframes, control playback range, and prepare cinematic jobs
for the Movie Render Queue. Starting a render and verifying output are outside
this skill's current contract.

Movie Render Queue tools require the `MovieRenderPipeline` plugin and fail
closed before queue, asset, console-command, or executor mutation when it is
not enabled. Use `unreal_automation__preflight_plugins` with
`capability=movie_render_queue` for a read-only readiness check.

## Workflow

1. Call `create_level_sequence` to create a new sequence asset.
2. Use `add_actor_to_sequence` to bind actors as sequence tracks.
3. Author keyframes with `add_transform_keyframe`.
4. Add camera cuts via `add_camera_cut_track` for shot switching.
5. Set playback ranges with `set_playback_range`.
6. Inspect sequence state with `get_sequence_info`.
7. Add an MRQ job with `queue_sequence_render`, start it with `start_queued_render`, poll `get_render_status`, and use `cancel_queued_render` when the output is rejected.

When UE 5.8+ official MCP is available, the `unreal-official-mcp` skill
provides additional Sequencer tools through Epic's toolset registry.
Prefer the official path for complex shot workflows.

## Scripts

- `create_level_sequence` — Create a new Level Sequence asset in the Content Browser
- `open_level_sequence` — Open a Level Sequence in the Sequencer editor
- `add_actor_to_sequence` — Bind an actor as a track in a Level Sequence
- `add_transform_keyframe` — Add location/rotation/scale keyframes at a time
- `set_playback_range` — Set the sequence start and end frame
- `get_sequence_info` — Inspect tracks, bindings, and playback range
- `add_camera_cut_track` — Add a camera cut track for shot switching
- `queue_sequence_render` — Configure an active-session MRQ job, optionally with an OCIO display transform, without starting a render
- `start_queued_render` — Start the active MRQ queue
- `get_render_status` — Inspect active MRQ state
- `cancel_queued_render` — Cancel the active MRQ executor and skip remaining jobs
