# Actions

Actions are the core building blocks of DCC-MCP-Core. Each action represents a discrete operation that can be performed in a DCC application.

## Action Base Class

Actions inherit from the `Action` base class, providing a standardized structure with Pydantic models:

```python
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.models import ActionResultModel
from pydantic import Field, field_validator, model_validator
from typing import List, Optional

class CreateSphereAction(Action):
    # Metadata as class attributes
    name = "create_sphere"
    description = "Creates a sphere in the scene"
    tags = ["geometry", "creation"]
    dcc = "maya"
    order = 0  # Execution priority

    # Input parameters model with validation
    class InputModel(Action.InputModel):
        radius: float = Field(default=1.0, description="Radius of the sphere")
        position: List[float] = Field(default=[0, 0, 0], description="Position")
        name: Optional[str] = Field(default=None, description="Name of the sphere")

        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("Radius must be positive")
            return v

        @model_validator(mode='after')
        def validate_model(self):
            if self.name and self.position == [0, 0, 0]:
                raise ValueError("Position must not be origin when name is specified")
            return self

    # Output data model
    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="Name of the created object")
        position: List[float] = Field(description="Final position")

    def _execute(self) -> None:
        radius = self.input.radius
        position = self.input.position
        name = self.input.name or f"sphere_{radius}"

        cmds = self.context.get("cmds")
        sphere = cmds.polySphere(r=radius, n=name)[0]
        cmds.move(*position, sphere)

        self.output = self.OutputModel(
            object_name=sphere,
            position=position,
            prompt="You can now modify the sphere's attributes or add materials"
        )
```

## Class Attributes

| Attribute | Type | Description |
|-----------|------|-------------|
| `name` | `str` | Action name (used for registration and lookup) |
| `description` | `str` | What this action does |
| `tags` | `List[str]` | Classification tags |
| `dcc` | `str` | Target DCC (`"maya"`, `"blender"`, `"houdini"`, etc.) |
| `order` | `int` | Execution priority (lower = first) |
| `category` | `str` | Organization category |
| `abstract` | `bool` | If `True`, not registered |

## Lifecycle

```
__init__(context) → setup(**kwargs) → process() → ActionResultModel
```

1. **`__init__(context)`** — Initialize with DCC context
2. **`setup(**kwargs)`** — Validate input, chainable
3. **`process()`** — Sync execute with error handling
4. **`process_async()`** — Async execute (thread pool by default)

## Key Methods

| Method | Description |
|--------|-------------|
| `setup(**kwargs)` | Validate input, returns self (chainable) |
| `validate_input(**kwargs)` | Pydantic validation |
| `process()` | Sync execute → `ActionResultModel` |
| `process_async()` | Async execute → `ActionResultModel` |
| `_execute()` | **Must implement** — core logic |
| `_execute_async()` | Override for native async |

## Async Support

By default, `_execute_async()` runs `_execute()` in a thread pool. Override for native async:

```python
async def _execute_async(self) -> None:
    import asyncio
    await asyncio.sleep(0.1)  # Simulate async work
    # ... async operations
```
