# Telemetry API

`dcc_mcp_core` (telemetry module)

Action performance recording and optional OpenTelemetry tracing/metrics.

## Overview

Provides:

- **Action Recording** — Per-action timing and success/failure counters via `ActionRecorder`
- **Metrics** — `ActionMetrics` snapshot with latency percentiles (p95/p99) and success rate
- **Optional OpenTelemetry** — stdout exporter, JSON/text logs, resource attributes (opt-in)

## ActionRecorder

Record execution time and outcomes for any action.

### Constructor

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-service")
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start(action_name, dcc_name)` | `RecordingGuard` | Start timing an action |
| `metrics(action_name)` | `ActionMetrics \| None` | Get metrics for an action |
| `all_metrics()` | `list[ActionMetrics]` | Get all action metrics |
| `reset()` | `None` | Reset all statistics |

### Recording with Guard

```python
guard = recorder.start("create_sphere", "maya")
# ... perform work ...
guard.finish(success=True)
```

### Context Manager Usage

```python
with recorder.start("create_sphere", "maya") as guard:
    # ... perform work ...
# guard.finish(success=True) called automatically on success
# guard.finish(success=False) called on exception
```

## ActionMetrics

Read-only snapshot of per-Action performance metrics.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `action_name` | `str` | Name of the action |
| `invocation_count` | `int` | Total invocations |
| `success_count` | `int` | Successful invocations |
| `failure_count` | `int` | Failed invocations |
| `avg_duration_ms` | `float` | Average duration in ms |
| `p95_duration_ms` | `float` | 95th percentile duration |
| `p99_duration_ms` | `float` | 99th percentile duration |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `success_rate()` | `float` | Success ratio (0.0-1.0) |

### Example

```python
metrics = recorder.metrics("create_sphere")
if metrics:
    print(f"Invocations: {metrics.invocation_count}")
    print(f"Success rate: {metrics.success_rate():.2%}")
    print(f"P95: {metrics.p95_duration_ms:.2f}ms")
```

## RecordingGuard

RAII guard returned by `ActionRecorder.start()`.

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `finish(success)` | `None` | Finish recording with success flag |
| `__enter__()` | `RecordingGuard` | Context manager entry |
| `__exit__()` | `None` | Context manager exit (sets success=False on exception) |

## TelemetryConfig

Optional OpenTelemetry configuration.

### Constructor

```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-dcc-service")
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `with_stdout_exporter()` | `TelemetryConfig` | Use stdout exporter |
| `with_noop_exporter()` | `TelemetryConfig` | Use no-op exporter (testing) |
| `with_json_logs()` | `TelemetryConfig` | Use JSON log format |
| `with_text_logs()` | `TelemetryConfig` | Use text log format |
| `with_attribute(key, value)` | `TelemetryConfig` | Add resource attribute |
| `with_service_version(version)` | `TelemetryConfig` | Set service version |
| `set_enable_metrics(enabled)` | `TelemetryConfig` | Enable/disable metrics |
| `set_enable_tracing(enabled)` | `TelemetryConfig` | Enable/disable tracing |
| `init()` | `None` | Install as global provider |

### Example

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_json_logs()
cfg.with_service_version("1.0.0")
cfg.init()
```

## is_telemetry_initialized()

Check if global telemetry provider is installed.

```python
from dcc_mcp_core import is_telemetry_initialized

if is_telemetry_initialized():
    print("Telemetry is active")
```

## Integration Example

### Maya Integration

```python
from dcc_mcp_core import ActionRecorder
import maya.cmds as cmds

recorder = ActionRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.start("create_sphere", "maya") as guard:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        guard.finish(success=True)
        return sphere
```
