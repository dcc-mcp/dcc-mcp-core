# Actions API

`dcc_mcp_core` — ActionRegistry、EventBus、ActionDispatcher、ActionValidator、VersionedRegistry。

## ActionRegistry

线程安全的 Action 注册表，底层使用 DashMap 实现。每个注册表实例相互独立。

### 构造函数

```python
from dcc_mcp_core import ActionRegistry
registry = ActionRegistry()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register(name, description="", category="", tags=[], dcc="python", version="1.0.0", input_schema=None, output_schema=None, source_file=None)` | — | 注册一个 Action |
| `get_action(name, dcc_name=None)` | `dict?` | 获取 Action 元数据字典 |
| `list_actions(dcc_name=None)` | `List[dict]` | 列出所有 Action 的元数据字典 |
| `list_actions_for_dcc(dcc_name)` | `List[str]` | 列出指定 DCC 的 Action 名称 |
| `get_all_dccs()` | `List[str]` | 列出所有已注册的 DCC 名称 |
| `reset()` | — | 清除所有已注册的 Action |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__len__` | 已注册 Action 的数量 |
| `__contains__(name)` | 检查 Action 是否已注册 |
| `__repr__` | `ActionRegistry(actions=N)` |

### Action 元数据字典

通过 `get_action()` 或 `list_actions()` 获取时，每个 Action 是一个字典：

```python
{
    "name": "create_sphere",
    "description": "Creates a sphere",
    "category": "geometry",
    "tags": ["geometry"],
    "dcc": "maya",
    "version": "1.0.0",
    "input_schema": {"type": "object", "properties": {}},
    "output_schema": {"type": "object", "properties": {}},
    "source_file": "/path/to/source.py"  # 或 null
}
```

## ActionValidator

基于 JSON Schema 的 Action 输入验证器。

### 构造函数

```python
from dcc_mcp_core import ActionValidator
validator = ActionValidator()
```

### 注册模式

```python
# 为 Action 注册 JSON Schema
validator.register_schema(
    "create_sphere",
    {
        "type": "object",
        "properties": {
            "radius": {"type": "number", "minimum": 0},
            "name": {"type": "string"}
        },
        "required": ["radius"]
    }
)
```

### 验证输入

```python
from dcc_mcp_core import ValidationResult

# 有效输入
result = validator.validate("create_sphere", {"radius": 1.0, "name": "sphere1"})
print(result.valid)       # True
print(result.action_name) # "create_sphere"

# 无效输入
result = validator.validate("create_sphere", {"radius": -1.0})
print(result.valid)       # False
print(result.errors)      # ["radius must be >= 0"]
```

### ValidationResult

| 字段 | 类型 | 说明 |
|------|------|------|
| `valid` | `bool` | 验证是否通过 |
| `action_name` | `str` | 被验证的 Action |
| `errors` | `List[str]` | 验证错误列表 |
| `validated_input` | `dict` | 清理后的输入字典 |

## ActionDispatcher

基于版本兼容性的 Action 路由到处理器函数。

### 构造函数

```python
from dcc_mcp_core import ActionDispatcher

dispatcher = ActionDispatcher()
```

### 注册处理器

```python
def handle_create_sphere(ctx, input):
    # 处理 Action
    return {"success": True, "sphere": input.get("name", "sphere")}

dispatcher.register_handler("create_sphere", handle_create_sphere)
```

### 分发 Action

```python
result = dispatcher.dispatch("create_sphere", context={}, input={"radius": 1.0})
print(result)  # {"success": True, "sphere": "sphere"}
```

## SemVer

语义版本控制工具。

### 解析版本

```python
from dcc_mcp_core import SemVer

v = SemVer.parse("1.2.3")
print(v.major)  # 1
print(v.minor)  # 2
print(v.patch)  # 3
print(v.prerelease)  # None 或 "alpha", "beta", "rc.1"
print(v.build_metadata)  # None 或 "build.123"
```

### 版本比较

```python
v1 = SemVer.parse("1.2.3")
v2 = SemVer.parse("1.2.4")
v3 = SemVer.parse("2.0.0")

print(v1 < v2)  # True
print(v2 > v1)  # True
print(v3 > v1)  # True
print(v1 == v1)  # True
```

### 版本排序

```python
versions = [
    SemVer.parse("2.0.0"),
    SemVer.parse("1.0.0"),
    SemVer.parse("1.2.3"),
]
sorted_versions = sorted(versions)
print([str(v) for v in sorted_versions])  # ["1.0.0", "1.2.3", "2.0.0"]
```

## VersionConstraint

版本需求规范。

### 创建约束

```python
from dcc_mcp_core import VersionConstraint

# 各种约束类型
constraint1 = VersionConstraint.parse(">=1.0.0,<2.0.0")
constraint2 = VersionConstraint.parse("^1.2.3")  # 兼容 1.x.x
constraint3 = VersionConstraint.parse("~1.2.0")  # 大致相当于 1.2.x
constraint4 = VersionConstraint.parse("1.2.3")   # 精确版本
```

### 检查约束

```python
v = SemVer.parse("1.5.0")
constraint = VersionConstraint.parse(">=1.0.0,<2.0.0")

print(constraint.matches(v))  # True
```

## VersionedRegistry

支持语义版本控制的注册表，用于向后兼容。

### 构造函数

```python
from dcc_mcp_core import VersionedRegistry
registry = VersionedRegistry()
```

### 注册版本化 Action

```python
# 注册同一 Action 的多个版本
registry.register(
    name="create_sphere",
    version="1.0.0",
    handler=handle_v1,
    input_schema={"type": "object", "properties": {"radius": {"type": "number"}}}
)

registry.register(
    name="create_sphere",
    version="2.0.0",
    handler=handle_v2,
    input_schema={"type": "object", "properties": {"radius": {"type": "number"}, "segments": {"type": "integer"}}}
)
```

### 查找 Action

```python
# 获取最新版本
action = registry.get_latest("create_sphere")

# 获取特定版本
action = registry.get_version("create_sphere", "1.0.0")

# 查找兼容版本
action = registry.find_compatible("create_sphere", ">=1.0.0,<2.0.0")
```

## CompatibilityRouter

基于版本约束路由 Action 到处理器的路由器。

### 构造函数

```python
from dcc_mcp_core import CompatibilityRouter

router = CompatibilityRouter()
```

### 注册路由

```python
router.add_route(
    action="create_sphere",
    constraint=">=1.0.0,<2.0.0",
    handler=handle_v1
)
router.add_route(
    action="create_sphere",
    constraint=">=2.0.0",
    handler=handle_v2
)
```

### 路由请求

```python
# 基于客户端版本头路由
result = router.route(
    action="create_sphere",
    client_version="1.5.0",
    context={},
    input={}
)

# 基于显式约束路由
result = router.route(
    action="create_sphere",
    constraint=">=1.0.0,<2.0.0",
    context={},
    input={}
)
```

## DispatchResult

分发操作的返回类型。

```python
result = dispatcher.dispatch("create_sphere", context, input)

print(result.success)      # True
print(result.action_name)   # "create_sphere"
print(result.version)       # "1.0.0"
print(result.output)        # 处理器输出
print(result.duration_ms)   # 执行时间
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `success` | `bool` | 分发是否成功 |
| `action_name` | `str` | 被分发的 Action |
| `version` | `str?` | 使用的处理器版本 |
| `output` | `dict` | 处理器输出 |
| `error` | `str?` | 失败时的错误消息 |
| `duration_ms` | `int` | 执行时间 |
