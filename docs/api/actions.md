# Actions API

## Action Base Class

`dcc_mcp_core.actions.base.Action`

### Class Attributes

| Attribute | Type | Description |
|-----------|------|-------------|
| `name` | `str` | Action name |
| `description` | `str` | Description |
| `tags` | `List[str]` | Tags |
| `dcc` | `str` | Target DCC |
| `order` | `int` | Priority (lower = first) |
| `category` | `str` | Category |
| `abstract` | `bool` | If `True`, not registered |

### Inner Classes

- `InputModel(Action.InputModel)` — Define input parameters
- `OutputModel(Action.OutputModel)` — Define structured output

### Methods

- `setup(**kwargs) -> Action` — Validate input, chainable
- `validate_input(**kwargs) -> InputModel`
- `process() -> ActionResultModel` — Sync execute
- `process_async() -> ActionResultModel` — Async execute
- `_execute() -> None` — **Must implement**
- `_execute_async() -> None` — Override for native async

## ActionManager

`dcc_mcp_core.actions.manager.ActionManager`

### Factory Functions

```python
create_action_manager(dcc_name, ...) -> ActionManager  # always new
get_action_manager(dcc_name, ...) -> ActionManager      # cached singleton
```

### Methods

- `discover_actions_from_path(path)`
- `discover_actions_from_package(package_name)`
- `refresh_actions(force=True)`
- `call_action(name, **kwargs) -> ActionResultModel`
- `call_action_async(name, **kwargs) -> ActionResultModel`
- `get_actions_info() -> ActionResultModel`
- `list_available_actions() -> List[str]`
- `add_middleware(cls, **kwargs)`

## ActionRegistry

`dcc_mcp_core.actions.registry.ActionRegistry` (singleton)

### Methods

- `register(action_class) -> bool`
- `get_action(name, dcc_name=None) -> Optional[Type[Action]]`
- `list_actions(dcc_name=None, tag=None) -> List[Dict]`
- `get_actions_by_dcc(dcc_name) -> Dict[str, Type[Action]]`
- `get_all_dccs() -> List[str]`

## Function Adapters

`dcc_mcp_core.actions.function_adapter`

- `create_function_adapter(action_name, dcc_name=None) -> Callable`
- `create_function_adapters(dcc_name=None, manager=None) -> Dict[str, Callable]`
