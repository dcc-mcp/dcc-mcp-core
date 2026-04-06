# Event System

DCC-MCP-Core provides a publish/subscribe event system via the `EventBus` for decoupled action lifecycle communication.

## Usage

```python
from dcc_mcp_core import EventBus

bus = EventBus()

def on_sphere_done(event, **kwargs):
    print(f"Event: {event}")
    # kwargs contains event-specific data

# Subscribe to an event — returns an integer subscriber ID
sub_id = bus.subscribe("action.after_execute.create_sphere", on_sphere_done)

# Unsubscribe using the event name and subscriber ID
bus.unsubscribe("action.after_execute.create_sphere", sub_id)

# Publish manually
bus.publish("my_custom_event", data="value")
```

::: tip
`subscribe()` returns a **subscriber ID** (integer), not the callback. Pass that ID to `unsubscribe()`, not the callback.
:::

## Event Discovery

The `EventBus` is a generic pub/sub system. What events are published depends on the DCC adapter or service that uses it. Consult your DCC-specific adapter documentation for the full list of events it publishes.

Common patterns:

| Event Pattern | Description |
|---------------|-------------|
| `action.before_execute.{name}` | Before executing a specific action |
| `action.after_execute.{name}` | After a specific action completes |
| `action.error.{name}` | When a specific action raises an error |

## Wildcard Subscriptions

The event bus supports `*` as a wildcard in event names:

```python
bus = EventBus()

def on_any_after_execute(event, **kwargs):
    print(f"Action completed: {event}")

# Subscribe to all "after_execute" events
id1 = bus.subscribe("action.after_execute.*", on_any_after_execute)

# Subscribe to all events
id2 = bus.subscribe("*", on_any_event)
```

## Publishing Events

Publish custom events for decoupled communication:

```python
bus = EventBus()

# Publish with keyword arguments
bus.publish("scene.saved", file_path="/tmp/scene.usda", size_kb=1024)
bus.publish("scene.opened", file_path="/tmp/scene.usda")
```

## Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `EventBus(subscribers=N)` |
