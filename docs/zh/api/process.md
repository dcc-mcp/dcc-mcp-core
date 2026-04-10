# 进程 API

`dcc_mcp_core` (process 模块)

跨平台 DCC 进程监控、生命周期管理和崩溃恢复。

## 概述

提供：

- **进程监控** — 通过 `PyProcessMonitor` 实时资源使用（CPU、内存、状态）
- **DCC 启动** — 通过 `PyDccLauncher` 异步启动/终止/kill
- **崩溃恢复** — 通过 `PyCrashRecoveryPolicy` 实现指数/固定退避重启策略
- **后台监视** — 通过 `PyProcessWatcher` 事件轮询

## PyProcessMonitor

使用 `sysinfo` 跟踪和查询进程资源使用。

### 构造函数

```python
from dcc_mcp_core import PyProcessMonitor

monitor = PyProcessMonitor()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `track(pid, name)` | `None` | 注册要监控的 PID |
| `untrack(pid)` | `None` | 停止监控 PID |
| `refresh()` | `None` | 刷新底层系统数据 |
| `query(pid)` | `dict \| None` | 获取 PID 的快照 |
| `list_all()` | `list[dict]` | 获取所有跟踪 PID 的快照 |
| `is_alive(pid)` | `bool` | 检查 PID 是否在 OS 进程表中 |
| `tracked_count()` | `int` | 跟踪的 PID 数量 |

### 返回字典的键

| 键 | 类型 | 描述 |
|-----|------|------|
| `pid` | `int` | 进程 ID |
| `name` | `str` | 用户定义的名称 |
| `status` | `str` | 操作系统状态字符串 |
| `cpu_usage_percent` | `float` | CPU 使用率 (0-100) |
| `memory_bytes` | `int` | 内存使用（字节） |
| `restart_count` | `int` | 重启计数 |

### 示例

```python
import os
monitor = PyProcessMonitor()
monitor.track(os.getpid(), "self")
monitor.refresh()
info = monitor.query(os.getpid())
if info:
    print(f"CPU: {info['cpu_usage_percent']:.1f}%")
    print(f"内存: {info['memory_bytes'] / 1024 / 1024:.1f} MB")
```

## PyDccLauncher

异步 DCC 进程启动器（启动/终止/kill）。

### 构造函数

```python
from dcc_mcp_core import PyDccLauncher

launcher = PyDccLauncher()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `launch(name, executable, args=None, launch_timeout_ms=30000)` | `dict` | 启动 DCC 进程 |
| `terminate(name, timeout_ms=5000)` | `None` | 优雅终止 |
| `kill(name)` | `None` | 强制杀死 |
| `pid_of(name)` | `int \| None` | 按名称获取 PID |
| `running_count()` | `int` | 活跃子进程数量 |
| `restart_count(name)` | `int` | 名称的重启计数 |

### 示例

```python
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=30000,
)
print(f"已启动 PID: {info['pid']}")

# 终止
launcher.terminate("maya", timeout_ms=5000)
```

## PyCrashRecoveryPolicy

DCC 进程的崩溃恢复策略。

### 构造函数

```python
from dcc_mcp_core import PyCrashRecoveryPolicy

policy = PyCrashRecoveryPolicy(max_restarts=3)
```

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `max_restarts` | `int` | 最大重启次数（构造函数参数，默认 3） |

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `use_exponential_backoff(initial_ms, max_delay_ms)` | `None` | 使用指数退避 |
| `use_fixed_backoff(delay_ms)` | `None` | 使用固定延迟 |
| `should_restart(status)` | `bool` | 检查状态是否需要重启 |
| `next_delay_ms(name, attempt)` | `int` | 获取尝试前的延迟 |

### 示例

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

if policy.should_restart("crashed"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"在 {delay}ms 后重启...")
```

### 支持的状态值

| 状态 | 描述 |
|------|------|
| `"crashed"` | 进程崩溃 |
| `"unresponsive"` | 进程无响应 |

## PyProcessWatcher

带事件轮询的异步后台进程监视器。

### 构造函数

```python
from dcc_mcp_core import PyProcessWatcher

watcher = PyProcessWatcher(poll_interval_ms=500)
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `track(pid, name)` | `None` | 注册要监控的 PID |
| `untrack(pid)` | `None` | 停止监控 PID |
| `start()` | `None` | 启动后台监视循环 |
| `stop()` | `None` | 停止后台监视循环 |
| `poll_events()` | `list[dict]` | 排出待处理事件 |
| `is_running()` | `bool` | 检查循环是否运行 |
| `tracked_count()` | `int` | 跟踪的 PID 数量 |

### 事件类型

事件字典包含: `type`, `pid`, `name`

| 事件类型 | 额外字段 |
|----------|----------|
| `heartbeat` | `new_status`, `cpu_usage_percent`, `memory_bytes` |
| `status_changed` | `old_status`, `new_status` |
| `exited` | — |

### 示例

```python
import os, time
watcher = PyProcessWatcher(poll_interval_ms=200)
watcher.track(os.getpid(), "self")
watcher.start()

time.sleep(0.5)

for event in watcher.poll_events():
    print(f"事件: {event['type']} - {event['name']}")

watcher.stop()
```

## 集成示例

### 自动重启 DCC

```python
import time
from dcc_mcp_core import PyDccLauncher, PyProcessWatcher, PyCrashRecoveryPolicy

launcher = PyDccLauncher()
watcher = PyProcessWatcher(poll_interval_ms=500)
policy = PyCrashRecoveryPolicy(max_restarts=5)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

info = launcher.launch(name="maya", executable="/usr/bin/maya")
watcher.track(info["pid"], "maya")
watcher.start()

while True:
    events = watcher.poll_events()
    for event in events:
        if event["type"] == "exited":
            if policy.should_restart("crashed"):
                delay = policy.next_delay_ms("maya", attempt=0)
                time.sleep(delay / 1000)
                info = launcher.launch(name="maya", executable="/usr/bin/maya")
                watcher.track(info["pid"], "maya")
    time.sleep(0.1)
```
