# Function Adapters

Convert Action classes to plain callable functions, useful for RPyC/API exposure.

## Single Action

```python
from dcc_mcp_core.actions import create_function_adapter

create_sphere = create_function_adapter("create_sphere", dcc_name="maya")
result = create_sphere(radius=2.0)
```

## All Actions for a DCC

```python
from dcc_mcp_core.actions import create_function_adapters

funcs = create_function_adapters(dcc_name="maya", manager=manager)
result = funcs["create_sphere"](radius=2.0)
```

## Use Case: RPyC Exposure

Function adapters are particularly useful when exposing actions through RPyC for remote DCC operations:

```python
from dcc_mcp_core.actions import create_function_adapters

# Create function adapters for all Maya actions
maya_functions = create_function_adapters(dcc_name="maya")

# These plain functions can be exposed via RPyC service
class MayaService(rpyc.Service):
    def exposed_call_action(self, name, **kwargs):
        if name in maya_functions:
            return maya_functions[name](**kwargs)
```
