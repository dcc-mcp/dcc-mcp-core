# Plugin Manifest — 为 Claude Code 打包一键安装包

> 源码：[`python/dcc_mcp_core/plugin_manifest.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/plugin_manifest.py) · Issue [#410](https://github.com/loonghao/dcc-mcp-core/issues/410) · [Claude Code 插件参考](https://code.claude.com/docs/en/plugins-reference#plugin-components-reference)
>
> **[English](../../api/plugin-manifest.md)**

将 MCP 服务器 URL、技能路径、可选 sub-agent 打包成单个 JSON 清单，Claude Code 用户一键安装即可使用。

**使用场景**

- 向用户交付预配置好的 DCC 集成（`maya-mcp`、`blender-mcp`），无需手工编辑 `claude_desktop_config.json`。
- 和服务器 URL 一起分发精选技能包。
- 对齐上游 MCP [`experimental-ext-skills`](https://github.com/modelcontextprotocol/experimental-ext-skills) 扩展——支持直接由服务器分发技能。

## 导入

```python
from dcc_mcp_core import (
    PluginManifest,
    build_plugin_manifest,
    export_plugin_manifest,
)
```

## `PluginManifest`（dataclass）

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 插件名（如 `"maya-mcp"`） |
| `version` | `str` | 插件版本 |
| `description` | `str` | Claude Code UI 中显示 |
| `mcp_servers` | `list[dict]` | 每项含 `"url"` 和可选 `"headers"` |
| `skills` | `list[str]` | 技能目录绝对路径 |
| `sub_agents` | `list[dict]` | 可选 sub-agent 定义（默认 `[]`） |

方法：

- `.to_dict() -> dict` — JSON 可序列化字典
- `.to_json(indent=2) -> str` — 格式化 JSON 字符串

## `build_plugin_manifest(dcc_name, mcp_url, skill_paths=None, *, version="0.1.0", description=None, api_key=None, extra_mcp_servers=None, sub_agents=None) -> dict`

组装清单字典。

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `dcc_name` | `str` | — | DCC 标识，用作插件名 `<dcc>-mcp` |
| `mcp_url` | `str \| None` | — | MCP 端点 URL；`None` 生成仅技能的包 |
| `skill_paths` | `list[str] \| None` | `None` | 纳入的目录；不存在的会被 DEBUG 过滤 |
| `version` | `str` | `"0.1.0"` | 插件版本 |
| `description` | `str \| None` | 自动 | `None` 时根据 `dcc_name` 生成 |
| `api_key` | `str \| None` | `None` | 注入首个 server 的 `headers.Authorization` 为 `Bearer <key>` |
| `extra_mcp_servers` | `list[dict] \| None` | `None` | 额外 MCP server 条目 |
| `sub_agents` | `list[dict] \| None` | `None` | sub-agent 定义 |

返回符合 Claude Code 插件 Schema 的 JSON 可序列化字典。INFO 级日志汇总写入服务器/技能数量。

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

把清单字典写入磁盘，父目录自动创建，返回解析后的 `pathlib.Path`。

## 推荐用法 — `DccServerBase.plugin_manifest()`

基于 `DccServerBase` 构建时，使用 #410 新增的便捷方法自动从运行中的服务器取回 `mcp_url` 与 `skill_paths`：

```python
class MayaServer(DccServerBase):
    def __init__(self):
        super().__init__(dcc_name="maya", http_config=McpHttpConfig(port=8765))

server = MayaServer()
handle = server.start()
manifest = server.plugin_manifest(version="1.2.0")   # dict
```

## 清单结构

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

## 参见

- [远程服务器指南](../guide/remote-server.md)
- [Skills 技能包](../guide/skills.md) — SKILL.md 发现机制与 `DCC_MCP_SKILL_PATHS`
- [Claude Code 插件组件参考](https://code.claude.com/docs/en/plugins-reference#plugin-components-reference)
