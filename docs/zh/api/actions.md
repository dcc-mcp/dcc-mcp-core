# Actions API

`dcc_mcp_core` — ActionRegistry、EventBus、ActionDispatcher、ActionValidator、SemVer、VersionConstraint、VersionedRegistry。

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
| `search_actions(category=None, tags=[], dcc_name=None)` | `List[dict]` | AND 组合过滤搜索 |
| `get_categories(dcc_name=None)` | `List[str]` | 排序后的唯一类别列表 |
| `get_tags(dcc_name=None)` | `List[str]` | 排序后的唯一标签列表 |
| `count_actions(category=None, tags=[], dcc_name=None)` | `int` | 符合条件的 Action 数量 |
| `reset()` | — | 清除所有已注册的 Action |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__len__` | 已注册 Action 的数量 |
| `__contains__(name)` | 检查 Action 名称是否已注册（作用域为 "python" dcc） |
| `__repr__` | `ActionRegistry(actions=N)` |

### Action 元数据字典

通过 `get_action()`、`list_actions()` 或 `search_actions()` 获取时，每个 Action 是一个字典：

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

### 示例

```python
reg = ActionRegistry()
reg.register(
    "create_sphere",
    description="Create a polygon sphere",
    category="geometry",
    tags=["geo", "create"],
    dcc="maya",
    input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}',
)

# 获取
meta = reg.get_action("create_sphere", dcc_name="maya")
print(meta["version"])  # "1.0.0"

# 搜索
results = reg.search_actions(category="geometry", tags=["create"])
```

## ActionValidator

基于 JSON Schema 的 Action 输入验证器。通过 schema 字符串或 `ActionRegistry` 中的 action 创建。

### 静态工厂方法

```python
from dcc_mcp_core import ActionValidator

# 从 JSON Schema 字符串创建
validator = ActionValidator.from_schema_json(
    '{"type": "object", "required": ["radius"], '
    '"properties": {"radius": {"type": "number", "minimum": 0.0}}}'
)

# 从 ActionRegistry 中的 action 创建
from dcc_mcp_core import ActionRegistry
reg = ActionRegistry()
reg.register("create_sphere", input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}')
validator = ActionValidator.from_action_registry(reg, "create_sphere")
```

### 验证输入

```python
# 有效输入 — 返回 (True, [])
ok, errors = validator.validate('{"radius": 1.0}')
print(ok)      # True
print(errors)  # []

# 无效输入 — 返回 (False, [error1, ...])
ok, errors = validator.validate('{"radius": -1.0}')
print(ok)      # False
print(errors)  # ["radius must be >= 0"]

# 缺少必填字段
ok, errors = validator.validate("{}")
print(ok)      # False
print(errors)  # ["radius is required"]
```

### 错误处理

```python
try:
    validator.validate('not json at all')
except ValueError as e:
    print(f"Invalid JSON: {e}")
```

::: tip
`validate()` 接收的是 **JSON 字符串**（`'{"radius": 1.0}'`），而非 Python 字典。这与 MCP 协议的线格式（wire-format）一致。
:::

## ActionDispatcher

基于版本兼容性的 Action 路由到处理器函数。

### 构造函数

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher

reg = ActionRegistry()
dispatcher = ActionDispatcher(reg)
```

### 注册处理器

```python
def handle_create_sphere(params):
    # params 是从 JSON 输入反序列化的 Python 字典
    return {"created": True, "radius": params.get("radius", 1.0)}

dispatcher.register_handler("create_sphere", handle_create_sphere)
```

### 分发 Action

```python
import json

result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result = {"action": "create_sphere", "output": {"created": True, "radius": 2.0}, "validation_skipped": False}
print(result["output"]["created"])  # True
```

### 处理器函数签名

```python
def handler(params: dict) -> Any:
    """接收验证后的 JSON params 作为 Python 字典。"""
    pass
```

### 其他方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register_handler(action_name, handler)` | — | 注册一个 Python 可调用对象 |
| `remove_handler(action_name)` | `bool` | 移除处理器，存在返回 True |
| `has_handler(action_name)` | `bool` | 检查处理器是否已注册 |
| `handler_count()` | `int` | 已注册处理器数量 |
| `handler_names()` | `List[str]` | 按字母排序的处理器名称 |
| `skip_empty_schema_validation` | `bool` | 属性：schema 为 `{}` 时跳过验证 |

## SemVer

语义版本控制工具，仅包含 major.minor.patch 三个数值组件。预发布标签（`-alpha`、`-beta`）会在解析和比较时被**剥离和忽略**。

### 构造函数

```python
from dcc_mcp_core import SemVer

v = SemVer(1, 2, 3)
print(str(v))  # "1.2.3"
```

### 解析版本

```python
from dcc_mcp_core import SemVer

v = SemVer.parse("1.2.3")
print(v.major)  # 1
print(v.minor)  # 2
print(v.patch)  # 3

# 支持前导 "v"
v2 = SemVer.parse("v2.0")
print(v2.major)  # 2
```

### 版本比较

