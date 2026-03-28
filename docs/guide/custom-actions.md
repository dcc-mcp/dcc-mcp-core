# Custom Actions

Learn how to create your own actions for DCC applications.

## Complete Example

```python
from dcc_mcp_core.actions.base import Action
from pydantic import Field, field_validator

class CreateSphereAction(Action):
    # Action metadata
    name = "create_sphere"
    description = "Creates a sphere in Maya"
    tags = ["geometry", "creation"]
    dcc = "maya"
    order = 0

    # Input parameters model with validation
    class InputModel(Action.InputModel):
        radius: float = Field(1.0, description="Radius of the sphere")
        position: list[float] = Field([0, 0, 0], description="Position")
        name: str = Field(None, description="Name of the sphere")

        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("Radius must be positive")
            return v

    # Output data model
    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="Name of the created object")
        position: list[float] = Field(description="Final position")

    def _execute(self) -> None:
        radius = self.input.radius
        position = self.input.position
        name = self.input.name or f"sphere_{radius}"

        cmds = self.context.get("cmds")

        try:
            sphere = cmds.polySphere(r=radius, n=name)[0]
            cmds.move(*position, sphere)

            self.output = self.OutputModel(
                object_name=sphere,
                position=position,
                prompt="You can now modify the sphere's attributes or add materials"
            )
        except Exception as e:
            raise Exception(f"Failed to create sphere: {str(e)}") from e
```

## Key Points

1. **Always define `InputModel`** with Pydantic `Field` for each parameter
2. **Always define `OutputModel`** for structured output
3. **Implement `_execute()`** — access `self.input`, `self.context`, set `self.output`
4. **Use validators** for parameter validation (`@field_validator`, `@model_validator`)
5. **Let exceptions propagate** — the framework converts them to `ActionResultModel`

## Registering Actions

Actions are automatically discovered when placed in registered action paths:

```python
from dcc_mcp_core.actions.manager import ActionManager

manager = ActionManager("maya", load_env_paths=False)
manager.register_action_path("/path/to/my_actions/")
manager.refresh_actions()
```

Or register manually via the registry:

```python
from dcc_mcp_core.actions.registry import ActionRegistry

registry = ActionRegistry()
registry.register(CreateSphereAction)
```
