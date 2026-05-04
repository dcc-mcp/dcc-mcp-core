# 自定义 Skill

学习如何为 DCC 应用程序构建自定义 Skill — 从推荐的 Skills-First 方式到低级注册表 API。

## 推荐：Skills-First 方式

Skills-First 方式通过环境变量发现 `SKILL.md` 包，是构建 DCC 工具的推荐方式：

- **零样板代码** — 无需手动注册处理器，工具自动发现
- **自动暴露为 MCP 工具** — Skill 管理器通过 MCP 协议将每个工具暴露给 AI
- **热重载** — `SKILL.md` 的修改无需重启即可生效

### 第一步：创建 SKILL.md 包

```markdown
---
name: maya-geometry
description: Maya 几何体创建工具
version: 1.0.0
dcc: maya
tags: [geometry, create]
tools:
  - name: create_sphere
    description: 创建多边形球体
    input_schema: |
      {
        "type": "object",
        "required": ["radius"],
        "properties": {
          "radius": {"type": "number", "minimum": 0.1},
          "name": {"type": "string"}
        }
      }
scripts:
  - create_sphere.py
---

# Maya 几何体工具

在 Maya 中创建和编辑几何体的工具集。
```

### 第二步：实现脚本

`create_sphere.py`：

```python
import maya.cmds as cmds
from dcc_mcp_core import success_result, error_result


def create_sphere(radius: float = 1.0, name: str | None = None):
    try:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        return success_result(
            message=f"已创建球体：{sphere}",
            object_name=sphere,
            radius=radius,
        )
    except Exception as e:
        return error_result("创建球体失败", str(e))
```

### 第三步：通过环境变量注册并启动

```python
import os
from dcc_mcp_core import create_skill_server

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my/skills"

# 一行代码：自动发现 Skill，启动 MCP HTTP 服务
manager = create_skill_server("maya")
```

::: tip Skills-First 是推荐模式
所有新 DCC 工具优先使用 `SKILL.md` 包。仅在需要运行时动态控制处理器逻辑时，才回退到注册表 API。
:::

---

## 低级注册表 API

当需要在运行时以编程方式控制处理器注册时，使用 `ToolRegistry` + `ToolDispatcher` API。

## 完整示例

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher

# 1. 使用 JSON Schema 注册 action 元数据
reg = ToolRegistry()
reg.register(
    name="create_sphere",
    description="在 Maya 中创建多边形球体",
    category="geometry",
    tags=["geo", "create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {
            "radius": {
                "type": "number",
                "minimum": 0.1,
                "description": "球体半径"
            },
            "segments": {
                "type": "integer",
                "minimum": 4,
                "default": 16,
                "description": "细分段数"
            },
            "name": {
                "type": "string",
                "description": "可选的球体名称"
            }
        }
    }),
)

# 2. 创建 dispatcher 并注册处理器
dispatcher = ToolDispatcher(reg)

def handle_create_sphere(params):
    radius = params.get("radius", 1.0)
    segments = params.get("segments", 16)
    name = params.get("name")

    # 调用 Maya API（使用 maya.cmds 的示例）
    import maya.cmds as cmds
    sphere_name = cmds.polySphere(r=radius, sx=segments, sy=segments, n=name)[0]

    return {
        "created": True,
        "object_name": sphere_name,
        "radius": radius,
        "segments": segments,
    }

dispatcher.register_handler("create_sphere", handle_create_sphere)

# 3. 使用 JSON 分派（wire format）
import json
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0, "segments": 32}))
print(result["output"]["object_name"])  # "pSphere1"
```

## 关键要点

1. **使用 `ToolRegistry.register()` 注册** —— 传入 name、description、tags、DCC、version 和 JSON Schema
2. **实现处理器函数** —— 接收 `params: dict`，返回结果字典
3. **使用 `ToolDispatcher` 注册处理器** —— 将 action 名称连接到 Python 可调用对象
4. **使用 JSON Schema 进行验证** —— `ToolDispatcher` 在调用处理器之前验证 JSON 输入
5. **使用 JSON 字符串分派** —— wire format 使用 JSON，而非 Python 字典

## 处理器函数签名

```python
def my_handler(params: dict) -> Any:
    """
    Args:
        params: 验证后的参数（已从 JSON 输入解析）
    Returns:
        一个字典，作为 action 结果（可序列化为 JSON）
    """
    pass
```

## 使用 ToolValidator 进行验证

在分派前验证输入：

```python
from dcc_mcp_core import ToolValidator

validator = ToolValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
ok, errors = validator.validate('{"radius": 1.5}')
if not ok:
    print(f"Validation failed: {errors}")
    # 处理错误
```

## 版本化 Action

使用 `VersionedRegistry` 保持向后兼容：

```python
from dcc_mcp_core import VersionedRegistry

vr = VersionedRegistry()

# v1: 基础球体
vr.register_versioned(
    "create_sphere", dcc="maya", version="1.0.0",
    description="Basic sphere creation",
)

# v2: 添加细分参数
vr.register_versioned(
    "create_sphere", dcc="maya", version="2.0.0",
    description="Sphere with subdivision control",
)

# 自动解析最佳版本
result = vr.resolve("create_sphere", "maya", "^1.0.0")
print(result["version"])  # "2.0.0"
```

## JSON Schema 技巧

- 使用 `$ref` 可重用 schema（ToolValidator 不支持 —— 请内联所有定义）
- `"default"` 字段在输入中缺少 key 时设置默认值
- 使用 `"minimum"`/`maximum` 进行数值约束
- 使用 `"minLength"`/`maxLength` 进行字符串长度约束
- 使用 `"enum"` 限制字符串选择

```python
input_schema = json.dumps({
    "type": "object",
    "required": ["radius"],
    "properties": {
        "radius": {
            "type": "number",
            "minimum": 0.1,
            "maximum": 1000.0,
        },
        "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
        },
        "align_to_world": {
            "type": "boolean",
            "default": False,
        }
    }
})
```
