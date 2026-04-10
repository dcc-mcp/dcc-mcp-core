# 进程指南

跨平台 DCC 进程监控、生命周期管理和崩溃恢复。

## 概述

提供：

- **进程监控** — 通过 `PyProcessMonitor` 实时资源使用（CPU、内存、状态）
- **DCC 启动** — 通过 `PyDccLauncher` 异步启动/终止/kill
- **崩溃恢复** — 通过 `PyCrashRecoveryPolicy` 实现指数/固定退避重启策略
- **后台监视** — 通过 `PyProcessWatcher` 事件轮询

## PyProcessMonitor

使用 `sysinfo` 跟踪和查询进程资源使用。

### 基础用法

```python
import os
from dcc_mcp_core import PyProcessMonitor

monitor = PyProcessMonitor()

# 跟踪当前进程
monitor.track(os.getpid(), "self")

# 查询前先刷新
monitor.refresh()

# 查询特定 PID
info = monitor.query(os.getpid())
if info:
    print(f"状态: {info['status']}")
    print(f"CPU: {info['cpu_usage_percent']:.1f}%")
    print(f"内存: {info['memory_bytes'] / 1024 / 1024:.1f} MB")
```

### 跟踪/取消跟踪

```python
monitor = PyProcessMonitor()

# 按 PID 跟踪
monitor.track(pid=1234, name="maya")

# 停止跟踪
monitor.untrack(pid=1234)
```

### 查询方法

```python
monitor.refresh()

# 查询单个进程
info = monitor.query(pid=1234)

# 查询所有跟踪的进程
all_info = monitor.list_all()
for info in all_info:
    print(f"{info['name']}: {info['cpu_usage_percent']}% CPU")

# 检查是否存活
if monitor.is_alive(pid=1234):
    print("进程正在运行")

# 跟踪计数
print(f"正在跟踪 {monitor.tracked_count()} 个进程")
```

### 返回字典的键

| 键 | 类型 | 描述 |
|-----|------|------|
| `pid` | `int` | 进程 ID |
| `name` | `str` | 用户定义的名称 |
| `status` | `str` | 操作系统状态字符串 |
| `cpu_usage_percent` | `float` | CPU 使用率 (0-100) |
| `memory_bytes` | `int` | 内存使用（字节） |
| `restart_count` | `int` | 重启计数 |

## PyDccLauncher

异步启动和管理 DCC 进程。

### 基础启动

```python
from dcc_mcp_core import PyDccLauncher

launcher = PyDccLauncher()

# 启动 DCC
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=30000,
)

print(f"已启动 PID: {info['pid']}")
```

### 带环境的启动

```python
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-script", "init.py"],
    launch_timeout_ms=60000,
)
```

### 进程生命周期

```python
# 优雅终止
launcher.terminate("maya", timeout_ms=5000)

# 强制杀死
launcher.kill("maya")

# 按名称获取 PID
pid = launcher.pid_of("maya")
if pid:
    print(f"Maya 运行在 PID {pid}")

# 检查运行计数
print(f"运行中: {launcher.running_count()} 个进程")

# 检查重启计数
print(f"重启计数: {launcher.restart_count('maya')}")
```

### Maya 示例

```python
launcher = PyDccLauncher()

maya_info = launcher.launch(
    name="maya-2025",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=60000,
)

print(f"Maya 运行在 PID {maya_info['pid']}")

# ... 工作 ...

launcher.terminate("maya-2025")
```

## PyCrashRecoveryPolicy

带退避策略的自动重启策略。

### 基础策略

```python
from dcc_mcp_core import PyCrashRecoveryPolicy

policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# 检查是否应重启
if policy.should_restart("crashed"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"在 {delay}ms 后重启...")
```

### 固定退避

```python
policy = PyCrashRecoveryPolicy(max_restarts=5)
policy.use_fixed_backoff(delay_ms=2000)

if policy.should_restart("unresponsive"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"在 {delay}ms 后重试...")
```

