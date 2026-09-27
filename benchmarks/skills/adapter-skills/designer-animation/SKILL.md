---
name: designer-animation
description: >-
  Host animation skill - discover time-like Designer inputs and bake ordered frames
  by sweeping one of them across a bounded range.
license: MIT
compatibility: "Substance 3D Designer 12.1+; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_designer
    version: "0.8.1" # x-release-please-version
    layer: domain
    stage: authoring
    search-hint: "substance designer animation frames time sweep sequence bake timeline"
    tags: "substance, designer, animation, frames, time, sequence, keyframe"
    tools: tools.yaml
---

# Designer Animation

## Honest capability boundary

Substance 3D Designer has **no timeline track, no keyframes, and no animation
curve editor** exposed through its Python SDK. Nothing here sets a keyframe,
edits a curve, or plays back a clip.

Designer graphs can still produce animated output because a graph is a
deterministic function of its inputs: change a time-like input and the graph
evaluates differently. This skill uses that:

- `list_animation_parameters` reports candidate time-like inputs by name
  convention (containing `time`, `frame`, `phase`, `speed`, `duration`,
  `animation`, or `offset`), at both graph level and node level.
- `bake_animation_frames` sweeps one of those parameters across a bounded range
  and exports one header-verified texture per step as an ordered frame set.

Frame rate and playback are **downstream choices**. This skill emits ordered
frames plus the swept values, not a video file and not a timed clip.

## Discovery before baking

`list_animation_parameters` is read-only and reports, for each candidate, its
current value plus `read_only` and `connected` flags. A candidate that is
read-only or already connected cannot be driven by `bake_animation_frames`
without rewiring, so confirm the target before baking.

The result is explicitly marked as **name-matched discovery, not a verified
animation binding**. Designer has no authoritative animation parameter to read.

## Bounded and reversible

- Series length is bounded to 64 steps and values to ±1,000,000.
- The output directory must be fresh, so a stale file can never be reported as a
  new result.
- The swept parameter is restored to its original value if any step fails.
- Every exported file is header-verified against the SDK-reported dimensions.

## Related

`designer-particles` uses the same engine for seed-variation tiles and
`designer-dynamics` for integer iteration passes.
