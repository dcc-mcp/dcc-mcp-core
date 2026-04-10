# Process Guide

Cross-platform DCC process monitoring, lifecycle management, and crash recovery.

## Overview

Provides:

- **Process Monitoring** — Live resource usage via `PyProcessMonitor` (CPU, memory, status)
- **DCC Launching** — Async spawn/terminate/kill via `PyDccLauncher`
- **Crash Recovery** — Restart policy with exponential/fixed backoff via `PyCrashRecoveryPolicy`
- **Background Watching** — Event polling via `PyProcessWatcher`

## PyProcessMonitor

Track and query process resource usage using `sysinfo`.

### Basic Usage

```python
import os
from dcc_mcp_core import PyProcessMonitor

monitor = PyProcessMonitor()

# Track current process
monitor.track(os.getpid(), "self")

# Refresh before querying
monitor.refresh()

# Query specific PID
info = monitor.query(os.getpid())
if info:
    print(f"Status: {info['status']}")
    print(f"CPU: {info['cpu_usage_percent']:.1f}%")
    print(f"Memory: {info['memory_bytes'] / 1024 / 1024:.1f} MB")
```

### Track/Untrack

```python
monitor = PyProcessMonitor()

# Track by PID
monitor.track(pid=1234, name="maya")

# Stop tracking
monitor.untrack(pid=1234)
```

### Query Methods

```python
monitor.refresh()

# Query single process
info = monitor.query(pid=1234)

# Query all tracked processes
all_info = monitor.list_all()
for info in all_info:
    print(f"{info['name']}: {info['cpu_usage_percent']}% CPU")

# Check if alive
if monitor.is_alive(pid=1234):
    print("Process is running")

# Count tracked
print(f"Tracking {monitor.tracked_count()} processes")
```

### Returned Dict Keys

| Key | Type | Description |
|-----|------|-------------|
| `pid` | `int` | Process ID |
| `name` | `str` | User-defined name |
| `status` | `str` | OS status string |
| `cpu_usage_percent` | `float` | CPU usage (0-100) |
| `memory_bytes` | `int` | Memory usage in bytes |
| `restart_count` | `int` | Restart count |

## PyDccLauncher

Launch and manage DCC processes asynchronously.

### Basic Launch

```python
from dcc_mcp_core import PyDccLauncher

launcher = PyDccLauncher()

# Launch a DCC
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=30000,
)

print(f"Launched PID: {info['pid']}")
```

### Launch with Environment

```python
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-script", "init.py"],
    launch_timeout_ms=60000,
)
```

### Process Lifecycle

```python
# Terminate gracefully
launcher.terminate("maya", timeout_ms=5000)

# Kill forcefully
launcher.kill("maya")

# Get PID by name
pid = launcher.pid_of("maya")
if pid:
    print(f"Maya running as PID {pid}")

# Check running count
print(f"Running: {launcher.running_count()} processes")

# Check restart count
print(f"Restart count: {launcher.restart_count('maya')}")
```

### Maya Example

```python
launcher = PyDccLauncher()

maya_info = launcher.launch(
    name="maya-2025",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=60000,
)

print(f"Maya running as PID {maya_info['pid']}")

# ... do work ...

launcher.terminate("maya-2025")
```

## PyCrashRecoveryPolicy

Automatic restart policy with backoff strategies.

### Basic Policy

```python
from dcc_mcp_core import PyCrashRecoveryPolicy

policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# Check if should restart
if policy.should_restart("crashed"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"Restarting in {delay}ms...")
```

### Fixed Backoff

```python
policy = PyCrashRecoveryPolicy(max_restarts=5)
policy.use_fixed_backoff(delay_ms=2000)

if policy.should_restart("unresponsive"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"Retrying in {delay}ms...")
```

### Exponential Backoff

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# Attempt 0 -> 1000ms, Attempt 1 -> 2000ms, Attempt 2 -> 4000ms
for attempt in range(3):
    if policy.should_restart("crashed"):
        delay = policy.next_delay_ms("maya", attempt=attempt)
        print(f"Attempt {attempt}: waiting {delay}ms")
```

### Managing Policy State

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)

# Check max_restarts limit
print(f"Max restarts: {policy.max_restarts}")

# Check restart eligibility
if policy.should_restart("crashed"):
    # Attempt restart
    pass
```

## PyProcessWatcher

Async background process watcher with event polling.

### Basic Watch

```python
import os
import time
from dcc_mcp_core import PyProcessWatcher

watcher = PyProcessWatcher(poll_interval_ms=200)
watcher.track(os.getpid(), "self")
watcher.start()

time.sleep(0.5)

# Poll for events
for event in watcher.poll_events():
    print(f"Event: {event['type']} - {event['name']}")

watcher.stop()
```

### Event Types

Event dicts contain: `type`, `pid`, `name`

| Event Type | Additional Fields |
|------------|-------------------|
| `heartbeat` | `new_status`, `cpu_usage_percent`, `memory_bytes` |
| `status_changed` | `old_status`, `new_status` |
| `exited` | — |

### Polling Pattern

```python
watcher = PyProcessWatcher(poll_interval_ms=500)
watcher.track(pid=1234, name="maya")
watcher.start()

try:
    while True:
        events = watcher.poll_events()
        for event in events:
            if event["type"] == "exited":
                print(f"{event['name']} exited")
            elif event["type"] == "heartbeat":
                print(f"CPU: {event['cpu_usage_percent']}%")
        time.sleep(0.1)
finally:
    watcher.stop()
```

### Start/Stop

```python
watcher = PyProcessWatcher()

watcher.track(pid=1234, name="maya")
watcher.start()

# ... do work ...

watcher.stop()

# Check status
print(f"Watcher running: {watcher.is_running()}")
print(f"Tracked: {watcher.tracked_count()}")
```

## Complete Example

### Auto-Restart DCC

```python
import time
from dcc_mcp_core import PyDccLauncher, PyProcessWatcher, PyCrashRecoveryPolicy

launcher = PyDccLauncher()
watcher = PyProcessWatcher(poll_interval_ms=500)
policy = PyCrashRecoveryPolicy(max_restarts=5)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

# Launch Maya
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt"],
)
print(f"Launched Maya PID {info['pid']}")

watcher.track(info["pid"], "maya")
watcher.start()

attempt = 0
while True:
    events = watcher.poll_events()
    for event in events:
        if event["type"] == "exited":
            print("Maya exited")
            if policy.should_restart("crashed") and attempt < 5:
                delay = policy.next_delay_ms("maya", attempt=attempt)
                print(f"Restarting in {delay}ms...")
                time.sleep(delay / 1000)
                info = launcher.launch(
                    name="maya",
                    executable="/usr/autodesk/maya/bin/maya",
                    args=["-prompt"],
                )
                watcher.track(info["pid"], "maya")
                attempt += 1
            else:
                print("Max restarts exceeded")
                watcher.stop()
                exit(1)

    time.sleep(0.1)
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
    print(f"CPU: {info['cpu_usage_percent']}%")
```

### 3. Use Appropriate Timeouts

```python
# Short timeout for quick operations
launcher.terminate("quick_proc", timeout_ms=2000)

# Longer timeout for DCC apps
launcher.terminate("maya", timeout_ms=10000)
```

### 4. Monitor Resource Usage

```python
def check_resources():
    monitor.refresh()
    for info in monitor.list_all():
        if info["cpu_usage_percent"] > 90:
            print(f"High CPU: {info['name']}")
        if info["memory_bytes"] > 10 * 1024 * 1024 * 1024:
            print(f"High memory: {info['name']}")
```
