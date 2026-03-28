# 自定义 Action

## 完整示例

```python
from dcc_mcp_core.actions.base import Action
from pydantic import Field, field_validator

class CreateSphereAction(Action):
    name = "create_sphere"
    description = "在 Maya 中创建球体"
    tags = ["几何体", "创建"]
    dcc = "maya"

    class InputModel(Action.InputModel):
        radius: float = Field(1.0, description="球体半径")
        position: list[float] = Field([0, 0, 0], description="位置")
        name: str = Field(None, description="球体名称")

        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("半径必须为正数")
            return v

    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="创建的对象名称")
        position: list[float] = Field(description="最终位置")

    def _execute(self) -> None:
        radius = self.input.radius
        cmds = self.context.get("cmds")
        sphere = cmds.polySphere(r=radius, n=self.input.name)[0]

        self.output = self.OutputModel(
            object_name=sphere,
            position=self.input.position,
            prompt="现在可以修改球体属性"
        )
```

## 注册 Action

```python
from dcc_mcp_core.actions.manager import ActionManager

manager = ActionManager("maya", load_env_paths=False)
manager.register_action_path("/path/to/my_actions/")
manager.refresh_actions()
```
