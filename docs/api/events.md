# Events API

`dcc_mcp_core.actions.events`

## EventBus

Global instance: `event_bus`

### Methods

- `subscribe(event_name: str, handler: Callable)` — Subscribe to an event
- `unsubscribe(event_name: str, handler: Callable)` — Unsubscribe
- `publish(event_name: str, data: dict)` — Publish an event

## Built-in Events

| Event | Published By | Data |
|-------|-------------|------|
| `action_manager.created` | ActionManager init | `{"manager": ...}` |
| `action_manager.before_discover_path` | `discover_actions_from_path` | `{"path": ...}` |
| `action_manager.after_discover_path` | `discover_actions_from_path` | `{"path": ..., "actions": ...}` |
| `action_manager.before_refresh` | `refresh_actions` | `{"force": ...}` |
| `action_manager.after_refresh` | `refresh_actions` | `{"actions_count": ...}` |
| `action.before_execute.{name}` | `call_action` | `{"action_name": ..., "kwargs": ...}` |
| `action.after_execute.{name}` | `call_action` | `{"action_name": ..., "result": ...}` |
| `action.error.{name}` | `call_action` | `{"action_name": ..., "error": ...}` |
| `skill.loaded` | skill loader | `{"skill_name": ..., "actions": ...}` |
