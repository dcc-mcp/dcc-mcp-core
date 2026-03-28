# Utilities API

`dcc_mcp_core.utils`

## Decorators

```python
from dcc_mcp_core.utils.decorators import error_handler, with_context

@error_handler
def risky_operation(data):
    # Always returns ActionResultModel, catches all exceptions
    return {"processed": True}

@with_context()
def process(data, context):
    # context defaults to {} if not provided
    pass
```

## Type Wrappers (RPyC)

```python
from dcc_mcp_core.utils.type_wrappers import (
    wrap_value, unwrap_value, wrap_boolean_parameters, unwrap_parameters,
    BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper,
)

wrapped = wrap_value(True)         # BooleanWrapper(True)
original = unwrap_value(wrapped)   # True

params = {"visible": True, "count": 5}
wrapped_params = wrap_boolean_parameters(params)
original_params = unwrap_parameters(wrapped_params)
```

## Module Loading

```python
from dcc_mcp_core.utils.module_loader import load_module_from_path, append_to_python_path

module = load_module_from_path(
    "/path/to/action.py",
    dependencies={"cmds": maya_cmds},
    dcc_name="maya"
)

with append_to_python_path("/path/to/script.py"):
    import my_script
```

## Filesystem

```python
from dcc_mcp_core.utils.filesystem import (
    get_config_dir, get_data_dir, get_log_dir, get_actions_dir,
    get_skills_dir, get_skill_paths_from_env, get_actions_paths_from_env,
)

config = get_config_dir()           # Platform-specific config directory
actions = get_actions_dir("maya")   # .../data/actions/maya/
skills = get_skills_dir("maya")     # .../data/skills/maya/
env_paths = get_skill_paths_from_env()  # from DCC_MCP_SKILL_PATHS
```

## Dependency Injection

```python
from dcc_mcp_core.utils.dependency_injector import inject_dependencies

inject_dependencies(
    module,
    {"cmds": maya_cmds},
    inject_core_modules=True,
    dcc_name="maya"
)
# module.cmds == maya_cmds
# module.DCC_NAME == "maya"
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_ACTION_PATHS` | Action search paths |
| `DCC_MCP_ACTION_PATH_{DCC}` | DCC-specific action paths (e.g., `DCC_MCP_ACTION_PATH_MAYA`) |
| `DCC_MCP_SKILL_PATHS` | Skill search paths |
| `DCC_MCP_ACTIONS_DIR` | Generic actions directory |

## Exceptions

`dcc_mcp_core.utils.exceptions`

| Exception | Description |
|-----------|-------------|
| `MCPError` | Base exception |
| `ConfigurationError` | Configuration issues |
| `ConnectionError` | Connection issues |
| `OperationError` | Operation failures |
| `ValidationError` | General validation errors |
| `ParameterValidationError` | Parameter validation errors |
| `VersionError` | Version compatibility errors |
| `ActionError` | Base action error (has `action_name`, `action_class`) |
| `ActionParameterError` | Parameter validation (has `parameter_name`) |
| `ActionExecutionError` | Execution failure (has `execution_phase`, `traceback`) |
