# Cross-DCC Verification

> Prove an asset produced by one DCC through its MCP server can be
> consumed, inspected, and validated by a second DCC. This page explains
> the **contract** `dcc-mcp-core` ships and how downstream DCC
> repositories plug into it.

## Why a contract, not an implementation

Every DCC (Blender, Maya, Unreal, Photoshop, Houdini, ...) has its own
native import API, its own scene traversal primitives, and its own idea
of what counts as a "mesh". `dcc-mcp-core` stays out of that jungle on
purpose: it only defines the **shape** of a verifier's result and the
**skill template** every downstream implementation is expected to
clone. Keeping the shape tiny (three fields) is what makes the
contract portable.

The actual `import_and_inspect` logic lives in the downstream repos:

| DCC | Repository | Skill name (convention) |
|-----|------------|-------------------------|
| Blender | `dcc-mcp-blender` | `blender-fbx-verifier` |
| Maya | `dcc-mcp-maya` | `maya-fbx-verifier` |
| Unreal | `dcc-mcp-unreal` | `unreal-fbx-verifier` |
| Photoshop | `dcc-mcp-photoshop` | `photoshop-psd-verifier` |

## The `SceneStats` contract

```python
from dcc_mcp_core import SceneStats

observed = SceneStats(
    object_count=1,
    vertex_count=482,
    has_mesh=True,
    extra={"dcc": "blender-3.6"},  # optional DCC-specific enrichments
)
```

| Field | Type | Meaning |
|-------|------|---------|
| `object_count` | `int` | Top-level objects seen after import. |
| `vertex_count` | `int` | Total vertex count across all mesh geometry. |
| `has_mesh` | `bool` | True when any imported object is polygon geometry. |
| `extra` | `dict` | Free-form enrichments. Survives serialisation, ignored by the core comparison. |

### Comparing producer vs verifier

The helper `SceneStats.matches()` implements the only comparison
semantics core endorses:

- **Strict** on `object_count` (structural invariant).
- **Strict** on `has_mesh` (detects silent empty imports).
- **Fuzzy** on `vertex_count` (±5% by default) because FBX normals,
  UV seams, and tangent basis changes can split vertices on re-import.

```python
produced = SceneStats(object_count=1, vertex_count=482, has_mesh=True)
observed = verifier_skill__import_and_inspect("/tmp/sphere.fbx")

assert produced.matches(observed, vertex_tolerance=0.05), (
    f"round-trip drift: expected {produced}, got {observed}"
)
```

Extra fields beyond the three core ones belong in `extra`. The
comparator does not read `extra`, which keeps DCC-specific telemetry
from ever breaking a round-trip assertion.

## Writing a verifier skill

The template lives at [`skills/templates/verifier-harness/`](../../skills/templates/verifier-harness/).
To create a verifier for a new DCC:

1. Copy the template directory into your downstream repo (e.g.
   `dcc-mcp-blender/skills/blender-fbx-verifier/`).
2. Edit `SKILL.md`: set `dcc: blender` (or similar) and rename the
   skill to match your repo's convention.
3. Replace the stub body of `scripts/import_and_inspect.py` with your
   DCC's native import + inspection calls. Return a
   `SceneStats.to_dict()` payload wrapped by `skill_success(...)`.
4. Add a CI job in your downstream repo that:
   - Boots two DCC processes (or one producer + one verifier).
   - Runs a producer skill to create and export an asset.
   - Runs your new verifier skill on the exported file.
   - Asserts `produced.matches(observed)` using
     `dcc_mcp_core.SceneStats`.

### Example stub (Blender)

```python
import bpy

from dcc_mcp_core import SceneStats
from dcc_mcp_core.skill import skill_entry, skill_success


def main(params):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.fbx(filepath=params["file_path"])
    objects = list(bpy.context.scene.objects)
    meshes = [o for o in objects if o.type == "MESH"]
    vertex_count = sum(len(m.data.vertices) for m in meshes)

    stats = SceneStats(
        object_count=len(objects),
        vertex_count=vertex_count,
        has_mesh=bool(meshes),
        extra={"blender_version": bpy.app.version_string},
    )
    return skill_success("Imported and inspected", **stats.to_dict())


if __name__ == "__main__":
    skill_entry(main)
```

## Where the round-trip CI lives

`dcc-mcp-core` does **not** ship a Blender-with-FBX round-trip job.
That belongs in whichever downstream repo owns the producer or verifier
binary, because only those repos pin the DCC version matrix. The
invariant core asserts is shape-only, exercised by
[`tests/test_verifier_contract.py`](../../tests/test_verifier_contract.py).

Downstream repos following this contract should add a CI job along
these lines:

```
1. Start producer DCC in headless mode, load producer skill.
2. mcporter call: producer__create_sphere, producer__export_fbx.
3. Start verifier DCC in headless mode, load verifier skill.
4. mcporter call: verifier__import_and_inspect(/tmp/sphere.fbx).
5. Python assertion: SceneStats.from_dict(...).matches(produced).
```

## FAQ

### Why only three fields? What about bounding boxes, materials, animations?

Every field that enters the core contract has to mean the same thing
across every DCC, forever. Three fields (objects / vertices /
has-mesh) is the set where we could rigorously define and test the
semantics. Anything DCC-specific (materials, cameras, animation frame
counts, bounding boxes) can travel through `extra` without committing
core to a cross-DCC definition.

### Can I extend `SceneStats` in a downstream repo?

Do not subclass. Put downstream-specific data in `extra`. If a field
becomes genuinely universal across all supported DCCs we'll promote it
to the core contract in a minor release.

### What if my DCC doesn't expose vertex counts?

Return `vertex_count=0` and set an explanatory note in `extra`. Round-
trip tests that target that DCC should then pass `has_mesh` as the
only real assertion.

## Related

- [`dcc-thread-safety.md`](dcc-thread-safety.md) — main-thread
  dispatcher primitives that verifier skills rely on when running
  inside a live DCC session.
- [`host-adapter.md`](host-adapter.md) — the `HostAdapter` base class
  downstream repos extend to plug a verifier into their DCC's idle
  loop.
- [`skills.md`](skills.md) — SKILL.md format the verifier template is
  built on.
