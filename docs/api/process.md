# Process API

`dcc_mcp_core` (process module)

Cross-platform DCC process monitoring, lifecycle management, and crash recovery.

## Overview

Provides:

- **Process Monitoring** — Live resource usage snapshots (CPU, memory)
- **DCC Launching** — Async spawn/terminate/kill of DCC applications
- **Crash Recovery** — Automatic restart policy with backoff strategies
- **Background Watching** — Event-driven process state monitoring

## ProcessMonitor

Track and query process resource usage.

### Constructor

```python
from dcc_mcp_core import ProcessMonitor

monitor = ProcessMonitor()
```

### Tracking Processes

```python
# Track by PID
monitor.track(pid=1234, name="maya")

# Track current process
monitor.track_current("self")

# Untrack a process
monitor.untrack(pid=1234)
```

### Querying Resources

```python
# Refresh process data
monitor.refresh()

# Query a specific process
info = monitor.query(pid=1234)
if info:
    print(f"CPU: {info.cpu_usage_percent}%")
    print(f"Memory: {info.memory_bytes / 1024 / 1024:.1f} MB")
    print(f"Status: {info.status}")
```

### ProcessInfo

| Field | Type | Description |
|-------|------|-------------|
| `pid` | `int` | Process ID |
| `name` | `str` | Process name |
| `cpu_usage_percent` | `float` | CPU usage (0-100) |
| `memory_bytes` | `int` | Memory usage in bytes |
| `status` | `ProcessStatus` | Current status |
| `start_time` | `datetime` | When process started |

### ProcessStatus

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

### Constructor

```python
from dcc_mcp_core import DccLauncher

launcher = DccLauncher()
```

### Launching DCCs

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
print(f"Launched PID: {process_info.pid}")
```

### DccProcessConfig

| Field | Type | Description |
|-------|------|-------------|
| `executable` | `str` | Path to DCC executable |
| `args` | `List[str]` | Command-line arguments |
| `cwd` | `str` | Working directory |
| `env` | `dict` | Environment variables |
| `timeout_ms` | `int` | Launch timeout |
| `detach` | `bool` | Run detached from parent |

### Process Lifecycle

```python
# Terminate gracefully (SIGTERM on Unix, WM_CLOSE on Windows)
launcher.terminate(pid=1234, timeout_ms=5000)

# Kill immediately
launcher.kill(pid=1234)

# Wait for exit
exit_code = launcher.wait(pid=1234, timeout_ms=60000)
```

## CrashRecoveryPolicy

Automatic restart policy engine.

### Constructor

```python
from dcc_mcp_core import CrashRecoveryPolicy, BackoffStrategy

policy = CrashRecoveryPolicy(
    max_restarts=3,
    backoff=BackoffStrategy.EXPONENTIAL
)
```

### Backoff Strategies

| Strategy | Description |
|----------|-------------|
| `NONE` | Immediate restart |
| `LINEAR` | Increase delay linearly |
| `EXPONENTIAL` | Double delay each restart |
| `FIBONACCI` | Fibonacci backoff |

### Policy Methods

```python
# Check if a crashed process should restart
should_restart = policy.should_restart(ProcessStatus.Crashed)
print(f"Should restart: {should_restart}")

# Get the delay before next restart attempt
delay_ms = policy.next_restart_delay(attempt=1)
print(f"Wait {delay_ms}ms before restart")

# Record a restart attempt
policy.record_restart(attempt=1)

# Reset policy after successful run
policy.reset()
```

### Building a Policy

```python
policy = CrashRecoveryPolicy.builder() \
    .max_restarts(5) \
    .backoff(BackoffStrategy.EXPONENTIAL) \
    .initial_delay_ms(1000) \
    .max_delay_ms(60000) \
    .build()
```

## ProcessWatcher

Async background watch loop with event notifications.

### Constructor

```python
from dcc_mcp_core import ProcessWatcher

watcher = ProcessWatcher()
```

### Watching Processes

```python
# Start watching with event callback
def on_event(event):
    print(f"Process event: {event}")

handle = watcher.watch(
    pid=1234,
    events=["started", "stopped", "crashed"],
    callback=on_event
)

# Stop watching
watcher.unwatch(handle)
```

### ProcessEvent

| Event | Description |
|-------|-------------|
| `Started` | Process was started |
| `Stopped` | Process stopped normally |
| `Crashed` | Process crashed |
| `OOM` | Process was killed by OOM killer |
| `Respawned` | Process was automatically respawned |

### WatcherHandle

```python
# Handle methods
handle.pause()   # Pause watching
handle.resume()  # Resume watching
handle.stop()    # Stop and cleanup
```

## Error Handling

```python
from dcc_mcp_core import ProcessError

try:
    info = monitor.query(pid=999999)  # Non-existent process
except ProcessError as e:
    print(f"Process error: {e}")

try:
    launcher.launch(invalid_config)
except ProcessError as e:
    print(f"Launch failed: {e}")
```

## Integration Examples

### Maya Auto-Restart

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

config = DccProcessConfig(executable="/usr/bin/maya")

def on_crash(event):
    if policy.should_restart(event.status):
        delay = policy.next_restart_delay(event.attempt)
        time.sleep(delay / 1000)
        launcher.launch(config)
        policy.record_restart(event.attempt)

watcher.watch(pid=1234, events=["crashed"], callback=on_crash)
```
