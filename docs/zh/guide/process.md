# 进程指南

跨平台 DCC 进程监控、生命周期管理和崩溃恢复。

## 概述

提供：

- **进程监控** — 实时资源使用快照（CPU、内存）
- **DCC 启动** — DCC 应用程序的异步启动/终止/kill
- **崩溃恢复** — 带退避策略的自动重启策略
- **后台监视** — 事件驱动的进程状态监控

## 快速开始

### 监控进程

```python
from dcc_mcp_core import ProcessMonitor

monitor = ProcessMonitor()

# 按 PID 跟踪进程
monitor.track(pid=1234, name="maya")

# 刷新和查询
monitor.refresh()
info = monitor.query(pid=1234)

if info:
    print(f"CPU: {info.cpu_usage_percent:.1f}%")
    print(f"内存: {info.memory_bytes / 1024 / 1024:.1f} MB")
    print(f"状态: {info.status}")
```

### 启动 DCC

```python
from dcc_mcp_core import DccLauncher, DccProcessConfig

launcher = DccLauncher()

config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=["-prompt"],
    cwd="/project",
    timeout_ms=30000,
)

future = launcher.launch(config)
process_info = future.await_result()
print(f"已启动 PID: {process_info.pid}")
```

## ProcessMonitor

跟踪和查询进程资源使用。

### 跟踪进程

```python
monitor = ProcessMonitor()

# 按 PID 跟踪
monitor.track(pid=1234, name="maya")

# 按名称模式跟踪
monitor.track_by_name("maya")

# 跟踪当前进程
monitor.track_current("self")

# 取消跟踪
monitor.untrack(pid=1234)
```

### 查询资源

```python
# 刷新所有跟踪的进程
monitor.refresh()

# 查询特定进程
info = monitor.query(pid=1234)

# 查询所有跟踪的进程
all_info = monitor.query_all()
for pid, info in all_info.items():
    print(f"{pid}: {info.name} - {info.cpu_usage_percent}% CPU")
```

### ProcessInfo 字段

| 字段 | 类型 | 描述 |
|------|------|------|
| `pid` | `int` | 进程 ID |
| `name` | `str` | 进程名称 |
| `cpu_usage_percent` | `float` | CPU 使用率 (0-100) |
| `memory_bytes` | `int` | 内存使用（字节） |
| `status` | `ProcessStatus` | 当前状态 |
| `start_time` | `datetime` | 启动时间 |

## DccLauncher

启动和管理 DCC 进程。

### 基础启动

```python
launcher = DccLauncher()

config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=["-prompt"],
)

process_info = launcher.launch(config).await_result()
```

### 配置选项

```python
config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=[
        "-prompt",           # 批处理模式运行
        "-script", "init.py" # 运行启动脚本
    ],
    cwd="/project/scenes",              # 工作目录
    env={                               # 环境变量
        "MAYA_APP_DIR": "/tmp/maya",
        "PYTHONPATH": "/project/python"
    },
    timeout_ms=30000,                  # 启动超时
    detach=True                        # 分离运行
)
```

### 进程生命周期

```python
# 优雅终止 (Unix SIGTERM, Windows WM_CLOSE)
launcher.terminate(pid=1234, timeout_ms=5000)

# 立即杀死
launcher.kill(pid=1234)

# 等待退出
exit_code = launcher.wait(pid=1234, timeout_ms=60000)
```

## CrashRecoveryPolicy

自动重启策略引擎。

### 基础策略

```python
from dcc_mcp_core import CrashRecoveryPolicy, BackoffStrategy

policy = CrashRecoveryPolicy(
    max_restarts=3,
    backoff=BackoffStrategy.EXPONENTIAL
)

# 检查是否应重启
if policy.should_restart(ProcessStatus.Crashed):
    delay = policy.next_restart_delay(attempt=1)
    time.sleep(delay / 1000)
    launch_maya()
```

### Builder 模式

```python
policy = CrashRecoveryPolicy.builder() \
    .max_restarts(5) \
    .backoff(BackoffStrategy.EXPONENTIAL) \
    .initial_delay_ms(1000) \
    .max_delay_ms(60000) \
    .build()
```

### 退避策略

| 策略 | 描述 | 示例延迟序列 |
|------|------|--------------|
| `NONE` | 立即重启 | 0, 0, 0... |
| `LINEAR` | 增加固定延迟 | 1s, 2s, 3s... |
| `EXPONENTIAL` | 双倍延迟 | 1s, 2s, 4s, 8s... |
| `FIBONACCI` | 斐波那契退避 | 1s, 1s, 2s, 3s, 5s... |

## ProcessWatcher

带事件通知的异步后台监视循环。

### 基础监视

```python
from dcc_mcp_core import ProcessWatcher

watcher = ProcessWatcher()

def on_event(event):
    print(f"事件: {event.type}")
    print(f"PID: {event.pid}")
    print(f"时间戳: {event.timestamp}")

handle = watcher.watch(
    pid=1234,
    events=["started", "stopped", "crashed"],
    callback=on_event
)
```

### 事件类型

| 事件 | 描述 |
|------|------|
| `Started` | 进程已启动 |
| `Stopped` | 进程正常停止 |
| `Crashed` | 进程已崩溃 |
| `OOM` | 进程被 OOM killer 杀死 |
| `Respawned` | 进程已自动重启 |

## 自动重启示例

```python
from dcc_mcp_core import (
    DccLauncher, ProcessWatcher, CrashRecoveryPolicy,
    ProcessMonitor, BackoffStrategy
)

launcher = DccLauncher()
monitor = ProcessMonitor()
watcher = ProcessWatcher()

policy = CrashRecoveryPolicy.builder() \
    .max_restarts(5) \
    .backoff(BackoffStrategy.EXPONENTIAL) \
    .build()

maya_config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=["-prompt"]
)

def on_crash(event):
    if not policy.should_restart(event.status):
        alert_operator("Maya 崩溃 5 次，放弃")
        return

    delay = policy.next_restart_delay(event.attempt)
    print(f"在 {delay}ms 后重启 Maya...")
    time.sleep(delay / 1000)

    launcher.launch(maya_config)
    policy.record_restart(event.attempt)

# 开始监视崩溃
watcher.watch(pid=maya_pid, events=["crashed"], callback=on_crash)
```

## 错误处理

```python
from dcc_mcp_core import ProcessError

try:
    info = monitor.query(pid=999999)
except ProcessError as e:
    print(f"进程错误: {e}")

try:
    launcher.launch(invalid_config)
except ProcessError as e:
    print(f"启动失败: {e}")
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
    print(f"CPU: {info.cpu_usage_percent}%")
```

### 3. 使用适当的超时

```python
# 快速操作短超时
launcher.terminate(pid=1234, timeout_ms=2000)

# DCC 应用更长超时
launcher.terminate(pid=maya_pid, timeout_ms=10000)
```
