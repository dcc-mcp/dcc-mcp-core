# ERRORS.md — asset

> Common asset pipeline failures and fixes.

## Import Failures

| Symptom | Fix |
|---------|-----|
| `asset_not_found` | Verify name with `search_assets`; check catalog health |
| `format_not_supported` | Check `variants` for alternative format |
| `descriptor_invalid` | Validate descriptor against contract |
| `file_not_found` | Re-resolve asset; check `variant.path` |
| `import_tool_missing` | Check adapter install; search for import tools |

## Export Failures

| Symptom | Fix |
|---------|-----|
| `export_tool_missing` | Check adapter; search for export tools |
| `format_not_writable` | Choose supported format (fbx, usd, abc, obj) |
| `selection_empty` | Select objects first or set `selection_only=false` |
| `permission_denied` | Check `output_dir` permissions |

## Validation Failures

| Symptom | Fix |
|---------|-----|
| `file_too_small` | Likely corrupted export; re-export from source |
| `format_mismatch` | Extension doesn't match content; rename or re-export |
| `texture_missing` | Re-export with embedded textures or fix paths |

## Catalog Errors

| Symptom | Fix |
|---------|-----|
| `catalog_unhealthy` | Check network, catalog config |
| `stale_catalog` | Trigger catalog refresh |
