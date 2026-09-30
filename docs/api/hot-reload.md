# Hot Reload API

Generic skill hot-reload support for any DCC adapter (requires dcc-mcp-core >= 0.12.24).

Monitors skill directories for changes and automatically reloads affected skills without requiring a server restart. DCC-agnostic.

**Exported symbols:** `DccSkillHotReloader`

## Adapter catalog refresh

`DccServerBase.reload_skill_paths()` re-scans the skill catalog. The direct
MCP tool `dcc_admin__reload_skills` invokes this method without arguments.
Both retain the `extra_skill_paths` and `include_bundled` options from the last
successful registration or explicit reload. Environment and admin-configured
paths are resolved again on every scan.

Pass `extra_skill_paths=[...]` to replace the saved extra roots, or `[]` to
clear them. Pass `include_bundled=True` or `False` to change bundled discovery;
omitting it retains the saved setting. A failed scan leaves those settings
unchanged. After a catalog refresh, use `load_skill` to activate the refreshed
tool definitions.

## DccSkillHotReloader

### Constructor

| Parameter | Type | Description |
|-----------|------|-------------|
| `dcc_name` | `str` | Short DCC identifier for log messages |
| `server` | `Any` | DCC MCP server instance (must expose `_server` with `list_skills()` and `load_skill()`) |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_enabled` | `bool` | Whether hot-reload is currently active |
| `reload_count` | `int` | Total number of reload events triggered |
| `watched_paths` | `list[str]` | Directories currently being monitored |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `enable(skill_paths, debounce_ms=300)` | `bool` | Enable hot-reload for given directories |
| `disable()` | `None` | Disable hot-reload and clean up the SkillWatcher |
| `reload_now()` | `int` | Manually trigger a reload; returns number of skills reloaded |
| `get_stats()` | `dict` | Return `{enabled, watched_paths, reload_count}` |

```python
from dcc_mcp_core import DccSkillHotReloader

reloader = DccSkillHotReloader(dcc_name="blender", server=self)
reloader.enable(["/path/to/skills"], debounce_ms=300)
# ... files are now monitored, skills reload automatically ...
reloader.disable()
```
