---
name: unreal-assets
description: >-
  Domain skill - Unreal Engine Content Browser asset management: list, import,
  export, inspect, and delete assets. Use for package and asset operations in
  the Unreal Editor. Not for actor placement or transforms - use unreal-actors
  for that.
license: MIT
compatibility: Unreal Engine 5.0+, Python 3.9+
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: unreal
    version: "0.1.0"
    layer: domain
    stage: interchange
    search-hint: "unreal content browser asset registry import export delete fbx texture package"
    tags: "unreal, assets, content-browser, import, export, pipeline"
    tools: tools.yaml
---

# Unreal Assets

## Detailed import verification

`import_asset` refuses a destination containing pre-existing unsaved assets.
After the synchronous import, it explicitly saves returned objects and assets
newly created or dirtied by that import inside the destination, including
materials and textures omitted from `AssetImportTask.imported_object_paths`.
The `persistence` receipt lists successful saves and failures. A partial save
returns an error while leaving imported assets available for inspection; it is
not rolled back. Saving is not a fresh-session reload verification.

Call `get_asset_info` with an exact object path, `include_dependencies: true`
and `include_details: true` after import. Details report each observation as
`observed` (including a real zero/empty value) or `unavailable` with a reason.
Static meshes include per-LOD render vertices/triangles/sections, section-to-slot
mapping, screen sizes, local bounds in centimeters, collision counts, material
slots, parent materials, global parameters and package dirty state. Material
property readbacks and instance overrides are raw evidence, not a synthesized
effective shader result. Layer parameters are outside the global parameter scope.

Direct dependency load checks exclude script packages explicitly. This is not a
recursive dependency closure or proof of saved disk state. Native SpeedTree wind
binding is not reflected by this contract; shader parameters are not dynamic wind
validation. The tool does not save assets, change the level or render a preview.

Tools for Content Browser asset discovery, import, export, inspection, and deletion.

Static Groom imports require `HairStrands` and `AlembicHairImporter`. Generic
imports of `.usd`, `.usda`, `.usdc`, or `.usdz` require `USDImporter`. These
dependencies are checked before an import task or Content Browser mutation is
created, and failures include the exact missing plugin names.

## Scripts

- `list_assets`
- `import_asset`
- `import_groom_cache`
- `import_static_groom`
- `export_asset`
- `get_asset_info`
- `delete_asset`
- `create_ocio_configuration`
