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
