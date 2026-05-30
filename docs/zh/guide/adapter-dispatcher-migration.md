# 适配器调度器迁移

当您需要将 DCC 适配器从本地调度器副本迁移到由 #1273、#1274、#1275 和 #1276 引入的共享核心原语时，请使用本指南。目标是让适配器仅保留宿主生命周期胶水代码，而 `dcc-mcp-core` 负责请求验证、队列管理、取消、超时处理、泵生命周期和标准结果信封。

## 决策表

| 适配器形态 | 使用 | 适配器仍需负责 |
|------------|------|----------------|
| 携带 Qt 的 DCC 伴生进程（Maya、Houdini、3ds Max、Nuke、Cinema 4D、Substance Painter、Mari） | `dcc_mcp_core.qt_dispatcher.start_qt_server` 和 `qtserver://` Rust 客户端 | 插件启动/关闭、会话元数据和宿主动作回调 |
| 脚本支撑的伴生进程动作分发 | `SidecarActionDispatcher` 作为 Qt 分发处理器 | `server_provider`、`action_resolver` 和执行器钩子如 `maya_executor(...)` 或 `script_executor(...)` |
| 具有主线程亲和性的交互式 UI 宿主 | `HostUiDispatcherBase` 子类加上 `HostPumpController` | `poke_host_pump()` 和用于宿主定时器原语的小型 `HostPumpTimerAdapter` |
| 非 Qt UI 宿主，具有原生定时器 | `HostUiDispatcherBase` 加上自定义的 `HostPumpTimerAdapter` | 仅定时器安装/卸载/调度映射 |
| 无头或批处理宿主（`mayapy`、`hython`、pytest） | `InProcessCallableDispatcher` 或 `DccServerBase.register_inprocess_executor(None)` | 验证宿主进程是否可安全内联调用 |
| 不运行技能脚本的原生宿主 RPC | 保留 `HostRpcClient` 实现 | 原生协议帧和宿主特定的 RPC 错误 |
| 适配器一致性测试 | `ManualHostTimerAdapter` 和假伴生进程/服务器夹具 | 仅断言适配器特定的元数据和宿主回调接线 |

这些原语可用后，请勿在适配器仓库中复制 `_qt_dispatcher.py`、`qt_bridge.py`、队列实现、取消标志、超时循环或伴生进程载荷验证器。

## 迁移清单

1. 清点本地调度器文件，并使用上述决策表对每个文件进行分类。
2. 使用 `dcc_mcp_core.qt_dispatcher.start_qt_server(...)` 替换 JSON 行 Qt 服务器副本。
3. 为脚本支撑的技能动作组合 `SidecarActionDispatcher`。将动作查找保留在适配器中，但让核心规范化载荷、缺失源错误、执行器异常和 JSON 安全结果信封。
4. 使用 `HostUiDispatcherBase` 子类替换本地 UI 线程作业队列。仅实现 `poke_host_pump()` 和可选的诊断钩子，如 `format_exception_error`、`format_timeout_error`、`on_job_queued`、`on_job_started` 和 `on_job_finished`。
5. 将定时器生命周期移入 `HostPumpController`。将宿主的空闲回调、.NET 定时器、Blender 定时器或 Qt `QTimer` 映射到 `HostPumpTimerAdapter`。
6. 仅在调度器能够实际运行主线程工作后连接就绪性。适配器冒烟测试可能需要 `host_execution_bridge` 和 `main_thread_executor` 就绪位。
7. 在接触实际 DCC 之前，使用假服务器和 `ManualHostTimerAdapter` 添加或更新适配器一致性测试。使用 `tests/test_dispatcher_migration_conformance.py` 中的核心夹具作为最小契约。
8. 当宿主可用时，在适配器仓库中运行一次实际冒烟测试。如果不可用，请在 PR 中记录差距，并保持假一致性路径在 CI 中可运行。

## 核心一致性夹具

`tests/test_dispatcher_migration_conformance.py` 模拟了两个适配器家族：

- 类似 Maya 的 Qt 伴生进程，使用 `SidecarActionDispatcher`，解析捆绑的技能脚本，通过 `HostUiDispatcherBase` 执行，并通过 `HostPumpController` 驱动泵。
- 类似 3ds Max 的脚本伴生进程，将显式 `source_file` 传递给 `SidecarActionDispatcher.script_executor(...)`。

夹具涵盖成功分发、格式错误的载荷、缺失服务器、未知动作、缺失源文件、执行器故障、取消、超时和关闭清理。适配器仓库应复制测试的形态，而非核心实现代码。

## 适配器说明

### Maya

使用 `start_qt_server(...)` 作为伴生进程端点，并让 Maya 插件负责进程生命周期、动作注册和会话元数据。通过 `SidecarActionDispatcher.maya_executor(...)` 路由脚本支撑的技能。当技能需要 UI 线程时，使用 `HostUiDispatcherBase` 子类，其 `poke_host_pump()` 映射到 Maya 的空闲或延迟执行原语。

### 3ds Max

当 Qt 可用时，使用共享 Qt 调度器替换本地 TCP/JSON 桥接代码。将 MaxScript 或 .NET 定时器胶水保留在 `HostPumpTimerAdapter` 中，通过 `SidecarActionDispatcher.script_executor(...)` 路由脚本支撑的技能，并将 Max 特定的诊断放在调度器钩子中而非队列内部。

### Blender

使用 `HostUiDispatcherBase` 处理 UI 线程工作，并将 `bpy.app.timers` 适配到定时器适配器契约。批处理 Blender 或 pytest 路径应使用 `InProcessCallableDispatcher`，而非假装存在 UI 泵。

### Houdini

携带 Qt 的 Houdini 会话可以使用 `QtHostTimerAdapter` 和 `start_qt_server(...)`。无头 `hython` 路径应在适配器验证所调用的 DCC API 无需 UI 循环即可安全运行后，保持内联。
