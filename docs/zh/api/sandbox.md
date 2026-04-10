# 沙盒 API

`dcc_mcp_core` (sandbox 模块)

带有 API 白名单、路径控制、审计日志和输入验证的脚本执行沙盒。

## 概述

企业级用户（游戏工作室、VFX 机构）有严格的安全需求，而原始的基于 Python 的 DCC MCP 集成无法满足。沙盒模块提供：

- **API 白名单** — 限制 AI 智能体可调用的 DCC 操作
- **路径白名单** — 将文件系统访问限制在安全目录内
- **审计日志** — 每次操作调用的结构化记录（`AuditEntry` / `AuditLog`）
- **输入验证** — 带注入防护的字段级规则（`InputValidator`）
- **只读模式** — 智能体可查询但不能修改场景

## SandboxPolicy

沙盒会话的安全策略配置。

### 构造函数

```python
from dcc_mcp_core import SandboxPolicy

policy = SandboxPolicy()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `allow_actions(actions)` | `None` | 限制只允许这些操作（替换之前的白名单）|
| `deny_actions(actions)` | `None` | 即使在白名单中也拒绝这些操作 |
| `allow_paths(paths)` | `None` | 允许访问这些目录路径内的文件系统 |
| `set_timeout_ms(ms)` | `None` | 设置执行超时（毫秒）|
| `set_max_actions(count)` | `None` | 设置每个会话允许的最大操作数 |
| `set_read_only(read_only)` | `None` | 启用/禁用只读模式 |

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `is_read_only` | `bool` | 策略是否处于只读模式 |

### 示例

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "get_object_info"])
policy.deny_actions(["delete_scene", "delete_object"])
policy.allow_paths(["/studio/assets", "/tmp/renders"])
policy.set_timeout_ms(5000)
policy.set_max_actions(100)
policy.set_read_only(False)

print(policy.is_read_only)  # False
```

::: tip 使用白名单而非黑名单
从**不允许任何操作**开始，然后明确许可安全的操作。这比列出所有要阻止的内容更安全。
:::

## SandboxContext

主沙盒执行上下文。将 `SandboxPolicy`、`AuditLog` 和操作计数器组合在一起。

### 构造函数

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])

ctx = SandboxContext(policy)
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `set_actor(actor)` | `None` | 设置审计条目的调用者身份 |
| `execute_json(action, params_json)` | `str` | 使用 JSON 参数执行操作，返回 JSON 结果 |
| `is_allowed(action)` | `bool` | 检查操作是否被当前策略允许 |
| `is_path_allowed(path)` | `bool` | 检查路径是否在允许目录内 |

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `action_count` | `int` | 成功执行的操作数量 |
| `audit_log` | `AuditLog` | 当前上下文的审计日志 |

### 执行

```python
ctx.set_actor("claude-agent")

# 使用 JSON 参数执行——返回 JSON 字符串
result = ctx.execute_json("get_scene_info", "{}")
print(result)  # '{"name": "my_scene", "object_count": 42, ...}'

# 在不执行的情况下检查权限
if ctx.is_allowed("delete_object"):
    ctx.execute_json("delete_object", '{"name": "pSphere1"}')
```

::: warning 错误会抛出 RuntimeError
如果操作被拒绝、验证失败或发生沙盒错误，`execute_json()` 会抛出 `RuntimeError`。
:::

### 审计日志访问

```python
log = ctx.audit_log
print(len(log))  # 已记录条目数

for entry in log.entries():
    print(f"{entry.actor}: {entry.action} → {entry.outcome} ({entry.duration_ms}ms)")

# 筛选视图
denied = log.denials()
succeeded = log.successes()

# 序列化为 JSON
json_str = log.to_json()
```

## AuditEntry

单次操作调用的审计记录。所有属性均为只读属性。

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `timestamp_ms` | `int` | Unix 时间戳（毫秒）|
| `actor` | `str \| None` | 调用者身份，或 `None` |
| `action` | `str` | 操作名称 |
| `params_json` | `str` | JSON 字符串形式的参数 |
| `duration_ms` | `int` | 执行耗时（毫秒）|
| `outcome` | `str` | `"success"`、`"denied"`、`"error"` 或 `"timeout"` |
| `outcome_detail` | `str \| None` | 拒绝原因或错误消息，或 `None` |

