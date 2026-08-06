---
name: spatial-interchange
description: >-
  Plan deterministic coordinate-axis and unit conversions between DCCs,
  engines, and interchange formats from explicit right/up/forward axes and
  meters-per-unit values.
license: "MIT"
compatibility: "dcc-mcp-core 0.19.91+, Python 3.7+"
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    stage: interchange
    version: "1.0.0"
    tags: [pipeline, read-only, spatial, interchange]
    search-hint: >-
      coordinate conversion, axis conversion, unit conversion, handedness,
      DCC interchange, up axis, forward axis, meters per unit
    intent: "Plan an auditable axis and unit conversion before importing or exporting DCC data."
    side-effects:
      creates: false
      modifies: false
      file_output: false
      targets: []
    produces: [spatial_conversion_plan]
    requires: []
    tools: tools.yaml
    skill-reference-docs:
      - "references/*.md"
---

# Spatial Interchange

Use `plan_conversion` before a cross-DCC import or export when source and
target coordinate conventions are known. The tool returns a pure conversion
plan; the importing or exporting adapter remains responsible for applying it
to scene data.

Do not infer a convention from a DCC name alone. Query the live scene, import
settings, or file metadata first: Maya up-axis and units are scene settings,
Blender FBX axes are operator settings, and USD stages carry `upAxis` and
`metersPerUnit` metadata.

The [3D Transform Visualizer](https://aike.github.io/3dtr/) is a useful
educational model for signed right/up/forward bases. It is not a file
converter or a source of truth for a live scene.

## Workflow

1. Inspect source file/scene axes and physical units.
2. Inspect the target scene/importer settings.
3. Call `plan_conversion` with explicit signed axes and meters per unit.
4. Apply the returned plan in the owning adapter/importer.
5. Validate a landmark point, bounds, face orientation, normals, and animation.

Read `references/CONVENTIONS.md` for official defaults, runtime caveats, and
the rules for positions, directions, transforms, and mesh winding.