### 指数退避

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# 尝试 0 -> 1000ms, 尝试 1 -> 2000ms, 尝试 2 -> 4000ms
for attempt in range(3):
    if policy.should_restart("crashed"):
        delay = policy.next_delay_ms("maya", attempt=attempt)
        print(f"尝试 {attempt}: 等待 {delay}ms")
```

### 管理策略状态

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)

# 检查 max_restarts 限制
print(f"最大重启次数: {policy.max_restarts}")

# 检查重启资格
if policy.should_restart("crashed"):
    # 尝试重启
    pass
```

## PyProcessWatcher

带事件轮询的异步后台进程监视器。

### 基础监视

```python
import os
import time
from dcc_mcp_core import PyProcessWatcher

watcher = PyProcessWatcher(poll_interval_ms=200)
watcher.track(os.getpid(), "self")
watcher.start()

time.sleep(0.5)

# 轮询事件
for event in watcher.poll_events():
    print(f"事件: {event['type']} - {event['name']}")

watcher.stop()
```

### 事件类型

事件字典包含: `type`, `pid`, `name`

| 事件类型 | 额外字段 |
|----------|----------|
| `heartbeat` | `new_status`, `cpu_usage_percent`, `memory_bytes` |
| `status_changed` | `old_status`, `new_status` |
| `exited` | — |

### 轮询模式

```python
watcher = PyProcessWatcher(poll_interval_ms=500)
watcher.track(pid=1234, name="maya")
watcher.start()

try:
    while True:
        events = watcher.poll_events()
        for event in events:
            if event["type"] == "exited":
                print(f"{event['name']} 已退出")
            elif event["type"] == "heartbeat":
                print(f"CPU: {event['cpu_usage_percent']}%")
        time.sleep(0.1)
finally:
    watcher.stop()
```

### 启动/停止

```python
watcher = PyProcessWatcher()

watcher.track(pid=1234, name="maya")
watcher.start()

# ... 工作 ...

watcher.stop()

# 检查状态
print(f"监视器运行中: {watcher.is_running()}")
print(f"已跟踪: {watcher.tracked_count()}")
```

## 完整示例

### 自动重启 DCC

```python
import time
from dcc_mcp_core import PyDccLauncher, PyProcessWatcher, PyCrashRecoveryPolicy

launcher = PyDccLauncher()
watcher = PyProcessWatcher(poll_interval_ms=500)
policy = PyCrashRecoveryPolicy(max_restarts=5)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# 启动 Maya
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt"],
)
print(f"已启动 Maya PID {info['pid']}")

watcher.track(info["pid"], "maya")
watcher.start()

attempt = 0
while True:
    events = watcher.poll_events()
    for event in events:
        if event["type"] == "exited":
            print("Maya 已退出")
            if policy.should_restart("crashed") and attempt < 5:
                delay = policy.next_delay_ms("maya", attempt=attempt)
                print(f"在 {delay}ms 后重启...")
                time.sleep(delay / 1000)
                info = launcher.launch(
                    name="maya",
                    executable="/usr/autodesk/maya/bin/maya",
                    args=["-prompt"],
                )
                watcher.track(info["pid"], "maya")
                attempt += 1
            else:
                print("超过最大重启次数")
                watcher.stop()
                exit(1)

    time.sleep(0.1)
```

## 最佳实践

### 1. 查询前始终刷新

```python
monitor.refresh()
info = monitor.query(pid=1234)  # 现在有最新数据
```

### 2. 优雅处理缺失进程

```python
info = monitor.query(pid=1234)
if info is None:
    print("进程未找到")
else:
    print(f"CPU: {info['cpu_usage_percent']}%")
```

### 3. 使用适当的超时

```python
# 快速操作短超时
launcher.terminate("quick_proc", timeout_ms=2000)

# DCC 应用更长超时
launcher.terminate("maya", timeout_ms=10000)
```

### 4. 监控资源使用

```python
def check_resources():
    monitor.refresh()
    for info in monitor.list_all():
        if info["cpu_usage_percent"] > 90:
            print(f"高 CPU: {info['name']}")
        if info["memory_bytes"] > 10 * 1024 * 1024 * 1024:
            print(f"高内存: {info['name']}")
```
