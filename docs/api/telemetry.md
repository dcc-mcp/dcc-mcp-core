# Telemetry API

`dcc_mcp_core` (telemetry module)

OpenTelemetry tracing, metrics, and structured logging.

## Overview

Provides observability for the DCC-MCP ecosystem:

- **Tracing** — Distributed tracing for action execution
- **Metrics** — Per-action timing and success-rate metrics
- **Logging** — Structured logging with OpenTelemetry integration
- **Exporters** — Support for OTLP gRPC (Jaeger, Grafana Tempo, Prometheus)

## TelemetryConfig

Configuration for the telemetry provider.

### Constructor

```python
from dcc_mcp_core import TelemetryConfig, ExporterBackend, LogFormat

config = TelemetryConfig.builder("my-dcc-service") \
    .with_exporter(ExporterBackend.CONSOLE) \
    .with_log_format(LogFormat.JSON) \
    .with_service_version("1.0.0") \
    .build()
```

### Exporter Backends

| Backend | Description |
|---------|-------------|
| `NOOP` | No-op exporter (for testing) |
| `CONSOLE` | Print to stdout |
| `OTLP_GRPC` | Export to OTLP gRPC endpoint |

### Log Formats

| Format | Description |
|--------|-------------|
| `JSON` | Structured JSON logs |
| `PRETTY` | Human-readable format |

## Initialization

```python
from dcc_mcp_core import init_telemetry, is_telemetry_initialized, shutdown_telemetry

# Initialize at startup
init_telemetry(config)

# Check if initialized
if is_telemetry_initialized():
    print("Telemetry is active")

# Shutdown at exit
shutdown_telemetry()
```

## ActionRecorder

Record metrics for action invocations.

### Constructor

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-dcc-service")
```

### Recording Actions

```python
# Start a recording
guard = recorder.start("create_sphere", "maya")

# ... perform action work ...

# Finish with success or failure
guard.finish(success=True)
```

### Recording with Context Manager

```python
with recorder.record("list_objects", "blender") as metrics:
    # ... perform work ...
    pass  # Automatically finishes with success=True

# Or with explicit result
guard = recorder.record("delete_mesh", "houdini")
# ... work ...
guard.finish(success=False, error="Object not found")
```

### Querying Metrics

```python
metrics = recorder.metrics("create_sphere")
print(f"Invocations: {metrics.invocation_count}")
print(f"Success rate: {metrics.success_rate():.2%}")
print(f"P50 latency: {metrics.latency_p50_ms}ms")
print(f"P95 latency: {metrics.latency_p95_ms}ms")
print(f"P99 latency: {metrics.latency_p99_ms}ms")
```

### ActionMetrics

| Field | Type | Description |
|-------|------|-------------|
| `action_name` | `str` | Name of the action |
| `invocation_count` | `int` | Total invocations |
| `success_count` | `int` | Successful invocations |
| `failure_count` | `int` | Failed invocations |
| `success_rate()` | `float` | Success ratio (0-1) |
| `latency_p50_ms` | `float` | 50th percentile latency |
| `latency_p95_ms` | `float` | 95th percentile latency |
| `latency_p99_ms` | `float` | 99th percentile latency |

## Tracing Spans

Create custom spans for detailed tracing.

### Python Tracing

```python
from dcc_mcp_core import tracer, action_span

# Get a tracer
t = tracer("my-component")

# Create a span manually
with t.start_as_current_span("my_operation") as span:
    span.set_attribute("key", "value")
    span.add_event("event_name", {"attr": "value"})
    # ... work ...

# Using the action_span helper
with action_span("create_sphere", dcc="maya") as span:
    span.set_attribute("radius", 1.0)
```

### Span Attributes

| Attribute | Type | Description |
|-----------|------|-------------|
| `dcc.name` | `str` | DCC application name |
| `dcc.version` | `str` | DCC version |
| `action.name` | `str` | Action name |
| `action.category` | `str` | Action category |

## Error Handling

```python
from dcc_mcp_core import TelemetryError

try:
    init_telemetry(config)
except TelemetryError as e:
    print(f"Telemetry initialization failed: {e}")
```

## OTLP Export

### OTLP gRPC Configuration

```python
config = TelemetryConfig.builder("my-service") \
    .with_exporter(ExporterBackend.OTLP_GRPC) \
    .with_otlp_endpoint("http://localhost:4317") \
    .with_otlp_headers({"Authorization": "Bearer token"}) \
    .build()
```

## Integration Examples

### Maya Integration

```python
from dcc_mcp_core import init_telemetry, ActionRecorder

# Initialize when Maya starts
init_telemetry(config)

# Record each action
recorder = ActionRecorder("maya")

def execute_action(action_name, params):
    with recorder.record(action_name, "maya") as metrics:
        # Call Maya API
        result = maya_cmds.sphere(radius=params.get("radius", 1.0))
        return result
```

### Decorator Usage

```python
from dcc_mcp_core import traced

@traced(action_name="create_sphere", dcc="maya")
def create_sphere(radius=1.0, name=None):
    # This function is automatically traced
    return maya_cmds.sphere(r=radius, n=name)
```
