# RECIPES.md — asset

> Copy-pasteable asset pipeline sequences.

## Find and Import

```bash
# 1. Search catalog
dcc-mcp-cli call asset_source__search_assets --json '{"query":"table"}'

# 2. Resolve to full descriptor
dcc-mcp-cli call asset_source__search_assets --json '{"query":"table_01"}'

# 3. Find host import tool
dcc-mcp-cli search --query "import to scene" --dcc-type maya

# 4. Import
dcc-mcp-cli call maya.<id>.maya_import__import_to_scene \
  --json '{"descriptor":{...}}'
```

## Export Selection as Asset

```bash
dcc-mcp-cli search --query "export selection" --dcc-type maya
dcc-mcp-cli call maya.<id>.maya_export__export_selection \
  --json '{"asset_name":"my_prop","format":"fbx","selection_only":true}'
```

## Validate an Asset File

```bash
python -c "
from pathlib import Path
p = Path('/assets/table_01.fbx')
print(f'exists={p.exists()} size={p.stat().st_size}')
"
```

## Cross-DCC Round-Trip

```
Maya export(fbx) → asset catalog → Blender import
    ↓                                  ↓
asset_export                    asset_import
```

## See Also

- `references/INTROSPECTION.md` — AssetDescriptor contract
- `references/ERRORS.md` — Common import/export failures
