# Batch Dispatch — 批量工具调用与 Eval 沙箱

> 源码：[`python/dcc_mcp_core/batch.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/batch.py) · Issue [#406](https://github.com/loonghao/dcc-mcp-core/issues/406)
>
> **[English](../zh/api/batch.md)**（中文本页）

Server-side batch execution for reducing agent round-trips and token usage.
Intermediate results never enter the model context — only the final
aggregated value is returned.

**When to use**

- **`batch_dispatch`** — N tool calls you already know up-front; you only
  want the combined summary, not each step's chatter.
- **`EvalContext`** — the agent writes a short Python script that conditionally
  chooses tools based on intermediate results ("the Cloudflare pattern").
  Pairs with [`DccApiExecutor`](./dcc-api-executor.md) for large DCC APIs.

Both are **pure-Python** — they work with any `ToolDispatcher` even before
the Rust-level `tools/batch` MCP endpoint lands.

## Imports

```python
from dcc_mcp_core import batch_dispatch, EvalContext
```

## `batch_dispatch(dispatcher, calls, *, aggregate="list", stop_on_error=False) -> dict`

Execute `(tool_name, arguments_dict)` pairs sequentially against a
`ToolDispatcher` and return a single aggregated summary.

**Parameters**

| Name | Type | Default | Notes |
|------|------|---------|-------|
| `dispatcher` | `ToolDispatcher` | — | Must expose `.dispatch(name, json_str) -> dict` |
| `calls` | `list[tuple[str, dict]]` | — | Ordered list of `(tool_name, args)` |
| `aggregate` | `"list" \| "merge" \| "last"` | `"list"` | See below |
| `stop_on_error` | `bool` | `False` | Abort on first failure when `True` |

**Aggregation modes**

| Mode | Resulting key | Shape |
|------|---------------|-------|
| `"list"` | `"results"` | List of individual `dispatch` return dicts |
| `"merge"` | `"merged"` | Each result's `output` dict merged (later keys win) |
| `"last"` | `"last"` | Only the final result dict |

**Return value** — always includes:

- `"total"` — number of calls attempted
- `"succeeded"` — number with `output.success != False`
- `"errors"` — list of `{index, tool, error}` for failing calls
- plus one of `results` / `merged` / `last` depending on `aggregate`

**Example**

```python
from dcc_mcp_core import ToolRegistry, ToolDispatcher, batch_dispatch

registry = ToolRegistry()
# ... register tools ...
dispatcher = ToolDispatcher(registry)

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
print(summary["merged"])   # combined output dict
```

## `EvalContext(dispatcher, *, sandbox=True, timeout_secs=30)`

Sandboxed script-execution context with access to `dispatch(name, args)`.

Mirrors the planned `dcc_mcp_core__eval` MCP built-in tool — hands the
agent a restricted Python interpreter that can orchestrate dozens of
tool calls in a loop without each intermediate value reaching the model.

**Constructor**

| Arg | Type | Default | Notes |
|-----|------|---------|-------|
| `dispatcher` | `ToolDispatcher` | — | |
| `sandbox` | `bool` | `True` | Strips `open`, `exec`, `eval`, `__import__`, `compile`, `getattr`, `setattr`, `delattr`, `vars`, `dir`, `globals`, `locals` from `__builtins__` |
| `timeout_secs` | `int \| None` | `30` | POSIX-only (`signal.SIGALRM`); silently ignored on Windows |

**Method: `.run(script: str) -> Any`**

- Wraps the script in a function body so top-level `return <expr>` works.
- The last expression is **not** implicitly returned (unlike a REPL).
- Raises `TimeoutError` when the budget is exceeded (POSIX only).
- Raises `RuntimeError` on any other script exception.
- Inside the script, `dispatch(tool_name, args_dict)` is available.

**Example**

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
# Only the final list reaches the agent — 10 tool calls cost 1 token-worth of output.
```

## Security considerations

- The `sandbox=True` restriction is **best-effort** — it hides dangerous
  builtins but does not provide OS-level isolation. Treat scripts as
  semi-trusted code from your own agent, not arbitrary user input.
- For untrusted input, combine `EvalContext` with
  [`SandboxPolicy`](./sandbox.md) and run inside a subprocess / container.
- Failing tool calls do **not** raise inside a script — they return a
  standard error dict, so scripts handle them idiomatically.

## Integration path

The Python helpers ship today. The Rust-level `tools/batch` and
`dcc_mcp_core__eval` built-in MCP tools are tracked in issue
[#406](https://github.com/loonghao/dcc-mcp-core/issues/406) and will call
through this same logic once implemented.

## See also

- [`DccApiExecutor`](./dcc-api-executor.md) — the 2-tool "search + execute" wrapper that uses `EvalContext`
- [Remote Server guide](../guide/remote-server.md)
- [Sandbox & Security](./sandbox.md)