```python
for entry in ctx.audit_log.entries():
    print(f"[{entry.timestamp_ms}] {entry.actor}: {entry.action}")
    print(f"  参数: {entry.params_json}")
    print(f"  结果: {entry.outcome} ({entry.duration_ms}ms)")
    if entry.outcome_detail:
        print(f"  详情: {entry.outcome_detail}")
```

## AuditLog

沙盒审计日志的只读视图。

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `entries()` | `list[AuditEntry]` | 所有已记录的条目 |
| `successes()` | `list[AuditEntry]` | outcome 为 `"success"` 的条目 |
| `denials()` | `list[AuditEntry]` | outcome 为 `"denied"` 的条目 |
| `entries_for_action(action)` | `list[AuditEntry]` | 特定操作的条目 |
| `to_json()` | `str` | 所有条目序列化为 JSON 数组字符串 |
| `__len__()` | `int` | 条目数量 |

```python
log = ctx.audit_log

print(f"总计: {len(log)}")
print(f"成功: {len(log.successes())}")
print(f"拒绝: {len(log.denials())}")

# 查询特定操作的历史记录
scene_queries = log.entries_for_action("get_scene_info")

# 导出到日志系统
json_str = log.to_json()
```

## InputValidator

带注入防护的字段级输入验证器。在调用 `SandboxContext.execute_json()` 之前对参数进行验证。

### 构造函数

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `require_string(field, max_length=None, min_length=None)` | `None` | 注册必填字符串字段，支持可选长度约束 |
| `require_number(field, min_value=None, max_value=None)` | `None` | 注册必填数值字段，支持可选范围约束 |
| `forbid_substrings(field, substrings)` | `None` | 注入防护：字段不得包含任何指定子串 |
| `validate(params_json)` | `tuple[bool, str \| None]` | 验证 JSON 参数；返回 `(True, None)` 或 `(False, 错误消息)` |

### 示例

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()

# 字段约束
validator.require_string("name", min_length=1, max_length=64)
validator.require_number("radius", min_value=0.01, max_value=1000.0)

# 注入防护
validator.forbid_substrings("script", ["__import__", "exec(", "eval(", "os.system"])

# 有效输入
ok, error = validator.validate('{"name": "sphere1", "radius": 2.0}')
print(ok, error)  # True, None

# 注入尝试被阻止
ok, error = validator.validate('{"script": "__import__(os)"}')
print(ok, error)  # False, "field 'script' contains forbidden substring '__import__'"

# 无效 JSON 抛出 RuntimeError
try:
    validator.validate("not json")
except RuntimeError as e:
    print(f"无效 JSON: {e}")
```

::: warning InputValidator vs ActionValidator
`InputValidator` 用于**沙盒字段级规则**（长度、范围、注入防护）。
`ActionValidator`（来自 actions 模块）根据 **JSON Schema** 进行验证。
在沙盒中使用 `InputValidator`；在操作分派层使用 `ActionValidator`。
:::

## 最佳实践

1. **始终使用白名单** — 从不允许任何操作开始，然后明确许可安全的操作
2. **设置超时** — 防止失控脚本挂起 DCC
3. **限制操作数量** — `set_max_actions()` 防止 AI 智能体进入无限循环
4. **启用审计日志** — 会话结束后始终检查 `ctx.audit_log`
5. **使用只读模式** — 仅查询数据时启用，防止意外修改
6. **添加注入防护** — 对任何脚本/代码参数使用 `InputValidator.forbid_substrings()`
7. **验证路径** — 使用 `allow_paths()` + `ctx.is_path_allowed()` 防止路径遍历

## 完整示例

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext, InputValidator

# 构建策略
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "run_script"])
policy.allow_paths(["/studio/assets"])
policy.set_timeout_ms(10000)
policy.set_max_actions(50)

# 为 run_script 构建验证器
validator = InputValidator()
validator.require_string("script", max_length=10000)
validator.forbid_substrings("script", [
    "__import__", "exec(", "eval(", "os.system", "subprocess",
])

# 创建上下文
ctx = SandboxContext(policy)
ctx.set_actor("my-ai-agent")

# 执行安全查询
result = ctx.execute_json("get_scene_info", "{}")
print(result)

# 尝试受限操作（被策略阻止）
try:
    ctx.execute_json("delete_scene", "{}")
except RuntimeError as e:
    print(f"已阻止: {e}")

# 审查审计日志
for entry in ctx.audit_log.entries():
    print(f"{entry.action}: {entry.outcome}")
```
