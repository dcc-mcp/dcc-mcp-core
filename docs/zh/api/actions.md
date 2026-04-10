# Skills API

`dcc_mcp_core` — ActionRegistry、EventBus、ActionDispatcher、ActionValidator、SemVer、VersionConstraint、VersionedRegistry。

## ActionRegistry

线程安全的 Skill 注册表，底层使用 DashMap 实现。每个注册表实例相互独立。

### 构造函数

```python
from dcc_mcp_core import ActionRegistry
registry = ActionRegistry()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register(name, description="", category="", tags=[], dcc="python", version="1.0.0", input_schema=None, output_schema=None, source_file=None)` | — | 注册一个 Skill |
| `get_action(name, dcc_name=None)` | `dict?` | 获取 Skill 元数据字典 |
| `list_actions(dcc_name=None)` | `List[dict]` | 列出所有 Skill 的元数据字典 |
| `list_actions_for_dcc(dcc_name)` | `List[str]` | 列出指定 DCC 的 Skill 名称 |
| `get_all_dccs()` | `List[str]` | 列出所有已注册的 DCC 名称 |
| `search_actions(category=None, tags=[], dcc_name=None)` | `List[dict]` | AND 组合过滤搜索 |
| `get_categories(dcc_name=None)` | `List[str]` | 排序后的唯一类别列表 |
| `get_tags(dcc_name=None)` | `List[str]` | 排序后的唯一标签列表 |
| `count_actions(category=None, tags=[], dcc_name=None)` | `int` | 符合条件的 Skill 数量 |
| `reset()` | — | 清除所有已注册的 Skill |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__len__` | 已注册 Skill 的数量 |
| `__contains__(name)` | 检查 Skill 名称是否已注册（作用域为 "python" dcc） |
| `__repr__` | `ActionRegistry(actions=N)` |

### Skill 元数据字典

通过 `get_action()`、`list_actions()` 或 `search_actions()` 获取时，每个 Skill 是一个字典：

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

基于 JSON Schema 的 Skill 输入验证器。通过 schema 字符串或 `ActionRegistry` 中的 skill 创建。

### 静态工厂方法

```python
from dcc_mcp_core import ActionValidator

# 从 JSON Schema 字符串创建
validator = ActionValidator.from_schema_json(
    '{"type": "object", "required": ["radius"], '
    '"properties": {"radius": {"type": "number", "minimum": 0.0}}}'
)

# 从 ActionRegistry 中的 skill 创建
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

基于版本兼容性的 Skill 路由到处理器函数。

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

### 分发 Skill

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
try:
    v = SemVer.parse("invalid")
except ValueError as e:
    print(f"Invalid version: {e}")
```

::: tip
`SemVer` 只有三个数值组件（`major`、`minor`、`patch`）。预发布标签和 build 元数据会被剥离和忽略。
:::

## VersionConstraint

版本需求规范，用于与注册的 Skill 版本进行匹配。

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

多版本 Skill 注册表。允许同一 `(skill_name, dcc_name)` 的多个版本共存。提供基于约束条件解析最佳匹配版本。

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
| `register_versioned(name, dcc, version, description, category, tags)` | — | 注册一个 Skill 版本 |
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


## ActionPipeline

`ActionDispatcher` 的中间件包装器。以可组合的方式叠加日志、计时、审计和速率限制中间件。

### 构造函数

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
dispatcher = ActionDispatcher(reg)
pipeline = ActionPipeline(dispatcher)
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `dispatch(action, params_json)` | `dict` | 通过所有中间件层进行调度 |
| `register_handler(name, fn)` | — | 注册 Python 处理器（与 `ActionDispatcher` 一致）|
| `add_logging(log_params=False)` | — | 添加 trace 日志中间件 |
| `add_timing()` | `TimingMiddleware` | 添加延迟统计；返回句柄 |
| `add_audit(record_params=False)` | `AuditMiddleware` | 添加审计日志；返回句柄 |
| `add_rate_limit(max_calls, window_ms)` | `RateLimitMiddleware` | 添加速率限制；返回句柄 |
| `add_callable(before_fn, after_fn)` | — | 添加 Python 可调用钩子 |
| `middleware_count()` | `int` | 已注册的中间件层数 |
| `middleware_names()` | `List[str]` | 按管道顺序排列的中间件名称 |
| `handler_count()` | `int` | 已注册的处理器数量 |

### dispatch() 返回值

`dispatch()` 返回包含以下键的字典：

| 键 | 类型 | 说明 |
|----|------|------|
| `action` | `str` | Skill 名称 |
| `output` | `Any` | 处理器返回值 |
| `success` | `bool` | 无异常时为 `True` |
| `error` | `str?` | 失败时的错误信息 |
| `validation_skipped` | `bool` | JSON Schema 验证是否执行 |

### TimingMiddleware

```python
timing = pipeline.add_timing()
pipeline.dispatch("my_action", '{}')

ms = timing.last_elapsed_ms("my_action")  # int | None — 最后一次调用耗时（ms）
```

### AuditMiddleware

```python
audit = pipeline.add_audit(record_params=True)
pipeline.dispatch("my_action", '{}')

records = audit.records()                        # 所有记录
records = audit.records_for_action("my_action")  # 按 Skill 名称筛选
count = audit.record_count()                     # int
audit.clear()
```

每条记录包含：`action`（str）、`success`（bool）、`error`（str | None）、`timestamp_ms`（int）。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `records()` | `List[dict]` | 所有审计记录 |
| `records_for_action(name)` | `List[dict]` | 指定 Action 的记录 |
| `record_count()` | `int` | 总记录数 |
| `clear()` | — | 清空所有记录 |

### RateLimitMiddleware

固定窗口速率限制器。在 `window_ms` 时间窗口内超出 `max_calls` 时抛出 `RuntimeError`。

```python
rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
print(rl.call_count("my_action"))  # 当前窗口的调用次数
print(rl.max_calls)                # 10
print(rl.window_ms)                # 1000
```

### 完整示例

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
reg.register("process_mesh", description="处理网格", category="geometry")
dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("process_mesh", lambda p: {"vertices": 1024})

pipeline = ActionPipeline(dispatcher)
pipeline.add_logging(log_params=True)
timing = pipeline.add_timing()
audit = pipeline.add_audit(record_params=True)
rl = pipeline.add_rate_limit(max_calls=100, window_ms=60000)

result = pipeline.dispatch("process_mesh", '{"mesh_name": "cube"}')
print(result["output"])                          # {"vertices": 1024}
print(timing.last_elapsed_ms("process_mesh"))    # 如：12
print(audit.record_count())                      # 1
```
