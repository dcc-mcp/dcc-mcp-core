# Process Guide

Cross-platform DCC process monitoring, lifecycle management, and crash recovery.

## Overview

Provides:

- **Process Monitoring** — Live resource usage snapshots (CPU, memory)
- **DCC Launching** — Async spawn/terminate/kill of DCC applications
- **Crash Recovery** — Automatic restart policy with backoff strategies
- **Background Watching** — Event-driven process state monitoring

## Quick Start

### Monitoring a Process

```python
from dcc_mcp_core import ProcessMonitor

monitor = ProcessMonitor()

# Track a process by PID
monitor.track(pid=1234, name="maya")

# Refresh and query
monitor.refresh()
info = monitor.query(pid=1234)

if info:
    print(f"CPU: {info.cpu_usage_percent:.1f}%")
    print(f"Memory: {info.memory_bytes / 1024 / 1024:.1f} MB")
    print(f"Status: {info.status}")
```

### Launching a DCC

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
print(f"Launched PID: {process_info.pid}")
```

## ProcessMonitor

Track and query process resource usage.

### Tracking Processes

```python
monitor = ProcessMonitor()

# Track by PID
monitor.track(pid=1234, name="maya")

# Track by name pattern
monitor.track_by_name("maya")

# Track current process
monitor.track_current("self")

# Untrack
monitor.untrack(pid=1234)
```

### Querying Resources

```python
# Refresh all tracked processes
monitor.refresh()

# Query specific process
info = monitor.query(pid=1234)

# Query all tracked processes
all_info = monitor.query_all()
for pid, info in all_info.items():
    print(f"{pid}: {info.name} - {info.cpu_usage_percent}% CPU")
```

### ProcessInfo Fields

| Field | Type | Description |
|-------|------|-------------|
| `pid` | `int` | Process ID |
| `name` | `str` | Process name |
| `cpu_usage_percent` | `float` | CPU usage (0-100) |
| `memory_bytes` | `int` | Memory usage in bytes |
| `status` | `ProcessStatus` | Current status |
| `start_time` | `datetime` | Start time |

### ProcessStatus Values

| Status | Description |
|--------|-------------|
| `Running` | Process is running normally |
| `Sleeping` | Process is sleeping |
| `Stopped` | Process is stopped |
| `Zombie` | Process is zombie |
| `Crashed` | Process has crashed |
| `Unknown` | Status could not be determined |

## DccLauncher

Launch and manage DCC processes.

### Basic Launch

```python
launcher = DccLauncher()

config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=["-prompt"],
)

process_info = launcher.launch(config).await_result()
```

### Configuration Options

```python
config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=[
        "-prompt",           # Run in batch mode
        "-script", "init.py" # Run startup script
    ],
    cwd="/project",                    # Working directory
    env={                             # Environment variables
        "MAYA_APP_DIR": "/tmp/maya",
        "PYTHONPATH": "/project/python"
    },
    timeout_ms=30000,                 # Launch timeout
    detach=True                        # Run detached
)
```

### Process Lifecycle

```python
# Terminate gracefully (SIGTERM on Unix, WM_CLOSE on Windows)
launcher.terminate(pid=1234, timeout_ms=5000)

# Kill immediately
launcher.kill(pid=1234)

# Wait for exit
exit_code = launcher.wait(pid=1234, timeout_ms=60000)
```

### Maya Launch Example

```python
maya_config = DccProcessConfig(
    executable="/usr/bin/maya",
    args=["-prompt", "-batch"],
    cwd="/project/scenes",
    env={
        "MAYA_APP_DIR": "/tmp/maya",
        "MAYA_SCRIPT_PATH": "/project/scripts"
    },
    timeout_ms=60000
)

launcher = DccLauncher()
future = launcher.launch(maya_config)

# Wait for launch
maya_info = future.await_result()
print(f"Maya running as PID {maya_info.pid}")
```

## CrashRecoveryPolicy

Automatic restart policy engine.

### Basic Policy

```python
from dcc_mcp_core import CrashRecoveryPolicy, BackoffStrategy

policy = CrashRecoveryPolicy(
    max_restarts=3,
    backoff=BackoffStrategy.EXPONENTIAL
)

