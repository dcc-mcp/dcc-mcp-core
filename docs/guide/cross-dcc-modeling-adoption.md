# Cross-DCC Modeling Adoption Matrix

This matrix tracks adoption of the versioned `dcc-mcp/model-state@1` contract
across the five adapters named by issue #2913. The contract defines the
canonical checks, workflow stages, and verdict values. Adapter repositories own
the host-specific implementation and evidence.

## Status rules

Use `pass` only when the adapter has linked evidence for the reported surface.
Use `fail` when a checked surface is implemented and the evidence shows a
defect. Use `unknown` when the adapter has not published enough evidence to
make either claim. A missing host observation must remain `unknown` and cannot
be promoted to `pass` by aggregation.

The machine-readable source is
[`cross-dcc-modeling-adoption.json`](cross-dcc-modeling-adoption.json). CI
checks its shape and rejects any adapter row that reports `pass` without an
evidence link.

## Canonical workflow

1. Resolve the modeling entry skill from intent-first metadata.
2. Inspect the current scene and reference evidence.
3. Apply bounded modeling operations.
4. Read back geometry, topology, editability, and hierarchy state.
5. Verify fixed-view evidence and return `pass`, `fail`, or `unknown`.

The canonical checks are `parts`, `hierarchy`, `materials`, `uv_coverage`,
`non_manifold`, and `euler`. Adapters may report unsupported checks through the
state document's `unavailable` field.

## Current matrix

| DCC | Typed coverage | Readback coverage | Reference fit | Real-DCC smoke | Implementation issue | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Blender | unknown | unknown | unknown | unknown | [#371](https://github.com/dcc-mcp/dcc-mcp-blender/issues/371) | pending downstream evidence |
| Houdini | unknown | unknown | unknown | unknown | [#384](https://github.com/dcc-mcp/dcc-mcp-houdini/issues/384) | pending downstream evidence |
| Maya | unknown | unknown | unknown | unknown | [#612](https://github.com/dcc-mcp/dcc-mcp-maya/issues/612) | pending downstream evidence |
| 3ds Max | unknown | unknown | unknown | unknown | [#242](https://github.com/dcc-mcp/dcc-mcp-3dsmax/issues/242) | pending downstream evidence |
| ZBrush | unknown | unknown | unknown | unknown | [#115](https://github.com/dcc-mcp/dcc-mcp-zbrush/issues/115) | pending downstream evidence |

The matrix records the current evidence boundary. It does not infer coverage
from a skill name, a catalog entry, or a successful process start. Downstream
adoption issues should add evidence links as each surface becomes verifiable.
