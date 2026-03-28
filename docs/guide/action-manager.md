# Action Manager

The `ActionManager` is the central coordinator for the action lifecycle — discovering, loading, and executing actions.

## Creating an ActionManager

```python
from dcc_mcp_core import create_action_manager, get_action_manager

# Create new instance (always new)
manager = create_action_manager(
    dcc_name="maya",
    name="default",
    auto_refresh=True,
    refresh_interval=60,
    context={"cmds": maya.cmds},     # injected into all actions
    load_env_paths=True,              # load from DCC_MCP_ACTION_PATHS
    load_skill_paths=True,            # load from DCC_MCP_SKILL_PATHS
    extra_skill_paths=["/my/skills"],
)

# Get cached singleton (same key = same instance)
manager = get_action_manager("maya")

# Force new instance
manager = get_action_manager("maya", force_new=True)
```

## Discovery

```python
# Discover actions from a file
manager.discover_actions_from_path("/path/to/actions.py")

# Discover actions from a package
manager.discover_actions_from_package("my_actions_package")

# Refresh all actions
manager.refresh_actions(force=True)
```

## Execution

```python
# Synchronous execution
result = manager.call_action("create_sphere", radius=2.0, name="ball")

# Asynchronous execution
result = await manager.call_action_async("create_sphere", radius=2.0)

# Check result
if result.success:
    print(f"Success: {result.message}")
    print(f"Context: {result.context}")
    if result.prompt:
        print(f"Next step: {result.prompt}")
else:
    print(f"Error: {result.error}")
```

## Action Info

```python
# Get info about all registered actions
info = manager.get_actions_info()  # ActionResultModel with all action metadata

# List available action names
names = manager.list_available_actions()  # ["create_sphere", "delete_object", ...]
```

## Adding Middleware

```python
from dcc_mcp_core.actions.middleware import LoggingMiddleware, PerformanceMiddleware

manager.add_middleware(LoggingMiddleware)
manager.add_middleware(PerformanceMiddleware, threshold=0.5)
```

## ActionRegistry

The `ActionRegistry` is a singleton used internally by `ActionManager`. You can also access it directly:

```python
from dcc_mcp_core.actions.registry import ActionRegistry

registry = ActionRegistry()  # always returns same instance
registry.register(MyAction)
action_cls = registry.get_action("my_action", dcc_name="maya")
all_actions = registry.list_actions(dcc_name="maya", tag="geometry")
dcc_actions = registry.get_actions_by_dcc("maya")
all_dccs = registry.get_all_dccs()  # ["maya", "blender", ...]
```
