# 自定义 Action

学习如何为 DCC 应用创建自定义 Action。

## 完整示例

```python
from dcc_mcp_core.actions.base import Action
from pydantic import Field, field_validator

class CreateSphereAction(Action):
    # Action 元数据
    name = "create_sphere"
    description = "在 Maya 中创建球体"
    tags = ["几何体", "创建"]
    dcc = "maya"
    order = 0

    # 带验证的输入参数模型
    class InputModel(Action.InputModel):
        radius: float = Field(1.0, description="球体半径")
        position: list[float] = Field([0, 0, 0], description="位置")
        name: str = Field(None, description="球体名称")

        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("半径必须为正数")
            return v

    # 输出数据模型
    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="创建的对象名称")
        position: list[float] = Field(description="最终位置")

    def _execute(self) -> None:
        radius = self.input.radius
        cmds = self.context.get("cmds")

        try:
            sphere = cmds.polySphere(r=radius, n=self.input.name)[0]
            cmds.move(*self.input.position, sphere)

            self.output = self.OutputModel(
                object_name=sphere,
                position=self.input.position,
                prompt="现在可以修改球体属性或添加材质"
            )
        except Exception as e:
            raise Exception(f"创建球体失败: {str(e)}") from e
```

## 关键要点

1. **始终定义 `InputModel`** — 使用 Pydantic `Field` 为每个参数提供描述
2. **始终定义 `OutputModel`** — 提供结构化输出
3. **实现 `_execute()`** — 通过 `self.input`、`self.context` 访问数据，设置 `self.output`
4. **使用验证器** — `@field_validator`、`@model_validator` 进行参数验证
5. **让异常自然传播** — 框架会自动将其转换为 `ActionResultModel`

## 注册 Action

Action 放置在已注册的路径中时会被自动发现：

```python
from dcc_mcp_core.actions.manager import ActionManager

manager = ActionManager("maya", load_env_paths=False)
manager.register_action_path("/path/to/my_actions/")
manager.refresh_actions()
```

也可以通过注册表手动注册：

```python
from dcc_mcp_core.actions.registry import ActionRegistry

registry = ActionRegistry()
registry.register(CreateSphereAction)
```
