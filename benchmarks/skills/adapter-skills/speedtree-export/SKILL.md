---
name: speedtree-export
description: Export selected SpeedTree SPM trees through the official Modeler CLI and verify an asset bundle for another DCC adapter.
license: MIT
metadata:
  dcc-mcp:
    dcc: speedtree
    layer: domain
    version: "0.1.0"
    tools: tools.yaml
    search-hint: "SpeedTree batch export ST9 FBX OBJ Alembic USD manifest dependencies"
---

# SpeedTree asset export

Use Games presets for ST9/ST and VFX presets for FBX/OBJ/Alembic/USD. Use plan_import to verify a bundle and prepare target-neutral handoff. Discover destination importer capabilities; Maya, 3ds Max, Blender, Houdini and Unity must not be assumed to have every format plugin. Preserve sidecars and texture paths. See the repository compatibility matrix for measured coverage.

Discover and fingerprint distinct Games SPM samples. For UE 5.5 prefer the installed Unreal ST9 Games preset. Configure the Modeler executable on the service. Call export_batch with a new output directory; this writes assets and textures. Check every item status, then verify_export before handoff. Exported does not mean imported.

Pass each mesh path from the returned manifest to dcc-mcp-unreal's discovered import_asset tool with a separate destination folder and replace_existing=false. Keep all files in each per-tree directory beside the mesh. Read back each imported mesh and its dependencies. Verify scale/orientation, materials/textures, and applicable LOD/wind in Unreal. Missing files, input changes, failed exit, or timeout stop the batch. Never copy SPM files into Content as if they were imported assets.

CLI work needs no GUI or injected hook. UI inspection uses only dcc-cua/ui-control with exact target binding. A license prompt requires the operator; do not retry via another integration.

For wind, inspect the installed VFX Wind preset and preserve the source SPM.
Export requests record `requested_animation`; they do not establish that the
file contains animation. Require a target cache or runtime readback at multiple
times with changed vertex positions or rendered wind before accepting motion.
Modeler Fan/generator wind editing is not implemented. The tested Modeler 10.1
Palm exports did not produce dynamic cache motion despite wind preset requests.
