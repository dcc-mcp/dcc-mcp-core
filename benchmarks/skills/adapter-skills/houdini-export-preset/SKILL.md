---
name: houdini-export-preset
description: >-
  Interchange skill — save/load/list/delete reusable export presets as JSON
  library files capturing ROP node configurations (Alembic, FBX, USD,
  Geometry, and other ROP output drivers). Use to standardize export
  settings across a team or project. Pair with houdini-interchange for
  one-shot exports or houdini-pipeline for shot packaging.
license: MIT
compatibility: "dcc-mcp-houdini 0.1+, Houdini 19.5+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: interchange
    version: "1.0.0"
    tags: [houdini, export, preset, rop, alembic, fbx, usd, pipeline, interchange, configuration]
    search-hint: "export preset, save export settings, ROP config, FBX preset, Alembic preset, USD export config, share export setup, export library"
    search-aliases: [save export preset, load export preset, export template, export profile, reuse export settings, saved export config, export configuration, rop settings, batch export config, share export settings]
    example-prompts:
      - "Save the current Alembic export settings as a preset for the project"
      - "List all export presets in the library"
      - "Load the character_FBX export preset and apply it to geo1"
      - "Delete the outdated export preset"
    intent: "Manage reusable ROP export presets as JSON library files for consistent interchange."
    recall-context:
      app_type: houdini
      domain: io
      workflow-stage: interchange
      task-category: mutate
    requires: []
    produces: [export_preset, rop_configuration]
    preconditions:
      - type: software
        name: houdini
        version: ">=19.5"
    side-effects:
      creates: true
      modifies: true
      targets: [filesystem, rop_node]
    tools: tools.yaml
---

# houdini-export-preset

JSON-backed export preset management for Houdini ROP networks. Sits next to
`houdini-interchange` so an agent can:

1. `save_export_preset` to persist a ROP node's export configuration as JSON
2. `load_export_preset` to retrieve a saved preset (optionally create a ROP
   node from it)
3. `list_export_presets` / `delete_export_preset` for library management

## Tool groups

- **`export-presets`:** `save_export_preset`, `load_export_preset`,
  `list_export_presets`, `delete_export_preset` — JSON files under a
  user-supplied `library_dir` (defaults to `~/.dcc-mcp/houdini/export_presets/`,
  override with `DCC_MCP_HOUDINI_EXPORT_PRESET_DIR`).

## Supported ROP types

| Format | ROP node type | Typical output |
|--------|--------------|----------------|
| alembic | `rop_alembic` | `.abc` |
| fbx | `rop_fbx` | `.fbx` |
| usd | `usdrender` / `usd_rop` | `.usd` / `.usda` / `.usdc` |
| geometry | `geometry` | `.bgeo` / `.geo` / `.obj` |
| ifd | `ifd` | `.ifd` (Mantra) |
| opengl | `opengl` | `.png` / `.jpg` (viewport) |
| redshift | `Redshift_ROP` | `.exr` (Redshift) |
| karma | `karma` | `.exr` (Karma) |

## Context limitations

- Presets store the ROP node's *parameters* (what), not the SOP/LOP
  network that feeds it (how). A preset is transferable across scenes as
  long as a node with the matching source path exists.
- `load_export_preset` with `create_rop: true` creates a ROP node inside
  `/out` (or the specified `rop_parent_path`) and wires it to the source
  node recorded in the preset when `connect_source: true`.

## Tracer-bullet flow

1. `houdini_interchange__export_alembic(node_path="/obj/geo1", output_path="/tmp/test.abc")`
2. `save_export_preset(rop_node_path="/out/alembic1", preset_name="character_alembic")`
3. `list_export_presets()` → find available presets
4. `load_export_preset(preset_name="character_alembic", create_rop=True, source_node_path="/obj/geo2")` → recreate ROP for a different object
