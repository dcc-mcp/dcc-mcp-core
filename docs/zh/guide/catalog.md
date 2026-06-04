# DCC-MCP 公共目录

DCC-MCP 目录是一个由社区维护的 DCC-MCP 生态系统适配器和技能包注册表（issue #774）。

## 目录文件格式

仓库根目录的 `dcc-mcp-catalog.yml`：

```yaml
version: "1"
entries:
  - name: dcc-mcp-maya-skills
    description: "DCC-MCP 官方 Maya 技能包"
    dcc: [maya]
    url: "https://github.com/example/dcc-mcp-maya-skills"
    tags: [skills, maya, official]

  - name: dcc-mcp-blender-skills
    description: "社区 Blender 技能包"
    dcc: [blender]
    url: "https://github.com/example/dcc-mcp-blender-skills"
    tags: [skills, blender, community]

  - name: dcc-mcp-houdini-adapter
    description: "DCC-MCP 的 Houdini 适配器"
    dcc: [houdini]
    url: "https://github.com/example/dcc-mcp-houdini"
    tags: [adapter, houdini, official]
```

## 条目字段

| 字段 | 必填 | 类型 | 说明 |
|------|------|------|------|
| `name` | ✅ | string | 唯一标识符（推荐 kebab-case） |
| `description` | ✅ | string | 一句话人类可读描述 |
| `dcc` | ✅ | list[string] | 支持的 DCC 类型列表（如 `[maya, blender]`） |
| `url` | ✅ | string | 仓库或文档 URL |
| `tags` | ❌ | list[string] | 可搜索标签（如 `skills`、`adapter`、`official`、`community`） |

## CLI 用法

```bash
# 按关键词搜索（匹配 name、description、DCC 类型或 tag）
dcc-mcp-server catalog search --query maya

# 不指定 query 时列出所有条目
dcc-mcp-server catalog search

# 按精确名称查看详情
dcc-mcp-server catalog describe --name dcc-mcp-maya-skills
```

输出为 JSON 格式，便于解析。

## MCP 资源用法

网关将 catalog 暴露为 MCP **资源**（#813 phase 2），通过 `resources/read` 读取：

```python
# 全量索引，可选 ?query=... 关键词过滤
result = client.resources_read("gateway://catalog?query=blender")
# 返回：{ "total": N, "query": "blender", "entries": [{"name": "...", "description": "...", "dcc": [...], "url": "...", "tags": [...]}] }

# 按精确名查单条
result = client.resources_read("gateway://catalog/dcc-mcp-blender-skills")
# 返回：单个条目，未找到时返回 `-32002` error
```

## 可选文档连接器

Catalog 条目也可以指向只读文档 MCP server。这类条目只是发现提示，不会让
gateway 在启动时自动启用远程连接器。

Autodesk Product Help 按独立文档后端建模，不属于 Maya、Houdini、
Photoshop 或 pipeline adapter：

```json
{
  "mcpServers": {
    "autodesk-product-help": {
      "url": "https://developer.api.autodesk.com/knowledge/public/v1/mcp"
    }
  }
}
```

这个连接器使用 `tags: [docs, autodesk, read-only, infrastructure]`。
文档查询应和 `pipeline` / `shotgrid` 搜索分开，避免生产跟踪工具和产品帮助结果
互相竞争。

Studio note：公共文档 MCP server 应视为可选互联网依赖。Autodesk Product
Help 适合 best-effort 参考查询；如果 studio 要求离线运行、固定版本文档或正式
服务保障，应保持禁用，并把 agent 路由到内部批准的文档源。

## 自定义目录路径

覆盖默认的 `dcc-mcp-catalog.yml` 位置：

```bash
DCC_MCP_CATALOG_PATH=/path/to/my-catalog.yml dcc-mcp-server ...
```

## 搜索行为

- 对 `name`、`description`、`dcc`、`tags` 进行大小写不敏感的子串匹配
- 空 query 返回所有条目
- `describe` 需要精确匹配 `name`（区分大小写）

## 参见

- [skills.md](skills.md) — 如何编写技能包
- [mcp-skills-integration.md](mcp-skills-integration.md) — 在服务器上注册技能
- [rez-skill-packages.md](rez-skill-packages.md) — 用于分发技能的 Rez 包布局