```python
v1 = SemVer.parse("1.2.3")
v2 = SemVer.parse("1.2.4")
v3 = SemVer.parse("2.0.0")

print(v1 < v2)   # True
print(v2 > v1)   # True
print(v3 > v1)   # True
print(v1 == SemVer.parse("1.2.3"))  # True
```

### 版本排序

```python
versions = [SemVer.parse("2.0.0"), SemVer.parse("1.0.0"), SemVer.parse("1.2.3")]
sorted_versions = sorted(versions)
print([str(v) for v in sorted_versions])  # ["1.0.0", "1.2.3", "2.0.0"]
```

### 错误处理

```python
from dcc_mcp_core import VersionParseError

try:
    v = SemVer.parse("invalid")
except VersionParseError as e:
    print(f"Invalid version: {e}")
```

::: tip
`SemVer` 只有三个数值组件（`major`、`minor`、`patch`）。预发布标签和 build 元数据会被剥离和忽略。
:::

## VersionConstraint

版本需求规范，用于与注册的 Action 版本进行匹配。

### 创建约束

```python
from dcc_mcp_core import VersionConstraint

# 各种约束类型
constraint1 = VersionConstraint.parse(">=1.0.0,<2.0.0")
constraint2 = VersionConstraint.parse("^1.2.3")  # 兼容 1.x.x
constraint3 = VersionConstraint.parse("~1.2.3")  # Patch 兼容（1.2.x）
constraint4 = VersionConstraint.parse("1.2.3")   # 精确版本
constraint5 = VersionConstraint.parse("*")       # 任意版本
```

### 检查约束

```python
from dcc_mcp_core import SemVer, VersionConstraint

v = SemVer.parse("1.5.0")
constraint = VersionConstraint.parse(">=1.0.0,<2.0.0")
print(constraint.matches(v))  # True
```

### 支持的约束格式

| 格式 | 示例 | 说明 |
|------|------|------|
| 精确 | `1.2.3` | 必须精确匹配 |
| 大于 | `>1.2.3` | 必须严格大于 |
| 范围 | `>=1.0.0,<2.0.0` | 在范围内 |
| Caret | `^1.2.3` | 同主版本（1.x.x） |
| Tilde | `~1.2.3` | 同主次版本（1.2.x） |
| 通配符 | `*` | 任意版本 |

## VersionedRegistry

多版本 Action 注册表。允许同一 `(action_name, dcc_name)` 的多个版本共存。提供基于约束条件解析最佳匹配版本。

### 构造函数

```python
from dcc_mcp_core import VersionedRegistry
registry = VersionedRegistry()
```

### 注册版本

```python
registry.register_versioned(
    "create_sphere",
    dcc="maya",
    version="1.0.0",
    description="Create a sphere",
    category="geometry",
    tags=["geo", "create"],
)

registry.register_versioned(
    "create_sphere",
    dcc="maya",
    version="2.0.0",
    description="Create a sphere with segments",
    category="geometry",
    tags=["geo", "create"],
)

registry.register_versioned(
    "create_sphere",
    dcc="blender",
    version="1.0.0",
    description="Blender sphere creation",
)
```

### 解析版本

```python
# 获取指定 (name, dcc) 的所有注册版本
versions = registry.versions("create_sphere", "maya")
print(versions)  # ["1.0.0", "2.0.0"]

# 获取最新版本字符串
latest = registry.latest_version("create_sphere", "maya")
print(latest)  # "2.0.0"

# 解析约束的最佳匹配 — 返回元数据字典或 None
result = registry.resolve("create_sphere", "maya", "^1.0.0")
if result:
    print(result["version"])   # "2.0.0"
    print(result["category"])  # "geometry"

# 解析所有满足约束的版本
all_matches = registry.resolve_all("create_sphere", "maya", ">=1.0.0,<3.0.0")
for m in all_matches:
    print(m["version"])  # ["1.0.0", "2.0.0"]
```

### 注册表内省

```python
# 所有注册的 (name, dcc) 键
keys = registry.keys()
print(keys)  # [("create_sphere", "maya"), ("create_sphere", "blender")]

# 总条目数
print(registry.total_entries())  # 3

# 按约束移除版本
removed = registry.remove("create_sphere", "maya", "^1.0.0")
print(removed)  # 2（移除了 1.0.0 和 2.0.0）
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register_versioned(name, dcc, version, description, category, tags)` | — | 注册一个 Action 版本 |
| `versions(name, dcc)` | `List[str]` | 所有版本，递增排序 |
| `latest_version(name, dcc)` | `str?` | 最高版本字符串或 None |
| `resolve(name, dcc, constraint)` | `dict?` | 最佳匹配元数据字典或 None |
| `resolve_all(name, dcc, constraint)` | `List[dict]` | 所有匹配的元数据字典列表 |
| `keys()` | `List[tuple]` | 所有 `(name, dcc)` 键 |
| `total_entries()` | `int` | 总条目数 |
| `remove(name, dcc, constraint)` | `int` | 移除数量（按约束） |

::: tip
`resolve()` 和 `resolve_all()` 内部使用 `VersionConstraint.parse()` —— 传入约束字符串如 `"^1.0.0"` 或 `">=1.0.0,<2.0.0"`。
:::
