---
name: asset
description: >-
  Gateway skill for cross-DCC asset operations — resolve asset descriptors,
  import/export to-from DCC hosts, validate file integrity, and inspect
  catalog status. Sits above asset-source for resolution and delegates to
  DCC-specific import/export skills at runtime.
license: MIT
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    tags: [pipeline, asset-import, asset-export, validation, catalog]
    search-hint: >-
      resolve asset, import asset, export asset, validate asset file,
      catalog status, asset statistics, cross-dcc asset pipeline
    search-aliases: [asset resolve, asset import, asset export, asset validate, catalog stats, asset pipeline]
    intent: "Resolve, import, export, and validate assets across DCC hosts using the shared AssetDescriptor contract."
    recall-context:
      app_type: python
      domain: asset
      workflow_stage: pipeline
      task_category: orchestration
    preconditions:
      - asset-source skill loaded for resolution
    side-effects:
      creates: true
      modifies: true
      file_output: true
      targets: [dcc_scene, asset_catalog]
    produces: [asset_descriptor, import_result, export_result, validation_report, catalog_stats]
    requires: [asset-source]
    tools: tools.yaml
---

# asset

Gateway skill for cross-DCC asset pipeline operations. Builds on top of
`asset-source` for resolution and delegates import/export to DCC-specific
skills discovered at runtime.

## Architecture

```
asset-source (resolution)
       │
       ▼
   asset (this skill)
       │
       ├── resolve_asset ───► full AssetDescriptor
       ├── import_asset ───► host import tools
       ├── export_asset ───► host export tools
       ├── validate_asset ─► file integrity report
       └── catalog_status ─► catalog statistics
```

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `resolve_asset` | Query | Resolve an asset_id or query into a full AssetDescriptor |
| `import_asset` | Action | Import a resolved asset into a DCC host scene |
| `export_asset` | Action | Export from a DCC host and register in the catalog |
| `validate_asset` | Inspection | Check file integrity (format, size, referenced textures) |
| `catalog_status` | Query | Report catalog statistics by format, tag, or attribution |

## Gateway flow

```
search_skills("import asset") → load_skill("asset") → call("resolve_asset", {asset_id: "props/table-round"})
→ AssetDescriptor → call("import_asset", {descriptor: ..., dcc: "blender"})
→ ImportToSceneResult
```

## Prerequisites

- `asset-source` skill must be loaded for `resolve_asset` to function.
- DCC host skills (e.g. `blender-import-to-scene`) are discovered at runtime
  via the gateway's tool search; they are not hard dependencies.
