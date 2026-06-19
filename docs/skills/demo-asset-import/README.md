# Cross-DCC Asset Import — End-to-End Demo

This demo walks through the first cross-DCC asset import flow: search for an
asset via the gateway `asset-source` skill, then import it into a Blender scene
via `blender-import-to-scene`.

## Prerequisites

- `dcc-mcp-core` (with `asset-source` skill)
- `dcc-mcp-blender` (with `blender-import-to-scene` skill)
- Blender >= 4.0 running with the dcc-mcp-blender adapter
- Gateway daemon running

## Architecture

```
[Agent]
  │
  ├─ search_skills("asset import")
  │   └─ returns [asset-source, blender-import-to-scene]
  │
  ├─ load_skill("asset-source")
  │   └─ call("search_assets", {query: "table"})
  │       └─ returns AssetDescriptor[]
  │
  ├─ load_skill("blender-import-to-scene")
  │   └─ call("import_to_scene", {descriptor: <AssetDescriptor>, target_collection: "Furniture"})
  │       └─ returns ImportToSceneResult
  │
  └─ load_skill("blender-scene")
      └─ call("list_objects")
          └─ verify imported nodes in scene
```

## Step-by-Step Walkthrough

### 1. Search for an asset

```
search_skills("asset import")
→ [asset-source, blender-import-to-scene]
```

Load the source skill:

```
load_skill("asset-source")
```

Call the search tool:

```
call("search_assets", {query: "table"})
```

Expected result:

```json
{
  "success": true,
  "message": "Found 1 asset(s) for 'table'",
  "context": {
    "results": [
      {
        "asset_id": "props/table-round",
        "variants": [
          {
            "local_path": "/data/assets/props/table_round.obj",
            "format": "obj",
            "preferred": true,
            "mime": "model/obj"
          }
        ],
        "attribution": {
          "source_url": "https://example.com/assets/table-round",
          "license_spdx": "CC-BY-4.0",
          "author": "Example Studio",
          "title": "Round Table"
        },
        "unit_hint": "centimeter",
        "meters_per_unit": 0.01,
        "up_axis": "y",
        "tags": ["furniture", "indoor", "table"]
      }
    ],
    "scores": [100.0],
    "total": 1,
    "query": "table"
  }
}
```

### 2. Import into Blender

```
load_skill("blender-import-to-scene")
```

Call the import tool with the descriptor from step 1:

```
call("import_to_scene", {
  descriptor: <AssetDescriptor from search>,
  target_collection: "Furniture"
})
```

Expected result:

```json
{
  "success": true,
  "message": "Imported 'props/table-round' into scene (5 node(s))",
  "context": {
    "success": true,
    "imported_nodes": ["table_round_mesh", "table_round_legs", "..."],
    "warnings": [],
    "extra": {}
  }
}
```

### 3. Verify

```
load_skill("blender-scene")
call("list_objects")
```

## Demo Catalog

The `asset-source` demo catalog includes:

| Asset ID | Format | Unit | Up Axis |
|----------|--------|------|---------|
| `props/table-round` | OBJ | cm | Y |
| `arch/city-bank/desk` | FBX | cm | Y |
| `characters/robot-v2` | FBX | m | Y |
| `sets/sci-fi-corridor` | USD + USDZ | m | Z |
| `props/chair-modern` | FBX | cm | Y |

## Constraints

- No Maya adapter — Blender-only demo per PIP-1832 decision
- No new gateway architecture — uses existing `search_skills` → `load_skill` → `call` flow
- No native MCP OAuth
- `AssetDescriptor` contract lives in `dcc_mcp_core.asset_import` (#1708)

## Next Steps (post-demo)

1. Replace static catalog with a real asset registry
2. Add download-helper for remote asset resolution
3. Record video walkthrough
4. Add Maya adapter (after Blender demo is recorded)
