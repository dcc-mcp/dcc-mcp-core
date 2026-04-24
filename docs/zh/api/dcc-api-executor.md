# DCC API Executor — 面向巨型 API 的代码编排模式

> 源码：[`python/dcc_mcp_core/dcc_api_executor.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/dcc_api_executor.py) · Issue [#411](https://github.com/loonghao/dcc-mcp-core/issues/411)
>
> **[English](../../api/dcc-api-executor.md)**

把 "Cloudflare 模式" 引入 DCC：只暴露**两个工具**——`dcc_search` 和 `dcc_execute`——二者合计约 500 token，即可覆盖整个 DCC Python API，而不必为 1500+ 个命令各注册一个 MCP 工具。

**为什么 DCC 尤其需要这种模式**

| DCC | 大致 API 规模 |
|-----|---------------|
| Maya | 2000+ MEL / Python 命令 |
| Houdini | 1500+ `hou` 方法 |
| Blender | 800+ `bpy` 操作符 |
| 3ds Max | 1000+ `pymxs` 命令 |

逐个注册会把 `tools/list` 撑爆 Agent 上下文。代码编排把工具面恒定在：**2 个工具，任意 DCC，任意规模**。

> 参考：Anthropic《Building agents that reach production systems with MCP》（2026-04-22）——"当工具面很大时要按代码编排设计。Cloudflare 的 MCP server 是参考范例：两个工具（search/execute）在大约 1K token 内覆盖 2500 个端点。"

## 导入

```python
from dcc_mcp_core import (
    DccApiCatalog,
    DccApiExecutor,
    register_dcc_api_executor,
)
```

## `DccApiCatalog(dcc_name, commands=None, catalog_text=None)`

可搜索的 DCC 命令目录，用于 `dcc_search`。

**来源**

1. `commands` — 显式列表：`[{"name": "polyCube", "signature": "...", "description": "..."}]`
2. `catalog_text` — 纯文本目录，一行一条：`name - description`。空行与 `#` 注释自动忽略。
3. `add_command(name, *, signature="", description="")` — 运行期追加。

**搜索 `search(query: str, *, limit: int = 10) -> list[dict]`**

- 按空白/标点切词
- 丢弃停用词（`the`、`a`、`an`、`in`、`for`、`of`）
- 按 `name + description + signature` 命中计数打分
- Top-`limit`，分数同则按名称字典序
- 零分项不返回

`len(catalog)` 返回命令条数。

## `DccApiExecutor(dcc_name, catalog=None, dispatcher=None)`

2 工具包装器。

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `dcc_name` | `str` | — | DCC 标识 |
| `catalog` | `DccApiCatalog \| None` | 空目录 | `dcc_search` 使用 |
| `dispatcher` | `ToolDispatcher \| None` | `None` | 提供时 `dcc_execute` 脚本内可用 `dispatch(name, args)` |

### `.search(query, *, limit=10) -> dict`

处理 `dcc_search` 调用：

```json
{
  "success": true,
  "message": "Found 3 command(s) matching 'create sphere'.",
  "results": [
    {"name": "polySphere", "signature": "...", "description": "..."}
  ]
}
```

无匹配时返回 `"results": []` 并附带建议使用 `search_skills` 的 `"hint"`。

### `.execute(code, *, timeout_secs=30) -> dict`

处理 `dcc_execute` 调用。脚本在 [`EvalContext`](./batch.md) 中以 `sandbox=True` 执行；构造时传入 `ToolDispatcher` 后脚本可使用 `dispatch()`。

可能返回：

- `{"success": True, "output": <脚本 return 值>, "message": "..."}`
- `{"success": False, "error": "...", "message": "Script timed out ..."}`
- `{"success": False, "error": "...", "message": "Script failed ..."}`

`output` 为脚本顶层 `return` 返回的值（最后一行表达式不会隐式返回）。

## `register_dcc_api_executor(server, executor, *, search_tool_name="dcc_search", execute_tool_name="dcc_execute") -> None`

在 `server.start()` **之前**在 `McpHttpServer` 上注册这两个工具。工具名可覆盖，用于多 DCC gateway 消歧（`maya_search`、`blender_search`）。

## 端到端示例

```python
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher,
    McpHttpServer, McpHttpConfig,
    DccApiCatalog, DccApiExecutor, register_dcc_api_executor,
)

registry = ToolRegistry()
dispatcher = ToolDispatcher(registry)

catalog = DccApiCatalog(
    "maya",
    catalog_text="""
polyCube - Create a cube polygon mesh
polySphere - Create a sphere polygon mesh
select - Select nodes in the scene
render - Render the current frame
""",
)

executor = DccApiExecutor("maya", catalog=catalog, dispatcher=dispatcher)

server = McpHttpServer(registry, McpHttpConfig(port=8765))
register_dcc_api_executor(server, executor)
handle = server.start()
# tools/list 现在恰好包含 2 个条目：dcc_search、dcc_execute
```

## Agent 典型使用流

```text
User  : "创建 5 个球体排成一行"
Agent : dcc_search({"query": "create sphere"})         → 命中 polySphere
Agent : dcc_execute({"code": "..."} )
        for i in range(5):
            dispatch("polySphere", {"position": [i, 0, 0]})
        return {"created": 5}
Server: { "output": {"created": 5}, "success": true }
```

只有最终脚本返回值进入模型——5 次中间 dispatch 不消耗 Token。

## 当前状态

Python 助手已可用。Rust 级 `dcc_search` / `dcc_execute` 内建 MCP 工具跟踪于 issue [#411](https://github.com/loonghao/dcc-mcp-core/issues/411)。之前通过 `register_dcc_api_executor(server, executor)` 注册——Python 处理器行为完全一致。

## 参见

- [`EvalContext` / `batch_dispatch`](./batch.md) — 底层沙箱执行器
- [Skills 技能包](../guide/skills.md) — `search-hint` 如何从 SKILL.md 填充目录
- [远程服务器指南](../guide/remote-server.md)