# Check if should restart
if policy.should_restart(ProcessStatus.Crashed):
    delay = policy.next_restart_delay(attempt=1)
    time.sleep(delay / 1000)
    launch_maya()
```

### Builder Pattern

```python
policy = CrashRecoveryPolicy.builder() \
    .max_restarts(5) \
    .backoff(BackoffStrategy.EXPONENTIAL) \
    .initial_delay_ms(1000) \
    .max_delay_ms(60000) \
    .build()
```

### Backoff Strategies

| Strategy | Description | Example Delay Sequence |
|----------|-------------|------------------------|
| `NONE` | Immediate restart | 0, 0, 0... |
| `LINEAR` | Add fixed delay | 1s, 2s, 3s... |
| `EXPONENTIAL` | Double delay | 1s, 2s, 4s, 8s... |
| `FIBONACCI` | Fibonacci backoff | 1s, 1s, 2s, 3s, 5s... |

### Managing Restarts

```python
# Record successful run (resets policy)
policy.reset()

# Record a restart attempt
policy.record_restart(attempt=1)

# Check if max restarts exceeded
if policy.should_restart(ProcessStatus.Crashed):
    # Attempt restart
    pass
else:
    # Give up
    alert_operator("Maya keeps crashing")
```

## ProcessWatcher

Async background watch loop with event notifications.

### Basic Watch

```python
from dcc_mcp_core import ProcessWatcher

watcher = ProcessWatcher()

def on_event(event):
    print(f"Event: {event.type}")
    print(f"PID: {event.pid}")
    print(f"Timestamp: {event.timestamp}")

handle = watcher.watch(
    pid=1234,
    events=["started", "stopped", "crashed"],
    callback=on_event
)
```

### Event Types

| Event | Description |
|-------|-------------|
| `Started` | Process was started |
| `Stopped` | Process stopped normally |
| `Crashed` | Process crashed |
| `OOM` | Process was killed by OOM |
| `Respawned` | Process was automatically respawned |

### Managing Watchers

```python
# Pause watching
handle.pause()

# Resume watching
handle.resume()

# Stop and cleanup
handle.stop()

# Unwatch by handle
watcher.unwatch(handle)
```

## Auto-Restart Example

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
        alert_operator("Maya crashed 5 times, giving up")
        return

    delay = policy.next_restart_delay(event.attempt)
    print(f"Restarting Maya in {delay}ms...")
    time.sleep(delay / 1000)

    launcher.launch(maya_config)
    policy.record_restart(event.attempt)

# Start watching for crashes
watcher.watch(pid=maya_pid, events=["crashed"], callback=on_crash)

# Also watch for normal stop (in case we want to respawn)
watcher.watch(pid=maya_pid, events=["stopped"], callback=on_crash)
```

## Error Handling

```python
from dcc_mcp_core import ProcessError

try:
    info = monitor.query(pid=999999)
except ProcessError as e:
    print(f"Process error: {e}")

try:
    launcher.launch(invalid_config)
except ProcessError as e:
    print(f"Launch failed: {e}")
```

## Best Practices

### 1. Always Refresh Before Query

```python
monitor.refresh()
info = monitor.query(pid=1234)  # Now has fresh data
```

### 2. Handle Missing Processes Gracefully

```python
info = monitor.query(pid=1234)
if info is None:
    print("Process not found")
else:
    print(f"CPU: {info.cpu_usage_percent}%")
```

### 3. Use Appropriate Timeouts

```python
# Short timeout for quick operations
launcher.terminate(pid=1234, timeout_ms=2000)

# Longer timeout for DCC apps
launcher.terminate(pid=maya_pid, timeout_ms=10000)
```

### 4. Monitor Resource Usage

```python
def check_resources():
    monitor.refresh()
    for pid, info in monitor.query_all().items():
        if info.cpu_usage_percent > 90:
            alert(f"High CPU: {info.name} ({info.cpu_usage_percent}%)")
        if info.memory_bytes > 10 * 1024 * 1024 * 1024:
            alert(f"High memory: {info.name} ({info.memory_bytes / 1024 / 1024 / 1024:.1f} GB)")
```
