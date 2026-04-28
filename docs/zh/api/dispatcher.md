# 可调用对象调度器 API

> **[English](../../api/dispatcher.md)**

DCC 中立的协议，用于将 Python skill 脚本路由到宿主程序的 **UI / 主线程**，
配套提供面向嵌入式 DCC 的声明式 **最小化加载模式**（issues #520、#521、#525）。

每一个嵌入式 DCC 插件（Maya、Houdini、Unreal、Blender Python …）都在重复实现
同一套模式：接收 MCP `tools/call` → 把脚本派发到宿主事件循环 → 返回 JSON 序列化的结果。
本模块把这套契约抽出来，每个适配器只需要补充宿主特定的胶水代码。

**导出符号：** `BaseDccCallableDispatcher`、`BaseDccCallableDispatcherFull`、
`BaseDccPump`、`InProcessCallableDispatcher`、`JobEntry`、`JobOutcome`、
`PendingEnvelope`、`DrainStats`、`PumpStats`、`current_callable_job`、
`MinimalModeConfig`、`build_inprocess_executor`、`run_skill_script`。

## 选用指南

| 需求 | 使用 |
|------|----------|
| 仅需要一个 `dispatch_callable(func, *args, **kwargs)` 入口 | `BaseDccCallableDispatcher`（#521） |
| 完整的 submit / cancel / shutdown 协议 | `BaseDccCallableDispatcherFull`（#520） |
| 由空闲 tick 主动 drain 队列（如 Maya `scriptJob(event=['idle', …])`） | `BaseDccPump`（#520） |
| `mayapy` / pytest / batch 场景下的单线程参考实现 | `InProcessCallableDispatcher` |
| 在 skill 脚本中获取每任务的取消句柄 | `current_callable_job` ContextVar |
| 启动期声明式渐进加载 skill | `MinimalModeConfig`（#525） |

## BaseDccCallableDispatcher（最小协议）

```python
from dcc_mcp_core import BaseDccCallableDispatcher

class MyDispatcher:  # duck typing 即可；显式继承可走 isinstance() 校验
    def dispatch_callable(self, func, *args, **kwargs):
        # 把 (func, args, kwargs) 推到宿主 UI 线程队列并阻塞等待返回。
        # Maya 示例：
        #   from maya import utils
        #   return utils.executeInMainThreadWithResult(lambda: func(*args, **kwargs))
        ...
```

唯一契约方法 `dispatch_callable(func, *args, **kwargs) -> Any` —
设计上保持窄面，最简单的宿主一行胶水即可满足。
需要取消能力时使用下面的 *Full* 协议。

## BaseDccCallableDispatcherFull（带取消）

```python
from dcc_mcp_core import (
    BaseDccCallableDispatcherFull, JobOutcome, PendingEnvelope,
)

class HostDispatcher:
    def submit_callable(self, request_id, task, affinity="main", timeout_ms=None) -> JobOutcome: ...
    def submit_async_callable(self, request_id, task, *, affinity="main", timeout_ms=None,
                              progress_token=None, on_complete=None) -> PendingEnvelope: ...
    def cancel(self, request_id: str) -> bool: ...
    def shutdown(self, reason: str = "Interrupted") -> int: ...
```

| 方法 | 用途 |
|--------|---------|
| `submit_callable` | 同步提交并阻塞等待 `JobOutcome` |
| `submit_async_callable` | 立即返回 `PendingEnvelope`，结果通过 `on_complete` 回调送达 |
| `cancel(request_id)` | 设置在飞任务的 `cancel_flag`；找到时返回 `True` |
| `shutdown(reason)` | 取消所有在飞任务，返回取消数量 |

`affinity` 取 `Literal["main", "any"]` —
实现可自由忽略 `"any"` 而始终在主线程执行。
`timeout_ms=None` 表示无超时（失控脚本的处理由宿主自行定义）。

## BaseDccPump（合作式 drain）

宿主自身负责从 idle/tick 回调中 drain 队列时，应实现 `drain_queue(budget_ms) -> DrainStats`
方法和 `stats` 属性：

```python
from dcc_mcp_core import BaseDccPump, DrainStats, PumpStats

class MyPump:
    def drain_queue(self, budget_ms: int) -> DrainStats: ...
    @property
    def stats(self) -> PumpStats: ...
```

`DrainStats(drained, elapsed_ms, overrun)` 报告单次 tick；
`PumpStats(ticks, drained, overrun_cycles)` 是累积统计。

## InProcessCallableDispatcher（参考实现）

```python
from dcc_mcp_core import InProcessCallableDispatcher, build_inprocess_executor

dispatcher = InProcessCallableDispatcher()
executor = build_inprocess_executor(dispatcher)
# 把 executor 传给 McpHttpServer.set_in_process_executor / DccServerBase.register_inprocess_executor。

# mayapy / batch 的独立兜底——内联执行脚本：
inline_executor = build_inprocess_executor(None)
```

生产级宿主调度器（Maya UI 线程、Houdini `hou.session` …）一般继承
`InProcessCallableDispatcher` 并覆写 `submit_callable`，把任务塞进宿主主线程队列
而不是内联执行。`cancel`、`shutdown` 以及每任务 `JobEntry` 簿记直接复用。

## 每任务取消

`current_callable_job` 是 `contextvars.ContextVar[JobEntry | None]`，由
`InProcessCallableDispatcher` 在每次提交任务时设置。
skill 脚本无需依赖 MCP request 上下文即可轮询：

```python
from dcc_mcp_core import current_callable_job, check_dcc_cancelled

def main(frames):
    for frame in frames:
        check_dcc_cancelled()      # 同时尊重 MCP token 和 callable-job 标志
        render_frame(frame)
```

`check_dcc_cancelled()`（取消 API，#522）已经把 `current_callable_job`
和 MCP `CancelToken` 串联在一起 —— **优先使用它**而不是手写探针。

## MinimalModeConfig（声明式启动）

```python
from dcc_mcp_core import MinimalModeConfig

CONFIG = MinimalModeConfig(
    skills=("scene_inspector", "render_queue"),     # 启动期完整加载
    deactivate_groups={"render_queue": ("submit",)}, # 但禁用 `submit` 工具组
    env_var_minimal="DCC_MCP_MINIMAL",               # 假值 → 加载所有发现的 skill
    env_var_default_tools="DCC_MCP_DEFAULT_TOOLS",   # 逗号/空格分隔覆盖
)
```

由 `DccServerBase.register_builtin_actions` 按以下顺序解析：

1. `env_var_default_tools` 已设且非空 → **仅**加载这些 skill。
2. `env_var_minimal` 设为 `"0" / "false" / "no" / "off" / ""` → 加载所有发现的 skill。
3. 否则 → 加载 `skills` 并应用 `deactivate_groups`。

参见 [服务器工厂 API](./factory.md) 了解如何把 `MinimalModeConfig`
和 in-process executor 串入 `register_builtin_actions`。
