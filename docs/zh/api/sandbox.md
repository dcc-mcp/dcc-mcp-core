# 沙箱 API

`dcc_mcp_core` (sandbox 模块)

带 API 白名单、审计日志和输入验证的脚本执行沙箱。

## 概述

企业用户（游戏工作室、VFX 影视制作公司）有严格的安全要求，vanilla Python DCC MCP 集成无法满足。沙箱提供：

- **API 白名单** — 限制 Agent 可以调用的 DCC 操作
- **审计日志** — 每个操作调用的结构化记录
- **输入验证** — 执行前的模式验证
- **只读模式** — Agent 只能查询不能修改场景

## SandboxPolicy

安全策略配置。

### 构造函数

```python
from dcc_mcp_core import SandboxPolicy

policy = SandboxPolicy()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `allow_actions(actions)` | `None` | 设置允许的操作列表 |
| `deny_actions(actions)` | `None` | 设置拒绝的操作列表 |
| `set_read_only(read_only)` | `None` | 启用只读模式 |
| `set_timeout_ms(timeout_ms)` | `None` | 设置执行超时 |

### 示例

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])
policy.set_read_only(True)
policy.set_timeout_ms(5000)
```

## SandboxContext

主要沙箱执行上下文。

### 构造函数

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])

ctx = SandboxContext(policy)
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `set_actor(name)` | `None` | 设置执行者名称用于审计 |
| `execute_json(action, params_json, validator=None)` | `str` | 使用 JSON 参数执行操作 |
| `audit_log()` | `list[dict]` | 获取审计日志 |

### 执行

```python
ctx.set_actor("my-agent")

# 使用 JSON 参数执行
result = ctx.execute_json("get_scene_info", "{}")
print(result)

# 使用自定义验证器
result = ctx.execute_json("run_script", '{"script": "print(1)"}', validator=validator)
```

### 审计日志

```python
log = ctx.audit_log()

for entry in log:
    print(f"执行者: {entry['actor']}")
    print(f"操作: {entry['action']}")
    print(f"结果: {entry['outcome']}")
```

## InputValidator

执行前的基于模式的输入验证。

### 构造函数

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `set_rules(rules)` | `None` | 设置每个操作的验证规则 |
| `add_forbidden_patterns(action, patterns)` | `None` | 添加禁止的模式 |

### 示例

```python
validator = InputValidator()
validator.set_rules([
    {"action": "run_script", "max_length": 10000},
])
validator.add_forbidden_patterns("run_script", [
    "__import__",
    "exec(",
    "eval(",
])
```

### 使用验证器

```python
# 安全输入
result = ctx.execute_json("run_script", '{"script": "print(1)"}', validator=validator)

# 恶意输入（被阻止）
result = ctx.execute_json("run_script", '{"script": "__import__"}', validator=validator)
```

## 最佳实践

1. **始终使用白名单** — 从不允许任何操作开始，然后显式允许安全操作
2. **设置超时** — 防止失控脚本挂起 DCC
3. **启用审计日志** — 为安全审计保留记录
4. **使用只读模式** — 仅在查询数据时启用只读以防止意外修改
5. **验证所有输入** — 使用 InputValidator 在攻击到达 DCC 代码之前捕获注入攻击
