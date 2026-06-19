---
name: asset-source
description: >-
  Gateway skill for cross-DCC asset import — search and resolve assets into a
  validated AssetDescriptor (local path + attribution). Demo source returns
  static catalog entries; production sources can add download or remote
  resolution without changing the contract.
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    stage: source
    version: "1.0.0"
    tags: [pipeline, asset-import, read-only]
    search-hint: >-
      search assets, find asset, resolve asset, asset descriptor, asset import source,
      cross-dcc asset
    search-aliases: [asset search, find asset, resolve asset, asset catalog, import source, asset descriptor]
    intent: "Search and resolve assets into a validated AssetDescriptor (local path + attribution) for cross-DCC import pipelines."
    recall-context:
      app_type: python
      domain: asset
      workflow_stage: source
      task_category: query
    preconditions: []
    side-effects:
      creates: false
      modifies: false
      file_output: false
      targets: []
    produces: [asset_descriptor]
    requires: []
    tools: tools.yaml
---

# asset-source

Gateway skill for searching and resolving assets into a validated
`AssetDescriptor` ready for cross-DCC import. The demo source returns static
catalog entries keyed by asset name. Production sources can layer download
helpers or remote resolution on top without changing the contract.

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `search_assets` | Query | Search the asset catalog and return matching `AssetDescriptor` entries |

The skill uses `dcc_mcp_core.AssetDescriptor` (from the shared
`dcc_mcp_core.asset_import` contract) as the wire format so every downstream
DCC adapter speaks the same shape.

## Gateway flow

```
search_skills("asset import") → load_skill("asset-source") → call("search_assets", {query: "table"})
→ AssetDescriptor → load_skill("blender-import-to-scene") → call("import_to_scene", {descriptor: ...})
```
