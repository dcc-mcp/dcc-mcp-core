---
name: designer-dynamics
description: >-
  Host dynamics skill - sweep an integer iteration-count parameter in a Substance 3D
  Designer graph and bake each pass as a separate verified texture.
license: MIT
compatibility: "Substance 3D Designer 12.1+; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_designer
    version: "0.8.1" # x-release-please-version
    layer: domain
    stage: authoring
    search-hint: "substance designer dynamics iteration passes erosion sweep procedural accumulation"
    tags: "substance, designer, dynamics, iteration, passes, erosion, procedural"
    tools: tools.yaml
---

# Designer Dynamics

## Honest capability boundary

**Substance 3D Designer has no dynamics solver.** There is no rigid body, cloth,
fluid, or granular simulation, and no timestep integration. Nothing here
simulates physics.

What Designer provides is a deterministic graph that can be re-evaluated after a
typed input changes. Many Designer filters expose an **iteration count** (erosion,
flow, distance, blend passes) that behaves like an accumulation control: raising
it deepens the effect. `bake_iteration_passes` sweeps that integer parameter and
exports one header-verified texture per value.

Read the result as **a progression of deterministic accumulations**, not as a
simulated timestep. Pass 8 is not "8 seconds of simulation"; it is the graph
evaluated with that node's iteration count set to 8.

## When this is the right tool

Use it to choose an iteration count, to build a weathering progression, or to
export an ordered set of accumulation stages for a downstream lookdev or
shot-assembly step. If you need motion over time rather than accumulation depth,
use `designer-animation`.

## Bounded and reversible

- Values are coerced to `int`, because iteration counts are integers.
- Series length is bounded to 64 steps and values to ±1,000,000.
- The output directory must be fresh, so a stale file can never be reported as a
  new result.
- The swept parameter is restored to its original value if any step fails.
- Every exported file is header-verified against the SDK-reported dimensions.

## Related

`designer-particles` uses the same engine for seed-variation tiles and
`designer-animation` for ordered frames.
