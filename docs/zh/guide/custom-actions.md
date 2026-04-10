# 自定义 Skill

学习如何为 DCC 应用程序构建自定义 Skill — 从推荐的 Skills-First 方式到低级注册表 API。

## 完整示例

```python
import json
from dcc_mcp_core import ActionRegistry, ActionDispatcher

# 1. 使用 JSON Schema 注册 action 元数据
reg = ActionRegistry()
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
dispatcher = ActionDispatcher(reg)

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

1. **使用 `ActionRegistry.register()` 注册** —— 传入 name、description、tags、DCC、version 和 JSON Schema
2. **实现处理器函数** —— 接收 `params: dict`，返回结果字典
3. **使用 `ActionDispatcher` 注册处理器** —— 将 action 名称连接到 Python 可调用对象
4. **使用 JSON Schema 进行验证** —— `ActionDispatcher` 在调用处理器之前验证 JSON 输入
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

## 使用 ActionValidator 进行验证

在分派前验证输入：

```python
from dcc_mcp_core import ActionValidator

validator = ActionValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
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

- 使用 `$ref` 可重用 schema（ActionValidator 不支持 —— 请内联所有定义）
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
