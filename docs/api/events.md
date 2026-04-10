# Events API

`dcc_mcp_core.EventBus`

## EventBus

Thread-safe publish/subscribe event bus backed by DashMap.

### Constructor

```python
from dcc_mcp_core import EventBus
bus = EventBus()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `subscribe(event_name, callback)` | `int` | Subscribe a callable. Returns subscriber ID |
| `unsubscribe(event_name, subscriber_id)` | `bool` | Unsubscribe by ID. Returns True if found |
| `publish(event_name, **kwargs)` | — | Call all subscribers with kwargs |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `EventBus(subscriptions=N)` |

### Behavior

- Subscribers receive keyword arguments from `publish(event_name, **kwargs)`
- Exceptions in subscribers are logged via `tracing` but do not propagate
- Callbacks are collected before invocation to avoid DashMap deadlocks
- Multiple subscribers per event are supported
- Subscriber IDs are monotonically increasing (starting at 1)

### Example

```python
bus = EventBus()

def on_action(action_name=None, **kwargs):
    print(f"Action: {action_name}")

sid = bus.subscribe("action.executed", on_action)
bus.publish("action.executed", action_name="create_sphere")
bus.unsubscribe("action.executed", sid)
```

---

## ActionRecorder

Records per-action execution time and success/failure counters. Use this to collect performance telemetry for any actions your code executes.

### Constructor

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("my-service")
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `scope` | `str` | Logical name for this recorder instance (e.g. service or module name) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start(action_name, dcc_name)` | `RecordingGuard` | Start timing an action; returns a RAII guard |
| `metrics(action_name)` | `ActionMetrics \| None` | Aggregated metrics for a specific action; `None` if no data |
| `all_metrics()` | `list[ActionMetrics]` | Aggregated metrics for all recorded actions |
| `reset()` | `None` | Clear all in-memory statistics |

### Example

```python
from dcc_mcp_core import ActionRecorder

recorder = ActionRecorder("maya-skill-server")

# Manual guard usage
guard = recorder.start("create_sphere", "maya")
try:
    # ... do work ...
    guard.finish(success=True)
except Exception:
    guard.finish(success=False)
    raise

# Context manager usage (success=True if no exception)
with recorder.start("delete_mesh", "maya"):
    pass  # work here

# Query metrics
m = recorder.metrics("create_sphere")
if m:
    print(f"calls={m.invocation_count}, success_rate={m.success_rate():.2%}")
    print(f"avg={m.avg_duration_ms:.1f}ms  p95={m.p95_duration_ms:.1f}ms")
```

---

## RecordingGuard

RAII guard returned by `ActionRecorder.start()`. Automatically records the duration and outcome.

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `finish(success)` | `None` | Commit the recording with the given success flag |
| `__enter__` | `RecordingGuard` | Context manager entry |
| `__exit__` | `None` | Context manager exit (success=True when no exception was raised) |

---

## ActionMetrics

Read-only snapshot of per-action performance metrics. Obtained from `ActionRecorder.metrics()` or `ActionRecorder.all_metrics()`.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `action_name` | `str` | Action this metric belongs to |
| `invocation_count` | `int` | Total number of calls recorded |
| `success_count` | `int` | Number of successful calls |
| `failure_count` | `int` | Number of failed calls |
| `avg_duration_ms` | `float` | Mean execution time in milliseconds |
| `p95_duration_ms` | `float` | 95th-percentile execution time in milliseconds |
| `p99_duration_ms` | `float` | 99th-percentile execution time in milliseconds |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `success_rate()` | `float` | Success ratio in `[0.0, 1.0]` |

### Example

```python
recorder = ActionRecorder("server")

for _ in range(10):
    with recorder.start("ping", "maya"):
        pass

all_m = recorder.all_metrics()
for m in all_m:
    print(
        f"{m.action_name}: "
        f"{m.invocation_count} calls, "
        f"{m.success_rate():.0%} success, "
        f"avg {m.avg_duration_ms:.1f}ms"
    )
```
