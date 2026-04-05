# Process API

`dcc_mcp_core` (process 模块)

跨平台 DCC 进程监控、生命周期管理和崩溃恢复。

## 概述

提供：

- **进程监控** — 实时资源使用快照（CPU、内存）
- **DCC 启动** — DCC 应用程序的异步启动/终止/kill
- **崩溃恢复** — 带退避策略的自动重启策略
- **后台监视** — 事件驱动的进程状态监控

## ProcessMonitor

跟踪和查询进程资源使用。

### 构造函数

```python
from dcc_mcp_core import ProcessMonitor

monitor = ProcessMonitor()
```

### 跟踪进程

```python
# 按 PID 跟踪
monitor.track(pid=1234, name="maya")

# 跟踪当前进程
monitor.track_current("self")

# 取消跟踪进程
monitor.untrack(pid=1234)
```

### 查询资源

```python
# 刷新进程数据
monitor.refresh()

# 查询特定进程
info = monitor.query(pid=1234)
if info:
    print(f"CPU: {info.cpu_usage_percent}%")
    print(f"内存: {info.memory_bytes / 1024 / 1024:.1f} MB")
    print(f"状态: {info.status}")
```

### ProcessInfo

| 字段 | 类型 | 描述 |
|------|------|-------------|
| `pid` | `int` | 进程 ID |
| `name` | `str` | 进程名称 |
| `cpu_usage_percent` | `float` | CPU 使用率 (0-100) |
| `memory_bytes` | `int` | 内存使用（字节） |
| `status` | `ProcessStatus` | 当前状态 |
| `start_time` | `datetime` | 进程启动时间 |

### ProcessStatus

| 状态 | 描述 |
|------|------|
| `Running` | 进程正常运行 |
| `Sleeping` | 进程处于睡眠状态 |
| `Stopped` | 进程已停止 |
| `Zombie` | 进程是僵尸进程 |
| `Crashed` | 进程已崩溃 |
| `Unknown` | 无法确定状态 |

## DccLauncher

启动和管理 DCC 进程。

### 构造函数

```python
from dcc_mcp_core import DccLauncher

launcher = DccLauncher()
```

### 启动 DCC

```python
from dcc_mcp_core import DccProcessConfig

config = DccProcessConfig(
    executable="/path/to/maya",
    args=["-prompt"],
    cwd="/project",
    env={"MAYA_APP_DIR": "/tmp/maya"},
    timeout_ms=30000,
)

future = launcher.launch(config)
process_info = future.await_result()
print(f"已启动 PID: {process_info.pid}")
```

### DccProcessConfig

| 字段 | 类型 | 描述 |
|------|------|------|
| `executable` | `str` | DCC 可执行文件路径 |
| `args` | `List[str]` | 命令行参数 |
| `cwd` | `str` | 工作目录 |
| `env` | `dict` | 环境变量 |
| `timeout_ms` | `int` | 启动超时 |
| `detach` | `bool` | 与父进程分离运行 |

### 进程生命周期

```python
# 优雅终止（Unix SIGTERM, Windows WM_CLOSE）
launcher.terminate(pid=1234, timeout_ms=5000)

# 立即杀死
launcher.kill(pid=1234)

# 等待退出
exit_code = launcher.wait(pid=1234, timeout_ms=60000)
```

## CrashRecoveryPolicy

自动重启策略引擎。

### 构造函数

```python
from dcc_mcp_core import CrashRecoveryPolicy, BackoffStrategy

policy = CrashRecoveryPolicy(
    max_restarts=3,
    backoff=BackoffStrategy.EXPONENTIAL
)
```

### 退避策略

| 策略 | 描述 |
|------|------|
| `NONE` | 立即重启 |
| `LINEAR` | 线性增加延迟 |
| `EXPONENTIAL` | 每次重启加倍延迟 |
| `FIBONACCI` | 斐波那契退避 |

### 策略方法

```python
# 检查崩溃进程是否应重启
should_restart = policy.should_restart(ProcessStatus.Crashed)
print(f"应重启: {should_restart}")

# 获取下次重启尝试前的延迟
delay_ms = policy.next_restart_delay(attempt=1)
print(f"等待 {delay_ms}ms 后重启")

# 记录重启尝试
policy.record_restart(attempt=1)

# 成功运行后重置策略
policy.reset()
```

### 构建策略

```python
policy = CrashRecoveryPolicy.builder() \
    .max_restarts(5) \
    .backoff(BackoffStrategy.EXPONENTIAL) \
    .initial_delay_ms(1000) \
    .max_delay_ms(60000) \
    .build()
```

## ProcessWatcher

带事件通知的异步后台监视循环。

### 构造函数

```python
from dcc_mcp_core import ProcessWatcher

watcher = ProcessWatcher()
```

### 监视进程

```python
def on_event(event):
    print(f"进程事件: {event}")

handle = watcher.watch(
    pid=1234,
    events=["started", "stopped", "crashed"],
    callback=on_event
)

# 停止监视
watcher.unwatch(handle)
```

### ProcessEvent

| 事件 | 描述 |
|------|------|
| `Started` | 进程已启动 |
| `Stopped` | 进程正常停止 |
| `Crashed` | 进程已崩溃 |
| `OOM` | 进程被 OOM killer 杀死 |
| `Respawned` | 进程已自动重启 |

## 错误处理

```python
from dcc_mcp_core import ProcessError

try:
    info = monitor.query(pid=999999)  # 不存在的进程
except ProcessError as e:
    print(f"进程错误: {e}")

try:
    launcher.launch(invalid_config)
except ProcessError as e:
    print(f"启动失败: {e}")
```
