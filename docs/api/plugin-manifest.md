# Plugin Manifest — One-click Install for Claude Code

> Source: [`python/dcc_mcp_core/plugin_manifest.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/plugin_manifest.py) · Issue [#410](https://github.com/loonghao/dcc-mcp-core/issues/410) · [Claude Code plugin reference](https://code.claude.com/docs/en/plugins-reference#plugin-components-reference)
>
> **[中文版](../zh/api/plugin-manifest.md)**

Bundle an MCP server URL, skill paths, and optional sub-agents into a
single JSON manifest that users install into Claude Code with one click.

**When to use**

- Ship a pre-configured DCC integration (`maya-mcp`, `blender-mcp`, …) to
  users without forcing them to edit `claude_desktop_config.json`.
- Distribute a curated skill bundle alongside your server URL.
- Prepare for the upstream MCP
  [`experimental-ext-skills`](https://github.com/modelcontextprotocol/experimental-ext-skills)
  extension that delivers skills directly from servers.

## Imports

```python
from dcc_mcp_core import (
    PluginManifest,
    build_plugin_manifest,
    export_plugin_manifest,
)
```

## `PluginManifest` (dataclass)

| Field | Type | Notes |
|-------|------|-------|
| `name` | `str` | Plugin name (e.g. `"maya-mcp"`) |
| `version` | `str` | Plugin version string |
| `description` | `str` | Shown in the Claude Code UI |
| `mcp_servers` | `list[dict]` | Each entry has `"url"` and optional `"headers"` |
| `skills` | `list[str]` | Absolute paths to skill directories |
| `sub_agents` | `list[dict]` | Optional sub-agent definitions (default `[]`) |

Methods:

- `.to_dict() -> dict` — JSON-serialisable dict
- `.to_json(indent=2) -> str` — formatted JSON string

## `build_plugin_manifest(dcc_name, mcp_url, skill_paths=None, *, version="0.1.0", description=None, api_key=None, extra_mcp_servers=None, sub_agents=None) -> dict`

Assemble a plugin manifest dict.

| Arg | Type | Default | Notes |
|-----|------|---------|-------|
| `dcc_name` | `str` | — | Short DCC identifier; becomes `<dcc>-mcp` as the plugin `name` |
| `mcp_url` | `str \| None` | — | Full MCP endpoint (`http://host:8765/mcp`). `None` → skills-only bundle |
| `skill_paths` | `list[str] \| None` | `None` | Directories to include. Non-existent paths are dropped with a debug log |
| `version` | `str` | `"0.1.0"` | Plugin version |
| `description` | `str \| None` | auto | Auto-generated from `dcc_name` when `None` |
| `api_key` | `str \| None` | `None` | Injected into `mcp_servers[0].headers.Authorization` as `Bearer <key>` |
| `extra_mcp_servers` | `list[dict] \| None` | `None` | Additional server entries beyond the primary |
| `sub_agents` | `list[dict] \| None` | `None` | Sub-agent definitions |

**Returns** a JSON-serialisable dict matching the Claude Code plugin
schema. Log message at INFO level summarises how many servers / skills
were included.

**Example**

```python
from dcc_mcp_core import build_plugin_manifest, export_plugin_manifest

manifest = build_plugin_manifest(
    dcc_name="maya",
    mcp_url="https://mcp.example.com/mcp",
    skill_paths=["/opt/skills/maya-geometry", "/opt/skills/maya-render"],
    version="1.2.0",
    api_key="s3cret-studio-token",
)
export_plugin_manifest(manifest, "dist/maya-mcp.plugin.json")
```

## `export_plugin_manifest(manifest, path, *, indent=2) -> Path`

Write a manifest dict to disk. Creates parent directories as needed.
Returns the resolved `pathlib.Path`.

## Recommended pattern — `DccServerBase.plugin_manifest()`

When building on `DccServerBase`, use the convenience method added in #410
that auto-fills `mcp_url` and `skill_paths` from the running server:

```python
class MayaServer(DccServerBase):
    def __init__(self):
        super().__init__(dcc_name="maya", http_config=McpHttpConfig(port=8765))

server = MayaServer()
handle = server.start()
manifest = server.plugin_manifest(version="1.2.0")   # dict
```

## Manifest shape

```json
{
  "name": "maya-mcp",
  "version": "1.2.0",
  "description": "MCP plugin for Maya — provides AI-accessible tools via dcc-mcp-core.",
  "mcp_servers": [
    {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer s3cret-studio-token" }
    }
  ],
  "skills": [
    "/opt/skills/maya-geometry",
    "/opt/skills/maya-render"
  ]
}
```

## See also

- [Remote Server guide](../guide/remote-server.md)
- [Skills System](../guide/skills.md) — SKILL.md discovery & `DCC_MCP_SKILL_PATHS`
- [Claude Code plugin components reference](https://code.claude.com/docs/en/plugins-reference#plugin-components-reference)
