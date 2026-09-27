---
name: 3dsmax-asset-source
description: >-
  Domain skill — search and resolve assets into validated AssetDescriptor
  contracts for cross-DCC asset import pipelines. Returns static catalog
  entries; production sources can layer download helpers or remote resolution
  without changing the contract.
license: MIT
compatibility: "dcc-mcp-core 0.18.37+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max asset search find resolve AssetDescriptor cross-DCC import source catalog"
    tags: "3dsmax, asset-import, search, read-only, catalog"
    tools: tools.yaml
    intent: "Search and resolve assets into validated AssetDescriptor contracts for 3ds Max cross-DCC import pipelines."
    search_aliases: ["asset search", "find asset", "resolve asset", "asset catalog", "import source", "asset descriptor"]
    recall_context:
      app_type: "3dsmax"
      domain: "asset"
      workflow_stage: "source"
      task_category: "query"
    preconditions: []
    side_effects:
      creates: false
      modifies: false
      deletes: false
      exports: false
      imports: false
      file_output: false
      render: false
      targets: []
    produces: ["asset_descriptor"]
---

# 3ds Max Asset Source

Gateway skill for searching and resolving assets into a validated
`AssetDescriptor` ready for cross-DCC import. Uses the shared
`dcc_mcp_core.AssetDescriptor` contract so every downstream DCC adapter
speaks the same shape.

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `search_assets` | Query | Search the asset catalog and return matching `AssetDescriptor` entries |

## Gateway flow

```
search_skills("asset import") → load_skill("3dsmax-asset-source") → call("search_assets", {query: "desk"})
→ AssetDescriptor → load_skill("3dsmax-import-to-scene") → call("import_to_scene", {descriptor: ...})
```
