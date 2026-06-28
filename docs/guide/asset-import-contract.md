# Asset Import Contract

> Define the shared hand-off shape between asset source skills and DCC adapter
> importers.  This page documents the **`AssetDescriptor` → `ImportToSceneResult`**
> contract shipped by `dcc-mcp-core` and how downstream DCC repos implement each
> side of the pipeline.

## Why a contract, not an implementation

Every DCC (Blender, Maya, Unreal, Houdini, ...) has its own native import API,
its own scene model, and its own material and geometry representations.
`dcc-mcp-core` stays out of that jungle on purpose — it only defines the
**shape** of the descriptor an asset source returns and the **shape** of the
result an adapter importer must produce.  Keeping the shape purely data-driven
(frozen dataclasses + plain-dict serialisation) is what makes the contract
portable across Python runtimes, DCC versions, and host processes.

The actual `import_to_scene` logic lives in the downstream repos:

| DCC | Repository | Skill name (convention) |
|-----|------------|-------------------------|
| Blender | `dcc-mcp-blender` | `blender-import-to-scene` |
| Maya | `dcc-mcp-maya` | `maya-import-to-scene` |
| Unreal | `dcc-mcp-unreal` | `unreal-import-to-scene` |
| Houdini | `dcc-mcp-houdini` | `houdini-import-to-scene` |

## Pipeline overview

```
[Source Skill]                          [DCC Adapter Skill]
      │                                        │
      │  search_assets(query)                  │
      │  ────────────────►                     │
      │  returns AssetDescriptor[]             │
      │                                        │
      │  (variant selected,                    │
      │   descriptor built)                    │
      │                                        │
      │         import_to_scene(descriptor)    │
      │         ──────────────────────────►    │
      │                                        │
      │                  ImportToSceneResult   │
      │         ◄──────────────────────────    │
```

The source skill **produces** `AssetDescriptor` values.  The adapter skill
**consumes** one and returns `ImportToSceneResult`.  Between them, the
descriptor is the single, versioned hand-off token.

---

## Source Skill — building an `AssetDescriptor`

An asset source skill (such as `asset-source` in the bundled demo) searches a
catalog — static or live — and returns candidate assets as a list of
`AssetDescriptor` objects.

### The `AssetDescriptor` contract

