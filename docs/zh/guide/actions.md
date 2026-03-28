# Actions 动作

Actions 是 DCC-MCP-Core 的核心构建块。每个 Action 代表一个可以在 DCC 应用程序中执行的离散操作。

## Action 基类

Actions 继承自 `Action` 基类，使用 Pydantic 模型提供标准化结构：

```python
from dcc_mcp_core.actions.base import Action
from pydantic import Field, field_validator, model_validator
from typing import List, Optional

class CreateSphereAction(Action):
    # 元数据
    name = "create_sphere"
    description = "在场景中创建球体"
    tags = ["几何体", "创建"]
    dcc = "maya"
    order = 0

    # 带验证的输入参数模型
    class InputModel(Action.InputModel):
        radius: float = Field(default=1.0, description="球体半径")
        position: List[float] = Field(default=[0, 0, 0], description="位置")
        name: Optional[str] = Field(default=None, description="球体名称")

        @field_validator('radius')
        def validate_radius(cls, v):
            if v <= 0:
                raise ValueError("半径必须为正数")
            return v

    # 输出数据模型
    class OutputModel(Action.OutputModel):
        object_name: str = Field(description="创建的对象名称")
        position: List[float] = Field(description="最终位置")

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
            prompt="现在可以修改球体属性或添加材质"
        )
```

## 类属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 动作名称（用于注册和查找） |
| `description` | `str` | 动作描述 |
| `tags` | `List[str]` | 分类标签 |
| `dcc` | `str` | 目标 DCC |
| `order` | `int` | 执行优先级（越小越先） |
| `category` | `str` | 组织分类 |
| `abstract` | `bool` | 为 `True` 时不注册 |

## 生命周期

```
__init__(context) → setup(**kwargs) → process() → ActionResultModel
```

## 关键方法

| 方法 | 说明 |
|------|------|
| `setup(**kwargs)` | 验证输入，支持链式调用 |
| `process()` | 同步执行 → `ActionResultModel` |
| `process_async()` | 异步执行 → `ActionResultModel` |
| `_execute()` | **必须实现** — 核心逻辑 |
| `_execute_async()` | 重写以支持原生异步 |
