# Batch Dispatch — 批量工具调用与 Eval 沙箱

> 源码：[`python/dcc_mcp_core/batch.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/batch.py) · Issue [#406](https://github.com/loonghao/dcc-mcp-core/issues/406)
>
> **[English](../../api/batch.md)**

服务端批量执行工具调用，减少 Agent 往返和 Token 消耗。中间结果**不会**进入模型上下文——只有最终聚合值返回。

**何时使用**

- **`batch_dispatch`** — 已知要连续调用的 N 个工具；只关心合并后的汇总，不希望逐步响应进入上下文。
- **`EvalContext`** — Agent 写一段短 Python 脚本，根据中间结果条件选择工具（"Cloudflare 模式"）。配合 [`DccApiExecutor`](./dcc-api-executor.md) 覆盖大型 DCC API。

两者均为**纯 Python**，在 Rust 级 `tools/batch` MCP 端点落地前即可使用任意 `ToolDispatcher`。

## 导入

```python
from dcc_mcp_core import batch_dispatch, EvalContext
```

## `batch_dispatch(dispatcher, calls, *, aggregate="list", stop_on_error=False) -> dict`

按顺序执行 `(tool_name, arguments_dict)` 列表，返回单一聚合汇总。

**参数**

| 名称 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `dispatcher` | `ToolDispatcher` | — | 需暴露 `.dispatch(name, json_str) -> dict` |
| `calls` | `list[tuple[str, dict]]` | — | 有序 `(tool_name, args)` 列表 |
| `aggregate` | `"list" \| "merge" \| "last"` | `"list"` | 详见下表 |
| `stop_on_error` | `bool` | `False` | 为 `True` 时首次失败即中止 |

**聚合模式**

| 模式 | 结果键 | 形态 |
|------|--------|------|
| `"list"` | `"results"` | 单次调用返回值列表 |
| `"merge"` | `"merged"` | 合并每次的 `output` 字典（后者覆盖前者） |
| `"last"` | `"last"` | 仅最后一次结果 |

**返回值**始终包含 `total`、`succeeded`、`errors`，以及按 `aggregate` 附加的 `results` / `merged` / `last` 之一。

```python
summary = batch_dispatch(
    dispatcher,
    [
        ("get_scene_objects", {}),
        ("get_render_stats", {"layer": "beauty"}),
        ("get_render_stats", {"layer": "specular"}),
    ],
    aggregate="merge",
)
print(summary["total"], summary["succeeded"])
print(summary["merged"])
```

## `EvalContext(dispatcher, *, sandbox=True, timeout_secs=30)`

带 `dispatch(name, args)` 的沙箱脚本执行环境——对应规划中的 `dcc_mcp_core__eval` MCP 内建工具。交给 Agent 一个受限的 Python 解释器，在循环中编排几十次工具调用，而不让中间值进入上下文。

| 参数 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `dispatcher` | `ToolDispatcher` | — | |
| `sandbox` | `bool` | `True` | 从 `__builtins__` 中剥离 `open`、`exec`、`eval`、`__import__`、`compile`、`getattr/setattr/delattr`、`vars/dir`、`globals/locals` |
| `timeout_secs` | `int \| None` | `30` | 仅 POSIX（`signal.SIGALRM`）；Windows 下自动跳过 |

**方法 `.run(script: str) -> Any`**

- 会把脚本包装成函数体，使顶层 `return <expr>` 生效。
- 最后一行表达式不会隐式返回（与 REPL 不同）。
- 超时抛 `TimeoutError`（仅 POSIX），其他异常抛 `RuntimeError`。
- 脚本内可使用 `dispatch(tool_name, args_dict)` 调用任意已注册工具。

```python
ctx = EvalContext(dispatcher)
keyframes = ctx.run("""
frames = []
for i in range(1, 11):
    r = dispatch("get_frame_data", {"frame": i})
    if r.get("output", {}).get("has_keyframe"):
        frames.append(i)
return frames
""")
# 仅最终列表回到 Agent——10 次调用只消耗 1 次返回的 Token 量。
```

## 安全注意

- `sandbox=True` 是**尽力而为**——隐藏了危险 builtin 但不是 OS 级隔离。可信度视作"自家 Agent 生成的半可信代码"，而非任意用户输入。
- 处理不可信输入时请配合 [`SandboxPolicy`](./sandbox.md) 并运行于子进程/容器。
- 脚本里工具调用失败不会抛异常——返回标准错误字典，便于脚本按惯用方式处理。

## 落地现状

Python 助手已可用。Rust 级 `tools/batch` 与 `dcc_mcp_core__eval` 内建 MCP 工具跟踪于 issue [#406](https://github.com/loonghao/dcc-mcp-core/issues/406)，将复用本模块逻辑。

## 参见

- [`DccApiExecutor`](./dcc-api-executor.md) — 使用 `EvalContext` 的 2 工具包装
- [远程服务器指南](../guide/remote-server.md)
- [沙箱与安全](./sandbox.md)
