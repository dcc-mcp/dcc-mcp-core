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
