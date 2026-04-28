# Process API

`dcc_mcp_core` (process module)

Cross-platform DCC process monitoring, lifecycle management, and crash recovery.

## Overview

Provides:

- **Process Monitoring** — Live resource usage via `PyProcessMonitor` (CPU, memory, status)
- **DCC Launching** — Async spawn/terminate/kill via `PyDccLauncher`
- **Crash Recovery** — Restart policy with exponential/fixed backoff via `PyCrashRecoveryPolicy`
- **Background Watching** — Event polling via `PyProcessWatcher`

## PyProcessMonitor

Track and query process resource usage using `sysinfo`.

### Constructor

```python
from dcc_mcp_core import PyProcessMonitor

monitor = PyProcessMonitor()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `track(pid, name)` | `None` | Register a PID to monitor |
| `untrack(pid)` | `None` | Stop monitoring a PID |
| `refresh()` | `None` | Refresh underlying system data |
| `query(pid)` | `dict \| None` | Get snapshot for a PID |
| `list_all()` | `list[dict]` | Get snapshots for all tracked PIDs |
| `is_alive(pid)` | `bool` | Check if PID is in OS process table |
| `tracked_count()` | `int` | Number of tracked PIDs |

### Returned Dict Keys

| Key | Type | Description |
|-----|------|-------------|
| `pid` | `int` | Process ID |
| `name` | `str` | User-defined name |
| `status` | `str` | OS status string |
| `cpu_usage_percent` | `float` | CPU usage (0-100) |
| `memory_bytes` | `int` | Memory usage in bytes |
| `restart_count` | `int` | Restart count |

### Example

```python
import os
monitor = PyProcessMonitor()
monitor.track(os.getpid(), "self")
monitor.refresh()
info = monitor.query(os.getpid())
if info:
    print(f"CPU: {info['cpu_usage_percent']:.1f}%")
    print(f"Memory: {info['memory_bytes'] / 1024 / 1024:.1f} MB")
```

## PyDccLauncher

Async DCC process launcher (spawn / terminate / kill).

### Constructor

```python
from dcc_mcp_core import PyDccLauncher

launcher = PyDccLauncher()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `launch(name, executable, args=None, launch_timeout_ms=30000)` | `dict` | Spawn a DCC process |
| `terminate(name, timeout_ms=5000)` | `None` | Gracefully terminate |
| `kill(name)` | `None` | Force kill |
| `pid_of(name)` | `int \| None` | Get PID by name |
| `running_count()` | `int` | Number of live children |
| `restart_count(name)` | `int` | Restart count for name |

### Example

```python
info = launcher.launch(
    name="maya",
    executable="/usr/autodesk/maya/bin/maya",
    args=["-prompt", "-batch"],
    launch_timeout_ms=30000,
)
print(f"Launched PID: {info['pid']}")

# Terminate
launcher.terminate("maya", timeout_ms=5000)
```

## PyCrashRecoveryPolicy

Crash recovery policy for DCC processes.

### Constructor

```python
from dcc_mcp_core import PyCrashRecoveryPolicy

policy = PyCrashRecoveryPolicy(max_restarts=3)
```

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `max_restarts` | `int` | Maximum restart count (constructor arg, default 3) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `use_exponential_backoff(initial_ms, max_delay_ms)` | `None` | Use exponential backoff |
| `use_fixed_backoff(delay_ms)` | `None` | Use fixed delay |
| `should_restart(status)` | `bool` | Check if status warrants restart |
| `next_delay_ms(name, attempt)` | `int` | Get delay before attempt |

### Example

```python
policy = PyCrashRecoveryPolicy(max_restarts=3)
policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)

if policy.should_restart("crashed"):
    delay = policy.next_delay_ms("maya", attempt=0)
    print(f"Restarting in {delay}ms...")
```

### Recognised Status Values

| Status | Description |
|--------|-------------|
| `"crashed"` | Process crashed |
| `"unresponsive"` | Process unresponsive |

## PyProcessWatcher

Async background process watcher with event polling.

### Constructor

```python
from dcc_mcp_core import PyProcessWatcher

watcher = PyProcessWatcher(poll_interval_ms=500)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `track(pid, name)` | `None` | Register a PID to monitor |
| `untrack(pid)` | `None` | Stop monitoring a PID |
| `start()` | `None` | Start background watch loop |
| `stop()` | `None` | Stop background watch loop |
| `poll_events()` | `list[dict]` | Drain pending events |
| `is_running()` | `bool` | Check if loop is running |
| `tracked_count()` | `int` | Number of tracked PIDs |

### Event Types

Event dicts contain: `type`, `pid`, `name`

| Event Type | Additional Fields |
|------------|-------------------|
| `heartbeat` | `new_status`, `cpu_usage_percent`, `memory_bytes` |
| `status_changed` | `old_status`, `new_status` |
| `exited` | — |

### Example

```python
import os, time
watcher = PyProcessWatcher(poll_interval_ms=200)
watcher.track(os.getpid(), "self")
watcher.start()

time.sleep(0.5)

for event in watcher.poll_events():
    print(f"Event: {event['type']} - {event['name']}")

watcher.stop()
```

## Integration Example

### Auto-Restart DCC

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

---

## GUI Executable Detection (issue #524)

DCC adapters routinely need to answer two questions before spawning a child
process: *is this path a GUI binary that will pop a window?* and *is there a
headless Python sibling I should prefer?*. The two helpers below + the
`GuiExecutableHint` value type lift that logic out of every plugin's
`server.py` and into a vendor-curated table in `dcc_mcp_core`.

**Exported symbols:** `is_gui_executable`, `correct_python_executable`,
`GuiExecutableHint`.

### is_gui_executable

```python
is_gui_executable(path: str) -> GuiExecutableHint | None
```

Probe `path` against the bundled DCC table. Returns `None` for Python
interpreters (`python.exe`, `mayapy`, `hython` …) and unknown vendor binaries.

```python
from dcc_mcp_core import is_gui_executable

hint = is_gui_executable(r"C:\Program Files\Autodesk\Maya2024\bin\maya.exe")
if hint is not None:
    print(hint.dcc_kind)               # "maya"
    print(hint.recommended_replacement) # PosixPath('.../bin/mayapy.exe')
```

### correct_python_executable

```python
correct_python_executable(path: str) -> pathlib.Path
```

If `path` is a known DCC GUI binary with a headless-Python sibling on disk,
return that sibling path; otherwise return `path` unchanged. Convenience for
one-shot fixing of `DCC_MCP_PYTHON_EXECUTABLE` before spawning a launcher
child:

```python
import os
from dcc_mcp_core import correct_python_executable

os.environ["DCC_MCP_PYTHON_EXECUTABLE"] = str(
    correct_python_executable(os.environ.get("DCC_MCP_PYTHON_EXECUTABLE", ""))
)
```

### GuiExecutableHint

Frozen value type returned by `is_gui_executable`. Not user-instantiable.

| Property | Type | Description |
|----------|------|-------------|
| `gui_path` | `pathlib.Path` | The path that was probed |
| `dcc_kind` | `str` | DCC family name (`"maya"`, `"houdini"`, `"unreal"`, `"blender"`, `"3dsmax"`, …) |
| `recommended_replacement` | `pathlib.Path \| None` | Headless sibling resolved on disk, or `None` |

`dcc_kind` is a stable wire string — adapters can pivot on it for skill scope
selection (e.g. only auto-load `maya/*` skills when `dcc_kind == "maya"`).