```python
from dcc_mcp_core import AssetDescriptor, AssetFileVariant, AssetAttribution
from dcc_mcp_core import AssetFormat, AxisHint, UnitHint

desc = AssetDescriptor(
    asset_id="arch/city-bank/desk",
    variants=[
        AssetFileVariant(
            local_path="/data/desk.fbx",
            format=AssetFormat.FBX,
            preferred=True,
            mime="model/fbx",
        ),
    ],
    attribution=AssetAttribution(
        source_url="https://example.com/assets/city-bank-desk",
        license_spdx="CC-BY-4.0",
        author="City Bank Studio",
        title="Office Desk",
    ),
    unit_hint=UnitHint.CENTIMETER,
    meters_per_unit=0.01,
    up_axis=AxisHint.Y,
    tags=["furniture", "desk", "office"],
)
desc.validate()  # raises AssetImportValidationError on violation
```

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `asset_id` | `str` | Yes | Stable identifier (e.g. `"arch/city-bank/desk"`). |
| `variants` | `List[AssetFileVariant]` | Yes (≥1) | File formats available. At least one variant with a non-empty `local_path`. |
| `attribution` | `Optional[AssetAttribution]` | No | Legal attribution metadata (see [table below](#attribution-metadata-fields)). |
| `preview` | `Optional[str]` | No | Path to a preview image / thumbnail. |
| `unit_hint` | `str` | No (default `unitless`) | Native unit hint from [`UnitHint`](#unithint-values). |
| `meters_per_unit` | `float` | No (default `1.0`) | Conversion factor from asset units to meters. |
| `up_axis` | `str` | No (default `"y"`) | Source asset up-axis from [`AxisHint`](#axishint-values). |
| `scale_hint` | `Optional[float]` | No | Suggested import scale multiplier. |
| `source_bbox` | `Optional[Dict]` | No | Bounding box from the source tool. **Always cm, Y-up.** Shape: `{"min": [x, y, z], "max": [x, y, z]}`. |
| `tags` | `List[str]` | No | Free-form tags for categorisation / search. |
| `extra` | `Dict[str, Any]` | No | Free-form mapping for source-specific enrichments. |

#### `AssetFileVariant`

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `local_path` | `str` | Yes | Absolute or relative path to the asset file on disk. |
| `format` | `str` | No (default `unknown`) | File format from [`AssetFormat`](#assetformat-string-constants). |
| `preferred` | `bool` | No (default `False`) | When the descriptor has multiple variants, `True` marks the recommended one. |
| `mime` | `Optional[str]` | No | MIME type (e.g. `"model/fbx"`). |
| `file_ref` | `Optional[Dict]` | No | Structured URI reference (v1 optional; `local_path` is the primary identifier). |

#### Variant selection rule

When a descriptor carries multiple variants the adapter should prefer the one
with `preferred=True`.  If none is marked, or several are marked, the adapter
may choose by format compatibility (e.g. native format over interchange).  The
contract does not mandate a specific tie-breaker — adapter documentation should
state the selection strategy.

### Validation guarantee

`AssetDescriptor.validate()` enforces three hard invariants:

1. **variants must not be empty** — every descriptor carries at least one file.
2. **every variant must have a non-empty `local_path`** — file-ref-only variants
   are not supported in v1.
3. **if attribution is set:** `source_url` must be non-empty **and** at least
   one of `license_spdx` / `license_text` must be present.

Violations raise `AssetImportValidationError` (a `ValueError` subclass).

Source skills should **always** call `validate()` before returning a descriptor.
Adapters should **never** trust an unvalidated descriptor — call `validate()`
at the top of `import_to_scene`.

### String constant enums

### AssetFormat string constants

Well-known interchange format identifiers:

| Constant | Value |
|----------|-------|
| `AssetFormat.OBJ` | `"obj"` |
| `AssetFormat.FBX` | `"fbx"` |
| `AssetFormat.GLTF` | `"gltf"` |
| `AssetFormat.GLB` | `"glb"` |
| `AssetFormat.USD` | `"usd"` |
| `AssetFormat.USDZ` | `"usdz"` |
| `AssetFormat.ABC` | `"abc"` |
| `AssetFormat.BLEND` | `"blend"` |
| `AssetFormat.UNKNOWN` | `"unknown"` |

### UnitHint values

| Constant | Value |
|----------|-------|
| `UnitHint.METER` | `"meter"` |
| `UnitHint.CENTIMETER` | `"centimeter"` |
| `UnitHint.INCH` | `"inch"` |
| `UnitHint.UNITLESS` | `"unitless"` |

### AxisHint values

| Constant | Value |
|----------|-------|
| `AxisHint.Y` | `"y"` |
| `AxisHint.Z` | `"z"` |

---

## DCC Adapter Skill — implementing `import_to_scene`

An adapter skill receives an `AssetDescriptor`, resolves the file, imports it
into the host DCC scene, and returns an `ImportToSceneResult`.

### The `import_to_scene` contract

**Input** — `ImportToSceneRequest`, serialised to a plain dict:

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `descriptor` | `AssetDescriptor` | Yes | The asset to import (from source skill). |
| `material_mode` | `str` | No (default `as_authored`) | Material handling strategy from [`MaterialMode`](#materialmode-values). |
| `placement` | `Optional[PlacementHint]` | No | Position, rotation, scale override. |
| `target_collection` | `Optional[str]` | No | Name of the collection / layer to import into. |
| `skip_existing` | `bool` | No (default `False`) | Skip import when the `asset_id` is already in the scene. |
| `extra` | `Dict[str, Any]` | No | Adapter-specific options. |

**Output** — `ImportToSceneResult`:

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `success` | `bool` | Yes | `True` when the import completed without fatal errors. |
| `imported_nodes` | `List[str]` | No | Names or paths of nodes created / updated in the scene. |
| `warnings` | `List[ImportWarning]` | No | Non-fatal warnings encountered during import. |
| `error_message` | `Optional[str]` | No | Human-readable error when `success` is `False`. |
| `extra` | `Dict[str, Any]` | No | Free-form DCC-specific enrichments. |

#### MaterialMode values

| Constant | Value | Meaning |
|----------|-------|---------|
| `MaterialMode.AS_AUTHORED` | `"as_authored"` | Use materials as they appear in the source file. |
| `MaterialMode.DEFAULT_GRAY` | `"default_gray"` | Replace all materials with a default gray shader. |
| `MaterialMode.SKIP` | `"skip"` | Do not import any materials. |

#### `PlacementHint`

| Field | Type | Meaning |
|-------|------|---------|
| `translate` | `Optional[List[float]]` | Translation offset `[x, y, z]` in host units. |
| `rotate` | `Optional[List[float]]` | Euler rotation `[rx, ry, rz]` in degrees. |
| `scale` | `Optional[List[float]]` | Scale factors `[sx, sy, sz]`. |
| `parent_name` | `Optional[str]` | Parent object name for scene hierarchy attachment. |

### Bounding box normalization

`source_bbox` on `AssetDescriptor` is always expressed in **centimeters** with
**Y-up** orientation — matching `_core.BoundingBox` native units.  Adapters
**must** convert host-native units to cm before writing `source_bbox` into a
descriptor, and **must** apply the reverse conversion when reading it back for
viewport framing or snapping.

| Source DCC unit | Conversion to cm |
|-----------------|------------------|
| Meters | multiply by 100 |
| Inches | multiply by 2.54 |
| Centimeters | identity |
| Unitless | treated as cm |

The `unit_hint` / `meters_per_unit` pair on `AssetDescriptor` tells the
adapter how to scale the imported geometry itself (the file content).
`source_bbox` is an orthogonal, already-normalized hint for viewport framing
and does not affect the geometry import pipeline.

### Writing attribution metadata to DCC custom attrs

When a descriptor carries `attribution`, the adapter **should** write those
fields as custom attributes on the imported root node(s) so the attribution
survives scene save/reload and is discoverable by other tools.

#### Attribution metadata fields

| Key | Type | Written when | Example |
|-----|------|-------------|---------|
| `source_url` | `str` | Always (if set) | `"https://example.com/assets/desk"` |
| `license_spdx` | `Optional[str]` | If non-empty | `"CC-BY-4.0"` |
| `license_text` | `Optional[str]` | If non-empty | Full license text |
| `author` | `Optional[str]` | If non-empty | `"City Bank Studio"` |
| `title` | `Optional[str]` | If non-empty | `"Office Desk"` |
| `attribution_text` | `Optional[str]` | If non-empty | Pre-formatted display string |

Recommended naming convention for DCC custom attributes:
`dcc_mcp_attribution_<key>` (e.g. `dcc_mcp_attribution_source_url`).  This
prefix avoids collisions with DCC-native and other-plugin attributes.

### ImportWarning codes

| Warning code | Meaning |
|--------------|---------|
| `missing_textures` | Texture files referenced by the asset were not found on disk. |
| `missing_plugin` | A required DCC plugin or format handler is not installed. |
| `scale_clamped` | Import scale was clamped to the adapter's supported range. |
| `rotation_clamped` | Rotation values were clamped to the adapter's supported range. |
| `non_uniform_scale_baked` | Non-uniform scaling was baked into the geometry. |
| `degenerate_geometry_skipped` | Zero-area faces or zero-length edges were dropped. |
| `hierarchy_flattened` | The original scene hierarchy was collapsed (e.g. USD to OBJ fallback). |
| `names_collided` | Object names conflicted with existing scene objects; suffixes were appended. |
| `material_fallback` | Materials were downgraded (e.g. PBR → standard). |
| `animation_stripped` | Animation data was discarded (format or adapter limitation). |
| `unsupported_feature` | A specific feature in the file is not supported by this adapter. |
| `unknown` | An unrecognised warning that does not fit any other code. |

Each warning is an `ImportWarning(code, message, detail?)` frozen dataclass.
Consumers should match on `code`, not on `message`.

---

## Format matrix

What the source skill sends and the adapter receives:

| Format | Source example | Adapter expectation |
|--------|---------------|-------------------|
| **OBJ** | `local_path: "/data/table.obj"`, `format: "obj"` | Standard Wavefront OBJ with MTL sidecar. No hierarchy, single mesh group. |
| **FBX** | `local_path: "/data/desk.fbx"`, `format: "fbx"` | Autodesk FBX, binary or ASCII. Adapter applies unit/axis conversion from descriptor hints. |
| **GLB** | `local_path: "/data/robot.glb"`, `format: "glb"` | Binary glTF 2.0. Adapter may read embedded textures from the buffer. |
| **USD** | `local_path: "/data/corridor.usd"`, `format: "usd"` | Pixar USD ASCII or USDA/USDC. Adapter may need `pxr.Usd` or a USD C++ runtime. |
| **USDZ** | `local_path: "/data/corridor.usdz"`, `format: "usdz"` | Packaged USDZ archive. Adapter unpacks and imports the root USD layer. |
| **ABC** | `local_path: "/data/anim.abc"`, `format: "abc"` | Alembic cache (primarily geometry / animation). |
| **BLEND** | `local_path: "/data/scene.blend"`, `format: "blend"` | Native Blender file. Only meaningful in Blender adapter; other adapters should reject with a clear error. |

No conversion tools live in `dcc-mcp-core`.  Each adapter owns its format
handling — the contract only describes what format string to expect, not how
to parse the file.

---

## Relationship to `SceneStats`

The asset import contract and the [cross-DCC verification](cross-dcc-verification.md)
contract serve different phases of the asset lifecycle but share the same design
philosophy (frozen dataclasses, `to_dict` / `from_dict`, Python-only v1).

| Aspect | Asset Import | SceneStats Verification |
|--------|-------------|------------------------|
| **Phase** | Source → DCC (producer puts asset **into** scene) | File → DCC (verifier opens file and **checks** contents) |
| **Primary object** | `AssetDescriptor` → `ImportToSceneResult` | `SceneStats` |
| **Scope** | Full descriptor: variants, attribution, placement, transforms | Three fields: object count, vertex count, has-mesh |
| **Error handling** | `ImportWarning` list + `ImportToSceneResult.success` | No warning model — strict/fuzzy `matches()` |
| **Where it lives** | `dcc_mcp_core.asset_import` | `dcc_mcp_core.verifier` |

In a round-trip pipeline:

1. **Produce** — source skill creates and exports an asset (e.g. Blender →
   FBX).
2. **Import** — `import_to_scene` loads the exported file into another DCC
   (e.g. Maya) and returns `ImportToSceneResult`.
3. **Verify** — a verifier skill calls `import_and_inspect` on the same file
   and returns `SceneStats` to assert round-trip fidelity.

Step 2 (import) is the **producer→scene** direction.  Step 3 (verify) is the
**file→verifier** direction.  They are complementary — import guarantees the
asset landed in the scene; verify guarantees the file contents survived
unchanged.

---

## FAQ

### Why Python-only in v1? Why no Rust crate?

The asset import contract is pure data — frozen dataclasses with `to_dict` /
`from_dict`.  A Rust crate would add build complexity without measurable
benefit at this stage.  If performance-critical validation or serialisation
pressure emerges, the contract can be migrated to a `dcc-mcp-asset-import`
crate without changing the dict wire format.

### What about `file_ref` vs `local_path`?

`local_path` is the required primary identifier in v1.  `file_ref` (an
artefact URI from `_core.FileRef`) is an optional addition — it exists so that
future versions can support artefact-store-backed resolution without breaking
existing callers.

### Why `AssetFormat.UNKNOWN` and not `"other"`?

`unknown` signals "this source did not report a format."  Adapters should
attempt format detection from the file extension or content.  `"other"` would
imply a known-other format, which the contract does not distinguish.

### Can a source skill return zero variants?

No — `AssetDescriptor.validate()` rejects empty `variants`.  Return an empty
result list instead of a descriptor with no files.

### Do adapters need to handle every `AssetFormat`?

No.  Each adapter documents which formats it supports.  If an adapter receives
a format it cannot handle, it should return `ImportToSceneResult(success=False,
error_message="Unsupported format: ...")` rather than crashing.

### Is the attribute naming convention (`dcc_mcp_attribution_*`) enforced?

No — it is a recommendation.  Adapters may use any naming scheme as long as
the attribution is discoverable in the DCC's custom attribute / metadata UI.
The convention maximises cross-DCC tooling compatibility.

## Related

- [`cross-dcc-verification.md`](cross-dcc-verification.md) — `SceneStats`
  contract for file-based round-trip verification.
- [`host-adapter.md`](host-adapter.md) — the `HostAdapter` base class
  downstream repos extend to plug an importer into their DCC's idle loop.
- [`dcc-thread-safety.md`](dcc-thread-safety.md) — main-thread dispatcher
  primitives that adapter importers rely on when running inside a live DCC
  session.
- [`skills.md`](skills.md) — SKILL.md format the import skills are built on.
- [`artefacts.md`](artefacts.md) — `FileRef` and `ArtefactStore` for
  cross-tool file handoff (future `file_ref` resolution).
