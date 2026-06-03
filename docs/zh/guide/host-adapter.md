# 编写 DCC 宿主适配器

> **目标读者**：构建 DCC 集成仓库的开发者
> （`dcc-mcp-blender`、`dcc-mcp-maya`、`dcc-mcp-photoshop`、
> `dcc-mcp-unreal` 或新的集成仓库）。
>
> **摘要**：子类化 [`dcc_mcp_core.host.HostAdapter`][HostAdapter]，
> 实现 3 个方法，接线一个入口点。基类处理其余的一切——生命周期、上下文管理器、自适应时钟间隔以及交互式/后台分离。

本指南假设你已经理解主线程亲和性的重要性——如果不了解，请先阅读 [`dcc-thread-safety.md`][thread-safety]。

## 3-hook 契约

`HostAdapter` 要求每个子类恰好实现三个方法。

| Hook | 用途 | 调用时机 |
|------|------|---------|
| `is_background() -> bool` | DCC 是否以无头模式运行？ | 每次 `start()` 调用时执行一次 |
| `attach_tick(tick_fn)` | 将 `tick_fn` 注册到 DCC 的原生空闲原语 | 交互模式下 `start()` 期间执行一次 |
| `detach_tick()` | 撤销 `attach_tick`——必须幂等 | `stop()` 期间 |

**不要**覆盖 `start`、`stop`、`run_headless`、`is_running`、`__enter__` 或 `__exit__`。这些方法编排这 3 个 hook，必须在所有适配器中保持一致，以便调用者可以互换使用它们（LSP）。

## 最简子类

```python
from dcc_mcp_core.host import HostAdapter


class BlenderHost(HostAdapter):
    def is_background(self) -> bool:
        import bpy
        return bpy.app.background

    def attach_tick(self, tick_fn):
        import bpy
        # 返回 ``tick_fn`` 以便每次定时器触发时重用同一个可调用对象，
        # 这样 `detach_tick` 可以找到并注销它。
        bpy.app.timers.register(tick_fn, first_interval=0.0, persistent=True)
        self._tick_fn = tick_fn

    def detach_tick(self) -> None:
        import bpy
        fn = getattr(self, "_tick_fn", None)
        if fn is not None and bpy.app.timers.is_registered(fn):
            bpy.app.timers.unregister(fn)
        self._tick_fn = None
```

完成。这就是整个适配器。其余所有内容——panic 处理、停止时的分发器关闭、"等待最多 5 秒让无头线程加入"的保障、队列繁忙时返回 0 秒/空闲时返回 0.5 秒的自适应间隔——都在基类中。

## 接线到 MCP 服务器

适配器**驱动**分发器；它不拥有分发器。入口点同时拥有两者：

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry
from dcc_mcp_core.host import BlockingDispatcher

# 1. 构建服务器
reg = ToolRegistry()
cfg = McpHttpConfig(port=18765, server_name="blender")
server = McpHttpServer(reg, cfg)

# 2. 创建分发器。BlockingDispatcher 适用于 --background DCC；
#    QueueDispatcher 适用于 GUI 会话。两者均被 HostAdapter、
#    McpHttpServer.attach_dispatcher 和 StandaloneHost 接受（实践中的 LSP）。
#    如果自定义 dispatcher 只需要类型契约，请从 dcc_mcp_core.host
#    导入公开的 TickableDispatcher 协议；不要直接导入私有
#    host protocol 模块。
dispatcher = BlockingDispatcher()
server.attach_dispatcher(dispatcher)

# 3. 启动服务器。立即返回——只绑定端口并生成 tokio 运行时。
handle = server.start()

# 4. 用适配器驱动分发器。
host = BlenderHost(dispatcher)
if host.is_background():
    host.run_headless()   # 阻塞直到关闭
else:
    host.start()          # 非阻塞；立即返回
```

到达 HTTP 端口的每个 `tools/call` 现在都会被投入分发器，并在驱动 `host._tick` 的任何线程上执行——即交互模式下的 DCC 主线程，或无头模式下的 `run_headless` 线程。处理器永远不会看到 tokio 工作线程。

## Maya 示例

```python
class MayaHost(HostAdapter):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self._script_job = None

    def is_background(self) -> bool:
        import maya.cmds as cmds
        return cmds.about(batch=True)

    def attach_tick(self, tick_fn):
        import maya.cmds as cmds
        # `idleEvent` 在 UI 空闲时触发——原生主线程。
        # 用 lambda 包装使 `tick_fn` 的返回值被丢弃
        # （scriptJob 不关心下一个间隔）。
        self._script_job = cmds.scriptJob(
            idleEvent=lambda: tick_fn(),
        )

    def detach_tick(self) -> None:
        import maya.cmds as cmds
        if self._script_job is not None and cmds.scriptJob(
            exists=self._script_job,
        ):
            cmds.scriptJob(kill=self._script_job)
        self._script_job = None
```

Maya 的 `idleEvent` 比 Blender 的定时器触发更频繁，因此默认的 `tick_interval_idle=0.5` 足够保守。如果 CPU 使用率过高，将 `tick_interval_idle` 提高到 `1.0`。

## 仅无头模式的 DCC（ExtendScript、MaxScript）

当 DCC 没有可从 Python 调用的空闲原语（Adobe Photoshop 的 ExtendScript、3ds Max 2022 之前的 MAXScript 桥接……）时，以完全无头模式运行：

```python
class PhotoshopHost(HostAdapter):
    def is_background(self) -> bool:
        return True  # 始终无头——无 ExtendScript UI 空闲 hook

    def attach_tick(self, tick_fn):
        # 永远不会被调用（is_background 始终为 True）。
        raise NotImplementedError(
            "PhotoshopHost 始终无头；run_headless 是唯一路径",
        )

    def detach_tick(self) -> None:
        pass  # 空操作；没有任何内容被附加
```

入口点随后无条件调用 `host.run_headless()`。

## 可替换性测试

每个行为良好的子类都应通过相同的契约测试，这本质上就是 `tests/test_host_adapter.py::test_subclass_overriding_hooks_drives_dispatcher` 已在假子类上演练的内容。将其复制到你的仓库，替换为真实子类，你就有了一个 CI 门控：

```python
def test_my_host_drives_dispatcher(live_dcc_fixture):
    dispatcher = QueueDispatcher()
    host = MyDccHost(dispatcher)
    with host:
        result = dispatcher.post(lambda: 42).wait(timeout=5.0)
    assert result == 42
```

## 开设 DCC 集成仓库时的检查清单

- [ ] 子类化 `HostAdapter`，实现 3 个 hook
- [ ] 附带至少一个示例技能（单个工具即可），证明 `bpy.ops` / `maya.cmds` / 等效函数可在主线程上工作
- [ ] 添加 CI 作业：以无头模式启动 DCC，对实时服务器运行 `mcpcall` 调用，并断言成功
- [ ] 编写 `README.md` 指回本文档，使未来维护者理解契约
- [ ] 在你的仓库中开一个跟踪 issue；交叉引用核心的 [总揽 issue][umbrella]，使进度跨仓库可见

[HostAdapter]: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/python/dcc_mcp_core/host/_adapter.py
[thread-safety]: ./dcc-thread-safety.md
[umbrella]: https://github.com/dcc-mcp/dcc-mcp-core/issues/690
