# Actions 动作

Actions 是 DCC-MCP-Core 的核心构建块。每个 Action 代表一个可以在 DCC 应用程序（Maya、Blender、Houdini 等）中执行的离散操作。

## 架构

DCC-MCP-Core 采用基于注册表的 action 模型，后端使用 Rust 的 DashMap 实现线程安全并发访问：

- **`ActionRegistry`** — 线程安全的 action 元数据存储（name、description、tags、DCC、version、JSON schemas）
- **`ActionDispatcher`** — 将验证后的调用路由到注册的 Python 处理器
- **`ActionValidator`** — 基于 JSON Schema 的输入验证
- **`VersionedRegistry`** — 支持语义版本解析的多版本 action 管理

所有 action 在运行时被发现和注册。**没有基类或 Pydantic 模型** —— action 是通过元数据注册的普通 Python 函数。

## ActionRegistry

`ActionRegistry` 是所有 DCC 操作的中央注册表。使用 JSON Schema 注册 action 进行输入验证：

```python
import json
from dcc_mcp_core import ActionRegistry

reg = ActionRegistry()

reg.register(
    name="create_sphere",
    description="在 DCC 场景中创建多边形球体",
    category="geometry",
    tags=["geo", "create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {
            "radius": {"type": "number", "minimum": 0.1, "description": "球体半径"},
            "segments": {"type": "integer", "minimum": 4, "default": 16},
            "name": {"type": "string", "description": "可选的球体名称"}
        }
    }),
)
```

### 发现和查询

```python
# 获取所有注册了 action 的 DCC
dccs = reg.get_all_dccs()
print(dccs)  # ["maya", "blender", "houdini"]

# 列出 Maya 的所有 action
maya_actions = reg.list_actions_for_dcc("maya")
print(maya_actions)  # ["create_sphere", "create_cube", ...]

# 获取完整元数据
meta = reg.get_action("create_sphere", dcc_name="maya")
print(meta["version"])  # "1.0.0"

# 按类别和标签搜索
results = reg.search_actions(category="geometry", tags=["create"])
for r in results:
    print(r["name"], r["dcc"])

# 所有类别和标签
categories = reg.get_categories()
tags = reg.get_tags(dcc_name="maya")
```

### Dunder 访问

```python
reg.register("echo", dcc="python")
print("echo" in reg)  # True
print(len(reg))        # 已注册 action 的数量
```

## ActionDispatcher

`ActionDispatcher` 与 `ActionRegistry` 配对，提供验证后的 action 执行路由：

```python
import json
from dcc_mcp_core import ActionRegistry, ActionDispatcher

reg = ActionRegistry()
reg.register(
    "create_sphere",
    dcc="maya",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {"radius": {"type": "number"}}
    }),
)
dispatcher = ActionDispatcher(reg)

def handle_create_sphere(params):
    radius = params["radius"]
    # 在此调用 Maya API（例如通过 pymel 或 maya.cmds）
    return {"created": True, "radius": radius}

dispatcher.register_handler("create_sphere", handle_create_sphere)

# 使用 JSON 字符串分派（wire format）
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result = {"action": "create_sphere", "output": {"created": True, "radius": 2.0}, "validation_skipped": False}
```

## ActionValidator

独立的验证器，用于根据 schema 检查 JSON 参数：

```python
from dcc_mcp_core import ActionValidator

validator = ActionValidator.from_schema_json(
    '{"type": "object", "required": ["radius"], '
    '"properties": {"radius": {"type": "number", "minimum": 0}}}'
)

ok, errors = validator.validate('{"radius": 1.5}')
print(ok, errors)  # True, []

ok, errors = validator.validate('{"radius": -1}')
print(ok, errors)  # False, ["radius must be >= 0"]
```

或从已存在的 `ActionRegistry` action 创建：

```python
from dcc_mcp_core import ActionRegistry, ActionValidator

reg = ActionRegistry()
reg.register("create_sphere", dcc="maya", input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}')

validator = ActionValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
```

## 结果模型

所有 action 结果都规范化为 `ActionResultModel`：

```python
from dcc_mcp_core import success_result, error_result, from_exception

# 成功
result = success_result(
    message="Sphere created",
    prompt="Consider adding materials",  # AI 指导
    object_name="sphere1",
    position=[0, 0, 0],
)
print(result.success)    # True
print(result.prompt)    # "Consider adding materials"
print(result.context)   # {"object_name": "sphere1", "position": [0, 0, 0]}

# 错误
result = error_result(
    message="Failed to create sphere",
    error="Maya API error: object already exists",
    object_name="sphere1",
)
print(result.success)  # False
print(result.error)    # "Maya API error: object already exists"

# 从异常创建
try:
    raise RuntimeError("connection refused")
except Exception:
    result = from_exception("Connection to Maya lost")
    print(result.success)  # False
```

## VersionedRegistry

对于需要在多个版本间保持向后兼容的 API：

```python
from dcc_mcp_core import VersionedRegistry, VersionConstraint

vr = VersionedRegistry()

# 注册同一 action 的多个版本
vr.register_versioned("create_sphere", dcc="maya", version="1.0.0",
    description="Basic sphere creation", category="geometry", tags=["geo"])
vr.register_versioned("create_sphere", dcc="maya", version="2.0.0",
    description="Sphere with UV support", category="geometry", tags=["geo", "uv"])

# 解析最佳版本
result = vr.resolve("create_sphere", "maya", "^1.0.0")
print(result["version"])   # "2.0.0"

# 所有匹配的版本
all_v = vr.resolve_all("create_sphere", "maya", ">=1.0.0")
print([v["version"] for v in all_v])  # ["1.0.0", "2.0.0"]

# 最新版本
print(vr.latest_version("create_sphere", "maya"))  # "2.0.0"
```

## EventBus

订阅 action 生命周期事件，用于监控、日志或链式调用：

```python
from dcc_mcp_core import EventBus

bus = EventBus()

def on_before_execute(event, **kwargs):
    print(f"Executing {event} with {kwargs}")

def on_after_execute(event, **kwargs):
    print(f"Completed {event}")

# 订阅所有 "before_execute" 事件（通配符）
id1 = bus.subscribe("action.before_execute.*", on_before_execute)

# 订阅特定 action
id2 = bus.subscribe("action.after_execute.create_sphere", on_after_execute)

# 取消订阅
bus.unsubscribe("action.before_execute.*", id1)

# 手动发布
bus.publish("custom.event", custom_data="value")
```
