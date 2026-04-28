# DCC 线程安全

> **适用对象**：适配器作者（`dcc-mcp-maya`、`dcc-mcp-blender` 等）以及在 DCC 宿主中运行长耗时计算的 skill 作者。
>
> **一句话总结**：任何修改场景的调用都必须在 DCC 的**主线程**上执行。
> `DeferredExecutor` 是 Tokio HTTP 工作线程与主线程之间**唯一被官方支持**的桥梁。
> 长耗时任务必须被切分为每帧（per-tick）的小块，并且必须使用 `poll_pending_bounded(max=N)`，
> 不要使用 `poll_pending()`。
>
> 如果是 Python 端的派发场景（在宿主 UI 线程上执行整段 skill *脚本* 而不仅仅是
> 一段 Rust callable），见 [可调用对象调度器 API](../api/dispatcher.md) ——
> 它与 `DeferredExecutor` 互补，是每个嵌入式 Python 适配器
> （`mayapy`、`hython`、`unreal-python`）应当继承的基础协议。

## 为什么主线程亲和性是强制的

所有主流 DCC 宿主都要求场景修改 API 只能在主线程上调用。运行时**不会**用锁保护你 ——
从工作线程调用这些 API 会破坏场景状态、让宿主崩溃，或者静默失败。每个 DCC 都提供了
一个标准的"派发到主线程"原语：

| DCC | 仅主线程可用的 API | 官方派发原语 |
|------|-----------------------|---------------------------|
| Maya | `maya.cmds`、`OpenMaya`、`pymel` | `maya.utils.executeDeferred(fn)` / `maya.cmds.evalDeferred("expr")` |
| Blender | `bpy.ops`、`bpy.data`、`bpy.context` | `bpy.app.timers.register(fn, first_interval=0.0)` |
| Houdini | `hou.*`（场景图、SOP、HDA） | `hou.ui.addEventLoopCallback(fn)` |
| 3ds Max | `MaxPlus`、`pymxs.runtime`、MAXScript | `pymxs.runtime.execute("...")` 只能在主线程调用（无 defer 原语，改用 Qt singleshot timer） |

这些原语共享相同的契约：

1. 可调用对象被入队。
2. DCC 事件循环在下一个安全时机从主线程调用它。
3. 可调用对象**同步**运行直到返回，并**阻塞 UI**。

第 (3) 点就是本指南存在的理由：一旦你的回调超过约 16 ms，DCC UI 就会卡顿；超过几百
毫秒，宿主就会看起来"冻住"。

## `DeferredExecutor` 如何桥接 Tokio 工作线程到主线程

`McpHttpServer` 在 Tokio 工作线程上接收 HTTP 请求。工作线程**不能**直接调用场景 API ——
它必须通过 `DeferredExecutor` 提交任务；后者把任务塞进一个 `mpsc::channel` 并返回
一个 future。DCC 事件循环在主线程上消费这个通道。

```text
   Tokio 工作线程 (HTTP handler)              DCC 主线程
   ───────────────────────────                ─────────────────
   handle.execute(task) ──── mpsc::channel ──► poll_pending_bounded(max=8)
           │                   （有界）                │
           │                                           │ 运行 task_fn()
           ▼                                           ▼
   await oneshot ◄──────────── oneshot::channel ── send(result)
```

该桥接的 Rust 权威源码简短到可以一次读完：

```rust
// crates/dcc-mcp-http/src/executor.rs (L23-L111)
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A boxed async-compatible task that runs on the DCC main thread.
pub type DccTaskFn = Box<dyn FnOnce() -> String + Send + 'static>;
// ...
impl DeferredExecutor {
    pub fn poll_pending(&mut self) -> usize { /* drain all */ }
    pub fn poll_pending_bounded(&mut self, max: usize) -> usize { /* drain <= max */ }
}
```

上层适配器实现的 job-dispatcher 层位于：

```rust
// crates/dcc-mcp-process/src/dispatcher.rs (L1-L166)
pub enum ThreadAffinity { Main, Named(&'static str), Any }
pub struct JobRequest { /* request_id, affinity, timeout_ms, task */ }
pub trait HostDispatcher {
    fn submit(&self, req: JobRequest) -> oneshot::Receiver<ActionOutcome>;
    fn supported(&self) -> &[ThreadAffinity];
    fn capabilities(&self) -> HostCapabilities;
}
```

当一个 skill 工具被标记为 `ThreadAffinity::Main` 时，适配器会把它路由到
`DeferredExecutor`；`ThreadAffinity::Any` 的任务则直接在 Tokio 工作线程上执行。

### Python 用法

```python
from dcc_mcp_core._core import DeferredExecutor  # 暂未进入公共 __init__

executor = DeferredExecutor(capacity=16)

# 从任意线程（例如 MCP HTTP 处理器）：
executor.execute(lambda: maya.cmds.polySphere(radius=1.0))

# 在 DCC 空闲回调中：
executor.poll_pending_bounded(max=8)  # 每帧有界 —— 见下文
```

## 长耗时任务的规则

