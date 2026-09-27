---
name: blender-asset-source
description: "Blender asset source discovery — search filesystem and asset libraries for importable 3D assets"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [blender, asset, source, search, discovery, import]
    search-hint: "search assets, find assets, asset browser, asset library, browse files, discover 3D files, asset source"
    search-aliases: [asset discovery, search for assets, find 3D files, asset browser, file search, browse assets, locate assets, asset source, import source]
    intent: "Search discoverable asset sources (filesystem directories and Blender asset libraries) and return validated AssetDescriptors for the downstream import pipeline."
    recall-context:
      app_type: blender
      domain: pipeline
      workflow_stage: source
      task_category: query
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
    side-effects:
      creates: false
      modifies: false
      deletes: false
      file_output: false
      targets: [filesystem]
    produces: [asset_descriptors, asset_search_results]
    requires:
      - type: software
        name: blender
        version: ">=4.0"
    tools: tools.yaml
---

# blender-asset-source

Search discoverable asset sources and return validated `AssetDescriptor[]` for
the downstream import pipeline.

Supports:
- **Filesystem search** — walk a local directory for supported 3D file formats.
- **Blender asset library search** — scan registered Blender asset libraries.
- **Type filtering** — restrict results to specific formats (blend, fbx, obj, usd, etc.).
- **Name matching** — case-insensitive substring filter on asset names.

Library registrations come from `bpy.context.preferences.filepaths.asset_libraries`.
Search is read-only: it does not refresh Asset Browser UI or save preferences.
`context.asset_library_status` distinguishes `scanned`, `no_libraries`,
`unavailable`, `partial`, and `error`; missing APIs or unreadable libraries are not empty
successful library searches. Broken registrations do not hide later valid
libraries: partial results include explicit status and warnings. With `source=all`, filesystem results can be
returned with a library warning. Reaching the result budget may truncate search.

This searches supported files directly inside registered local directories,
not asset-marked data-blocks inside `.blend` files. Remote libraries, catalogs,
tags, and recursive dependency discovery require separate capabilities.

## Tools

| Tool | Description |
|------|-------------|
| `search_assets` | Search asset sources and return validated `AssetDescriptor[]` |
