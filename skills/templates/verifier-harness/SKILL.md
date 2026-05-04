---
name: my-asset-verifier
description: "Template for a DCC verifier skill. Imports a previously-exported asset and reports structural statistics against the dcc-mcp-core SceneStats contract."
license: MIT
compatibility: Python 3.7+
metadata:
  category: diagnostics
  author: your-name
  dcc-mcp:
    dcc: blender
    version: "1.0.0"
    tags: "verifier, round-trip, contract, example"
    search-hint: "verify, import, inspect, fbx, round-trip, scene-stats"
    contract: scene-stats
    tools: tools.yaml
---

# my-asset-verifier

A skill **template** for implementing a cross-DCC asset verifier. The
tool `import_and_inspect` consumes a file produced by another DCC's
export pipeline and reports a `SceneStats`-shaped payload so upstream
callers can assert round-trip integrity.

## Why this template exists

`dcc-mcp-core` defines the abstract `SceneStats` contract
(`dcc_mcp_core.SceneStats`) but deliberately ships **no** DCC-specific
verifier. Each downstream repository (`dcc-mcp-blender`, `dcc-mcp-maya`,
`dcc-mcp-unreal`, `dcc-mcp-photoshop`, ...) is expected to copy this
template and replace `scripts/import_and_inspect.py` with the relevant
native API calls (`bpy.ops.import_scene.fbx`, `cmds.file(..., i=True)`,
`hou.hipFile.merge`, etc.).

## Contract surface

| Field             | Type      | Meaning                                                |
|-------------------|-----------|--------------------------------------------------------|
| `object_count`    | int       | Top-level objects observed after import.               |
| `vertex_count`    | int       | Total vertex count across all mesh geometry.           |
| `has_mesh`        | bool      | `True` when at least one object is a polygon mesh.     |
| `extra`           | object    | Optional DCC-specific enrichments, not compared.       |

See the full spec in
[`docs/guide/cross-dcc-verification.md`](../../../docs/guide/cross-dcc-verification.md).

## How the producer side drives it

The typical CI round-trip is:

1. Producer DCC (e.g. Blender) creates a primitive and exports an FBX.
2. This verifier skill is loaded into a second DCC process.
3. CI invokes `my-asset-verifier__import_and_inspect` with the produced
   file path.
4. The caller asserts
   `produced.matches(observed, vertex_tolerance=0.05)` — implemented by
   `SceneStats.matches()` in core.

## Local smoke-test

The template's `scripts/import_and_inspect.py` ships a stub that returns
hard-coded zeros. It exists so the skill can be loaded by
`parse_skill_md` without crashing during core contract tests. Replace
the stub before wiring this into a real DCC.
