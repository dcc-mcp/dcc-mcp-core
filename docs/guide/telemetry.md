# Telemetry Guide

OpenTelemetry tracing, metrics, and structured logging.

## Overview

Provides observability for the DCC-MCP ecosystem:

- **Tracing** — Distributed tracing for action execution
- **Metrics** — Per-action timing and success-rate metrics
- **Logging** — Structured logging with OpenTelemetry integration
- **Exporters** — Support for OTLP gRPC (Jaeger, Grafana Tempo, Prometheus)

## Quick Start

### Basic Initialization

```python
from dcc_mcp_core import TelemetryConfig, init_telemetry, is_telemetry_initialized

# Initialize telemetry
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .build()

init_telemetry(config)

print(f"Initialized: {is_telemetry_initialized()}")
```

### Recording Actions

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-dcc-service")

# Record an action
guard = recorder.start("create_sphere", "maya")
# ... perform Maya operations ...
guard.finish(success=True)
```

### Context Manager Pattern

```python
with recorder.record("list_objects", "blender") as metrics:
    # Action executes automatically
    # Metrics are recorded when the context exits
    pass

# Access metrics
print(f"Success rate: {metrics.success_rate():.2%}")
```

## Configuration

### Console Exporter (Development)

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .with_log_format("pretty") \
    .build()
```

### JSON Logging (Production)

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("console") \
    .with_log_format("json") \
    .with_service_version("1.0.0") \
    .build()
```

### OTLP Export (Enterprise)

```python
config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter("otlp_grpc") \
    .with_otlp_endpoint("http://jaeger:4317") \
    .with_otlp_headers({"Authorization": "Bearer token"}) \
    .with_service_version("1.0.0") \
    .build()
```

## Action Metrics

### Recording with Guard

```python
recorder = ActionRecorder("maya-service")

# Start recording
guard = recorder.start("create_sphere", "maya")

# ... perform the action ...

# Finish with explicit success/failure
guard.finish(success=True)
```

### Automatic Recording

```python
with recorder.record("delete_mesh", "houdini") as metrics:
    # Perform the action
    delete_mesh("Cube")

# If no exception: success=True
# If exception raised: success=False automatically
```

### Querying Metrics

```python
# Get metrics for a specific action
metrics = recorder.metrics("create_sphere")

print(f"Action: {metrics.action_name}")
print(f"Invocations: {metrics.invocation_count}")
print(f"Success rate: {metrics.success_rate():.2%}")
print(f"P50 latency: {metrics.latency_p50_ms}ms")
print(f"P95 latency: {metrics.latency_p95_ms}ms")
print(f"P99 latency: {metrics.latency_p99_ms}ms")
```

### Aggregated Metrics

```python
# Get all metrics
all_metrics = recorder.get_all_metrics()

for name, m in all_metrics.items():
    print(f"{name}: {m.invocation_count} calls, {m.success_rate():.1%} success")
```

## Tracing

### Action Spans

```python
from dcc_mcp_core import action_span

# Create a span for an action
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", 1.0)
    span.set_attribute("segments", 32)

    # ... perform the action ...

    span.add_event("completed", {"sphere_name": "sphere1"})
```

### Custom Spans

```python
from dcc_mcp_core import tracer

# Get a tracer for your component
t = tracer("my-component")

# Create a span
with t.start_as_current_span("my_operation") as span:
    span.set_attribute("key", "value")
    span.add_event("event_name", {"attr": "value"})

    # ... work ...

    if error:
        span.record_exception(error)
```

### Span Attributes

Common attributes for DCC operations:

| Attribute | Type | Description |
|-----------|------|-------------|
| `dcc.name` | `string` | DCC application name |
| `dcc.version` | `string` | DCC version |
| `action.name` | `string` | Action name |
| `action.category` | `string` | Action category |
| `user.id` | `string` | User identifier |

## Integration Examples

### Maya Integration

```python
from dcc_mcp_core import init_telemetry, ActionRecorder
import maya.cmds as cmds

# Initialize on Maya startup
init_telemetry(config)
recorder = ActionRecorder("maya")

def traced_create_sphere(radius=1.0, name=None):
    with recorder.record("create_sphere", "maya") as metrics:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        return sphere

def traced_delete(object_name):
    with recorder.record("delete_object", "maya") as metrics:
        cmds.delete(object_name)
```

### Blender Integration

```python
import bpy
from dcc_mcp_core import init_telemetry, ActionRecorder

init_telemetry(config)
recorder = ActionRecorder("blender")

def traced_blender_operation(operation_name, func):
    with recorder.record(operation_name, "blender"):
        return func()

# Usage
result = traced_blender_operation("create_cube", lambda: bpy.ops.mesh.primitive_cube_add())
```

### Multi-DCC Tracing

```python
from dcc_mcp_core import init_telemetry, ActionRecorder

init_telemetry(config)

# One recorder per DCC
maya_recorder = ActionRecorder("maya")
blender_recorder = ActionRecorder("blender")
houdini_recorder = ActionRecorder("houdini")

# Each recorder maintains separate metrics
```

## OTLP Export Configuration

### Jaeger

```python
config = TelemetryConfig.builder("my-service") \
    .with_exporter("otlp_grpc") \
    .with_otlp_endpoint("http://jaeger:4317") \
    .build()
```

### Grafana Tempo

```python
config = TelemetryConfig.builder("my-service") \
    .with_exporter("otlp_grpc") \
    .with_otlp_endpoint("http://tempo:4317") \
    .build()
```

### Prometheus

```python
config = TelemetryConfig.builder("my-service") \
    .with_exporter("otlp_grpc") \
    .with_otlp_endpoint("http://prometheus:4317") \
    .build()
```

## Best Practices

### 1. Initialize Early

```python
# On application startup
def initialize_dcc_mcp():
    config = TelemetryConfig.builder("my-dcc") \
        .with_exporter("console") \
        .build()
    init_telemetry(config)
```

### 2. Use Context Managers

```python
# Automatic cleanup and error handling
with recorder.record("my_action", "maya") as metrics:
    perform_action()
# Success/failure automatically recorded
```

### 3. Add Meaningful Attributes

```python
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", radius)
    span.set_attribute("segments", segments)
    span.set_attribute("user", current_user)
```

### 4. Shutdown Gracefully

```python
import atexit

def cleanup():
    shutdown_telemetry()

atexit.register(cleanup)
```

## Metrics Dashboard

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `invocation_count` | Total action calls | — |
| `success_rate` | Success ratio | < 95% |
| `latency_p99` | 99th percentile latency | > 1000ms |
| `error_rate` | Error ratio | > 5% |

### Example Dashboard Queries

```promql
# Success rate by action
sum(rate(action_success_total[5m])) by (action_name)
/
sum(rate(action_total[5m])) by (action_name)

# P99 latency
histogram_quantile(0.99, rate(action_duration_seconds_bucket[5m]))

# Error rate
sum(rate(action_errors_total[5m])) by (action_name)
```