"长耗时任务"是指无法在单个 DCC tick（60 FPS 约 16 ms，30 FPS 约 33 ms）内完成的
工作。典型例子：playblast、批量渲染、几千次场景图编辑、USD 合成、复杂几何生成。

三条不可妥协的规则：

### 1. 把工作切分为每帧小块

每个 timer tick 渲染一帧，而不是一次调用里渲染全部帧。每批处理 N 个图元，而不是
处理整个网格。每块大小应至少给 DCC 留出 50% 的 tick 预算用于 UI。

```python
# 好：Blender timer 每帧渲染一帧
frame_iter = iter(range(1, 241))

def render_next():
    try:
        frame = next(frame_iter)
    except StopIteration:
        return None  # 注销 timer
    bpy.context.scene.frame_set(frame)
    bpy.ops.render.render(write_still=True)
    return 0.0  # 立即重新调度

bpy.app.timers.register(render_next)
```

### 2. 协作式检查点

在每块之间检查取消标志，并把控制权交还给 DCC。参见
[issue #329 — `check_cancelled()`](https://github.com/loonghao/dcc-mcp-core/issues/329)
了解计划中的协作式取消原语。

```python
for batch in chunks(primitives, size=500):
    if job.check_cancelled():           # #329
        return skill_error("被用户取消")
    create_primitives(batch)
    # 在 batch 之间控制权会回到 DCC
```

### 3. 使用 `poll_pending_bounded(max=N)`，不要用 `poll_pending()`

`poll_pending()` 在返回前会排空**所有**已排队任务 —— 如果同时有 50 个任务到达，
DCC 就会冻结它们运行时长之和的时间。`poll_pending_bounded(max=N)` 限制每次 pump
最多处理 `N` 个任务，从而让事件循环在批次之间有机会重绘。

```python
# ❌ 不好 —— 无上限；任务突发会冻结 UI
executor.poll_pending()

# ✅ 好 —— 有界；每 tick 最多 8 个，最差延迟可控
executor.poll_pending_bounded(max=8)
```

60 FPS 下合理的起点是 `max=8`；如果单个任务较重，请调小。

[issue #332 — `@chunked_job`](https://github.com/loonghao/dcc-mcp-core/issues/332)
中计划的分块任务装饰器落地后，会自动编码规则 (1) 和 (2)。

## 禁止的模式

### 在 `DccTaskFn` 中使用 `time.sleep()`

`DccTaskFn` 运行在 DCC 主线程上。`time.sleep(n)` 会阻塞该线程 —— 宿主会冻结 `n` 秒。

```python
# ❌ 会让 Maya 冻结 5 秒
executor.execute(lambda: (time.sleep(5), cmds.polySphere()))
```

如果需要延迟，使用 DCC 自带的 timer 原语（`maya.utils.executeDeferred`、
`bpy.app.timers.register` 等）重新调度，并把控制权还给事件循环。

### 从 skill 脚本里启动原生 OS 线程执行场景操作

启动 `threading.Thread` 并在其中调用 `maya.cmds` / `bpy.ops` 会完全绕过主线程契约。
即使在测试中看起来能工作，在负载下也会段错误或破坏状态。

```python
# ❌ 未定义行为 —— Maya API 不是线程安全的
threading.Thread(target=lambda: cmds.polySphere()).start()
```

请改用 `DeferredExecutor.execute(...)` —— 它是进入场景 API 的唯一线程安全路径。

### 在主线程上做阻塞 I/O

`requests.get(url)`、`urllib.urlopen(...)`、同步数据库调用、大文件读取 —— 都不应
出现在 `DccTaskFn` 中。它们像 `time.sleep` 一样会阻塞事件循环。

```python
# ❌ 会让 DCC UI 冻结一整个 HTTP round-trip
executor.execute(lambda: json.loads(requests.get(url).text))
```

在工作线程里先做 I/O（提交前），然后把已解析的数据传进 `DccTaskFn`：

```python
# ✅ 在 worker 里做 I/O；只把场景调用 defer 到主线程
payload = requests.get(url).json()                   # Tokio worker
executor.execute(lambda: apply_to_scene(payload))    # 主线程
```

## 另请参阅

- [ADR 002 — DCC 主线程亲和性](../../adr/002-dcc-main-thread-affinity.md)
  —— 该设计的架构理由（英文）。
- [快速开始 → DeferredExecutor](./getting-started.md#deferredexecutor-dcc-main-thread-safety)
  —— 最小 "hello world" 示例。
- [`skills/integration-guide.md`](https://github.com/loonghao/dcc-mcp-core/blob/main/skills/integration-guide.md)
  —— 各 DCC 的桥接模式（嵌入式 Python / WebSocket / WebView）。
- [Issue #329 — `check_cancelled()`](https://github.com/loonghao/dcc-mcp-core/issues/329)
  —— 分块任务的协作式取消。
- [Issue #332 — `@chunked_job`](https://github.com/loonghao/dcc-mcp-core/issues/332)
  —— 将分块 + 检查点规则封装进的装饰器。
