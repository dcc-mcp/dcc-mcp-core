# Actions API

`dcc_mcp_core.ActionRegistry`

## ActionRegistry

Thread-safe action registry backed by DashMap. Each registry instance is independent.

### Constructor

```python
from dcc_mcp_core import ActionRegistry
registry = ActionRegistry()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `register(name, description="", category="", tags=[], dcc="python", version="1.0.0", input_schema=None, output_schema=None, source_file=None)` | — | Register an action |
| `get_action(name, dcc_name=None)` | `dict?` | Get action metadata as dict |
| `list_actions(dcc_name=None)` | `List[dict]` | List all actions as metadata dicts |
| `list_actions_for_dcc(dcc_name)` | `List[str]` | List action names for a DCC |
| `get_all_dccs()` | `List[str]` | List all registered DCC names |
| `reset()` | — | Clear all registered actions |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__len__` | Number of registered actions |
| `__contains__(name)` | Check if action is registered |
| `__repr__` | `ActionRegistry(actions=N)` |

### Action Metadata Dict

When retrieved via `get_action()` or `list_actions()`, each action is a dict:

```python
{
    "name": "create_sphere",
    "description": "Creates a sphere",
    "category": "geometry",
    "tags": ["geometry"],
    "dcc": "maya",
    "version": "1.0.0",
    "input_schema": {"type": "object", "properties": {}},
    "output_schema": {"type": "object", "properties": {}},
    "source_file": "/path/to/source.py"  # or null
}
```
