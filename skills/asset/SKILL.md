---
name: asset
description: >-
  Task-oriented domain skill — cross-DCC asset pipeline: search, resolve,
  import, export, and validate creative assets. Use when the user asks "find
  this asset", "import this model", "export to...", "what assets are
  available?", or any asset management workflow. Builds on asset-source with
  import/export lifecycle orchestration. Not for initial setup verification —
  use verify; not for diagnosing import failures — use debug.
license: MIT
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [pipeline, asset, import, export, read-only]
    search-hint: >-
      asset search find import export resolve validate catalog model texture
      material cross-dcc 资产 导入 导出 搜索 查找
    search-aliases: [asset search, find asset, import asset, export asset, asset catalog, asset pipeline]
    intent: "Search, resolve, import, export, and validate creative assets across DCC applications."
    recall-context:
      app_type: python
      domain: asset
      workflow_stage: pipeline
      task_category: mixed
    preconditions:
      - dcc-mcp-cli on PATH or Python fallback available
      - asset-source skill available
    side-effects:
      creates: true
      modifies: true
      file_output: true
      targets: [asset_file, scene]
    produces: [AssetDescriptor, import_result, export_result]
    requires: [dcc-mcp, asset-source]
    tools: tools.yaml
    depends: [dcc-mcp, asset-source]
---

# Asset — Task-Oriented Cross-DCC Pipeline

> **Domain skill**: Use this to find, import, export, and manage creative assets
> across DCC applications.

Asset orchestrates `asset-source` catalog search and per-adapter import/export
tools into single-call asset workflows. Use it to move creative data between
DCCs without manually chaining search → resolve → import steps.

## When to use

| Scenario | Tool |
|----------|------|
| Find an asset in the catalog | `search_assets` — search by name, type, tags |
| Get full details for import | `resolve_asset` — full AssetDescriptor from search hit |
| Import an asset into the active DCC | `import_asset` — resolve + discover host tool + import |
| Export selection/scene as an asset | `export_asset` — discover host tool + export + register |
| Check asset file integrity | `validate_asset` — format + file checks |
| Catalog health and stats | `catalog_status` — count, types, sources, health |

## When NOT to use

- **Verifying instance readiness** — use the `verify` skill
- **Diagnosing import failures** — use the `debug` skill
- **Interacting with UI** — use the `ui` skill
- **Building/deploying skills** — use the `build` skill

## Canonical import flow

```
search_assets("table")
  → resolve_asset("table_01")       # Get AssetDescriptor
    → dcc-mcp: search("import")     # Find host's import tool
      → import_asset(descriptor)     # Execute import
```

## Canonical export flow

```
dcc-mcp: search("export selection")
  → export_asset(name, format)      # Export + register in catalog
```

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `search_assets` | Query | Search catalog by name, type, tags, metadata |
| `resolve_asset` | Query | Full AssetDescriptor with variants and attribution |
| `import_asset` | Write | Resolve + discover host tool + import to scene |
| `export_asset` | Write | Discover host tool + export selection + register |
| `validate_asset` | Query | Check file integrity, format, metadata |
| `catalog_status` | Query | Catalog stats: count, types, sources, health |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md) — copy-pasteable asset workflows
- **Contract reference**: [INTROSPECTION.md](references/INTROSPECTION.md) — AssetDescriptor, variants, attribution
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md) — import/export failure patterns
