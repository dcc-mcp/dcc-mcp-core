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
| `before(event_name, callback)` | `int` | Register a blocking veto hook for a supported lifecycle event |
| `unsubscribe(event_name, subscriber_id)` | `bool` | Unsubscribe by ID. Returns True if found |
| `unsubscribe_before(event_name, subscriber_id)` | `bool` | Remove a before hook by ID |
| `publish(event_name, **kwargs)` | `None` | Call all matching subscribers with kwargs |
| `emit(event_name, source=None, correlation=None, attributes=None)` | `dict` | Emit a structured event envelope and pass it to all matching subscribers |
| `veto(reason, code="vetoed")` | `dict` | Build a veto payload for a before hook |
| `vetoable_events()` | `list[str]` | Return lifecycle events that accept before hooks |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `EventBus(subscriptions=N)` |

### Behavior

- Subscribers receive keyword arguments from `publish(event_name, **kwargs)`
- Subscribers receive one event-envelope dict from `emit(...)`
- Event names support exact matches, `prefix.*` wildcards, and a catch-all `*`
- Exceptions in subscribers are logged via `tracing` but do not propagate
- Before hooks support only `skill.loading`, `tool.dispatched`,
  `resource.subscribed`, and `client.initialize`
- Before hooks return `None`/`False` to allow, or a string/dict/`veto(...)`
  payload to reject
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

### Structured Event Envelope

```python
events = []
bus.subscribe("tool.*", lambda event: events.append(event))

event = bus.emit(
    "tool.completed",
    source={"dcc_type": "maya"},
    correlation={"request_id": "req-123"},
    attributes={"tool_slug": "maya_scene__open", "result_success": True},
)

assert event["schema_version"] == 1
assert events == [event]
```

### Before Hook Veto

```python
def policy(event):
    if event["attributes"]["tool_slug"] == "delete_scene":
        return EventBus.veto("destructive tools are disabled", "policy_denied")
    return None

sid = bus.before("tool.dispatched", policy)
bus.unsubscribe_before("tool.dispatched", sid)
```

Tool vetoes surface as `EVENT_VETOED` dispatch errors and `tool.failed` events
with `error_kind="event_vetoed"`, `veto_code`, and `veto_reason`.

Envelope fields:

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | `int` | Event envelope schema version, currently `1` |
| `name` | `str` | Dotted event name |
| `id` | `str` | Opaque event id prefixed with `ev_` |
| `timestamp_ns` | `int` | Unix timestamp in nanoseconds |
| `source` | `dict` | Emitter identity such as `dcc_type` |
| `correlation` | `dict` | Request/session/trace correlation fields when available |
| `attributes` | `dict` | Event-specific payload |

For `tool.completed`, `attributes.result_success` follows the handler output's
`success` boolean when present. If the output has no `success` field, a handler
that returned normally is treated as successful.

### Standalone Server Webhooks

`dcc-mcp-server` can forward structured EventBus envelopes to HTTP webhooks.
Set `DCC_MCP_WEBHOOKS_CONFIG` to a YAML file, or place
`webhooks.yaml` under `~/dcc-mcp/etc` (`DCC_MCP_ETC_DIR` overrides that
directory). The Admin UI Integrations panel writes the local file path and marks
the integration as `pending_restart` because webhook runtimes are loaded when
the server starts.

Each `webhooks` entry supports `name`, `url`, `events`, optional `kind`,
optional `headers`, optional `delivery` retry settings, optional dotted-path
`filters`, and optional `payload_template`. Delivery is asynchronous and
bounded; when all attempts fail the server emits `webhook.delivery_failed` on
the same EventBus.

:::: v-pre

```yaml
queue_capacity: 1024
webhooks:
  - name: studio-events
    url: https://ops.example.invalid/dcc-mcp-events
    events:
      - tool.failed
      - gateway.instance.*
    headers:
      authorization: Bearer ${DCC_EVENTS_TOKEN}
    filters:
      - source.dcc_type: maya
    delivery:
      attempts: 3
      timeout_ms: 2000
      backoff_ms: [200, 1000, 5000]
    payload_template: |
      {"event":"{{name}}","tool":"{{attributes.tool_slug}}","dcc":"{{source.dcc_type}}"}
```

::::

`payload_template` resolves `&#123;&#123;path.to.field&#125;&#125;` placeholders against the
structured event envelope. Omit it to send the full envelope as JSON.

### WeCom Message Push

Enterprise WeChat group robots can be configured as webhook entries with
`kind: wecom`. The runtime sends markdown payloads in the format expected by
the robot endpoint.

```yaml
webhooks:
  - name: wecom-message-push
    kind: wecom
    url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=${WECOM_ROBOT_KEY}
    events:
      - tool.failed
      - webhook.delivery_failed
    message_template: |
      DCC-MCP $event
      DCC: $dcc-type
      Tool: $tool-slug
      URL: $url
```

`message_template` supports both <code v-pre>{{source.dcc_type}}</code> envelope paths and
dollar variables. Built-in variables include `$event`, `$event-id`,
`$dcc-type`, `$instance-id`, `$tool-slug`, `$skill-name`, and `$url`.

You can also enable the same integration without YAML:

| Variable | Default | Description |
|----------|---------|-------------|
| `DCC_MCP_WECOM_WEBHOOK_URL` | disabled | Enterprise WeChat group robot webhook URL |
| `DCC_MCP_WECOM_EVENTS` | `tool.failed, webhook.delivery_failed` | Comma or newline separated event patterns |
| `DCC_MCP_WECOM_TEMPLATE` | built-in markdown template | Message body with `$...` variables |

When configured from the Admin UI, WeCom is saved as the
`wecom-message-push` entry in the shared local `webhooks.yaml`. Saving it
preserves unrelated webhook entries and replaces only an existing `kind: wecom`
or `name: wecom-message-push` entry.

---

## ToolRecorder

Records per-tool execution time and success/failure counters. Use this to collect performance telemetry for any tools your code executes.

### Constructor

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("my-service")
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `scope` | `str` | Logical name for this recorder instance (e.g. service or module name) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `start(action_name, dcc_name)` | `RecordingGuard` | Start timing a tool; returns a RAII guard |
| `metrics(action_name)` | `ToolMetrics \| None` | Aggregated metrics for a specific tool; `None` if no data |
| `all_metrics()` | `list[ToolMetrics]` | Aggregated metrics for all recorded actions |
| `reset()` | `None` | Clear all in-memory statistics |

### Example

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("maya-skill-server")

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

RAII guard returned by `ToolRecorder.start()`. Automatically records the duration and outcome.

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `finish(success)` | `None` | Commit the recording with the given success flag |
| `__enter__` | `RecordingGuard` | Context manager entry |
| `__exit__` | `None` | Context manager exit (success=True when no exception was raised) |

---

## ToolMetrics

Read-only snapshot of per-tool performance metrics. Obtained from `ToolRecorder.metrics()` or `ToolRecorder.all_metrics()`.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `action_name` | `str` | Tool this metric belongs to |
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
recorder = ToolRecorder("server")

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
