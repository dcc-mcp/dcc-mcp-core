# Event System

DCC-MCP-Core provides a publish/subscribe event system via the `EventBus` for decoupled action lifecycle communication.

## Usage

```python
from dcc_mcp_core.actions.events import event_bus

def on_action_done(data):
    print(f"Action {data['action_name']} completed: {data['result'].success}")

# Subscribe to events
event_bus.subscribe("action.after_execute.create_sphere", on_action_done)

# Unsubscribe when done
event_bus.unsubscribe("action.after_execute.create_sphere", on_action_done)
```

## Built-in Events

Events published automatically by ActionManager:

| Event | Description |
|-------|-------------|
| `action_manager.created` | Manager instance created |
| `action_manager.before_discover_path` | Before discovering actions from a path |
| `action_manager.after_discover_path` | After discovering actions from a path |
| `action_manager.before_refresh` | Before refreshing all actions |
| `action_manager.after_refresh` | After refreshing all actions |
| `action.before_execute.{name}` | Before executing a specific action |
| `action.after_execute.{name}` | After executing a specific action |
| `action.error.{name}` | When an action raises an error |
| `skill.loaded` | When a skill package is loaded |

## Event Data

Event handlers receive a `data` dictionary containing relevant information:

```python
def on_before_execute(data):
    action_name = data["action_name"]
    kwargs = data.get("kwargs", {})
    print(f"About to execute {action_name} with {kwargs}")

def on_after_execute(data):
    action_name = data["action_name"]
    result = data["result"]  # ActionResultModel
    print(f"{action_name}: {'OK' if result.success else 'FAIL'}")

event_bus.subscribe("action.before_execute.create_sphere", on_before_execute)
event_bus.subscribe("action.after_execute.create_sphere", on_after_execute)
```
