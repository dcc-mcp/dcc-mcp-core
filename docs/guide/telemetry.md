# Telemetry Guide

Operation performance recording and optional OpenTelemetry tracing/metrics.

## Overview

Provides:

- **Operation recording** — `ToolRecorder` records execution time and success/failure counts for each operation
- **Metrics** — `ToolMetrics` snapshots with latency percentiles (p95/p99) and success rates
- **Optional OpenTelemetry** — stdout exporter, JSON/text logs, resource attributes (optional)

## ToolRecorder

Records execution time and results for any operation.

### Quick Start

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("my-service")

# Record with guard
guard = recorder.start("create_sphere", "maya")
# ... do work ...
guard.finish(success=True)
```

### Guard Pattern (Recommended)

The guard returned by `start()` is an RAII context manager:

```python
# Automatically success=True unless an exception is raised
with recorder.start("create_sphere", "maya") as guard:
    # ... do work ...
    pass
# guard.finish(success=True) is called automatically
```

Manual finish when you need explicit control:

```python
guard = recorder.start("delete_mesh", "houdini")
try:
    delete_mesh("Cube")
    guard.finish(success=True)
except Exception:
    guard.finish(success=False)
    raise
```

### Querying Metrics

```python
# Get metrics for a specific operation
metrics = recorder.metrics("create_sphere")
if metrics:
    print(f"Operation: {metrics.action_name}")
    print(f"Invocations: {metrics.invocation_count}")
    print(f"Success rate: {metrics.success_rate():.2%}")
    print(f"Avg duration: {metrics.avg_duration_ms:.2f}ms")
    print(f"P95: {metrics.p95_duration_ms:.2f}ms")
    print(f"P99: {metrics.p99_duration_ms:.2f}ms")
```

### All Metrics

```python
# Get metrics for all recorded operations
all_metrics = recorder.all_metrics()

for m in all_metrics:
    print(f"{m.action_name}: {m.invocation_count} calls, {m.success_rate():.1%} success")
```

### Reset

```python
recorder.reset()  # Clear all in-memory statistics
```

## TelemetryConfig

Optional OpenTelemetry configuration. Not needed when using `ToolRecorder` alone.

### Basic Setup

```python
from dcc_mcp_core import TelemetryConfig

cfg = TelemetryConfig("my-dcc-service")
cfg.init()
```

### Console Exporter

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.init()
```

### JSON Logs (Production)

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_json_logs()
cfg.with_service_version("1.0.0")
cfg.init()
```

### Text Logs (Default)

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_text_logs()
cfg.init()
```

### No-op Exporter (Testing)

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_noop_exporter()
cfg.init()
```

### Custom Attributes

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.with_attribute("dcc.name", "maya")
cfg.with_attribute("dcc.version", "2025")
cfg.init()
```

### Enable/Disable Features

```python
cfg = TelemetryConfig("my-dcc-service")
cfg.with_stdout_exporter()
cfg.set_enable_metrics(False)   # Disable metrics collection
cfg.set_enable_tracing(False)   # Disable distributed tracing
cfg.init()
```

### Check Initialization

```python
from dcc_mcp_core import is_telemetry_initialized

if is_telemetry_initialized():
    print("Telemetry is active")
```

## Maya Integration

```python
from dcc_mcp_core import ToolRecorder
import maya.cmds as cmds

recorder = ToolRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.start("create_sphere", "maya") as guard:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        guard.finish(success=True)
        return sphere

def traced_delete(object_name):
    with recorder.start("delete_object", "maya") as guard:
        cmds.delete(object_name)
        guard.finish(success=True)
```

## Blender Integration

```python
from dcc_mcp_core import ToolRecorder
import bpy

recorder = ToolRecorder("blender")

def traced_blender_operation(operation_name, func):
    with recorder.start(operation_name, "blender") as guard:
        result = func()
        guard.finish(success=True)
        return result
```

## Multi-DCC Tracing

```python
# One recorder per DCC
maya_recorder = ToolRecorder("maya")
blender_recorder = ToolRecorder("blender")
houdini_recorder = ToolRecorder("houdini")

# Each maintains independent metrics
```

## Best Practices

### 1. Use Context Managers

```python
with recorder.start("my_action", "maya") as guard:
    perform_action()
# Success/failure is recorded automatically
```

### 2. Explicitly Catch Exceptions

```python
with recorder.start("risky_action", "maya") as guard:
    try:
        risky_operation()
        guard.finish(success=True)
    except Exception:
        guard.finish(success=False)
        raise
```

### 3. Query Metrics After Batch Operations

```python
# Run many operations
for i in range(100):
    with recorder.start("batch_op", "maya"):
        do_work(i)

# Check aggregate metrics
metrics = recorder.metrics("batch_op")
if metrics:
    print(f"P99 latency: {metrics.p99_duration_ms}ms")
```
