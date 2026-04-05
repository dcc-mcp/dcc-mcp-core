# Sandbox API

`dcc_mcp_core` (sandbox 模块)

脚本执行沙箱，带 API 白名单、审计日志和输入验证。

## 概述

企业用户（游戏工作室、VFX 影视制作公司）有严格的安全要求，沙箱提供：

- **API 白名单/黑名单** — 限制 Agent 可以调用的 DCC 操作
- **审计日志** — 每个操作调用的防篡改记录
- **输入验证** — 执行前的模式验证
- **只读模式** — Agent 只能查询不能修改场景
- **操作限流** — 每个会话的最大操作数
- **路径白名单** — 文件系统访问限制

## SandboxContext

主要沙箱执行上下文。

### 构造函数

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects"]) \
    .timeout_ms(5000) \
    .build()

ctx = SandboxContext(policy)
ctx = ctx.with_actor("my-agent")
```

### 方法

| 方法 | 返回值 | 描述 |
|------|--------|------|
| `with_actor(name)` | `SandboxContext` | 设置审计的执行者名称 |
| `execute(action, params, validator, handler)` | `ExecutionResult` | 执行操作 |
| `action_count()` | `int` | 已执行的操作数 |
| `audit_log()` | `AuditLog` | 获取审计日志 |
| `reset()` | — | 重置上下文 |

### 执行

```python
result = ctx.execute(
    "get_scene_info",
    json.dumps({}),
    None,  # 无自定义验证器
    None   # 无自定义处理器
)

print(result.outcome)     # "success" 或 "denied"
print(result.duration_ms) # 执行时间
print(result.error)       # 失败时的错误消息
```

## SandboxPolicy

安全策略配置。

### Builder

```python
policy = SandboxPolicy.builder() \
    .allow_actions(["get_info", "list_objects"]) \
    .deny_actions(["delete_all", "format_disk"]) \
    .max_actions(10) \
    .timeout_ms(5000) \
    .read_only(True) \
    .build()
```

### 策略选项

| 选项 | 类型 | 描述 |
|------|------|------|
| `allow_actions` | `List[str]` | 允许操作的白名单 |
| `deny_actions` | `List[str]` | 拒绝操作的黑名单 |
| `max_actions` | `int` | 每个会话的最大操作数 |
| `timeout_ms` | `int` | 每个操作的最大执行时间 |
| `read_only` | `bool` | True 则拒绝所有写操作 |
| `allowed_paths` | `List[str]` | 允许的文件系统路径 |
| `rate_limit` | `int` | 每分钟最大操作数 |

## AuditLog

防篡改审计跟踪。

### 方法

```python
log = ctx.audit_log()

print(f"总条目: {len(log)}")
print(f"成功: {len(log.successes())}")
print(f"拒绝: {len(log.denials())}")
print(f"失败: {len(log.failures())}")

for entry in log.entries:
    print(f"{entry.timestamp}: {entry.action} - {entry.outcome}")
```

### AuditEntry

每个条目包含：

| 字段 | 类型 | 描述 |
|------|------|------|
| `timestamp` | `datetime` | 操作尝试时间 |
| `actor` | `str` | 谁发起的操作 |
| `action` | `str` | 操作名称 |
| `params` | `dict` | 操作参数 |
| `outcome` | `AuditOutcome` | success/denied/failed |
| `duration_ms` | `int` | 执行时长 |
| `error` | `str?` | 失败时的错误消息 |

## InputValidator

执行前的基于模式的输入验证。

### 创建验证器

```python
from dcc_mcp_core import InputValidator, FieldSchema, ValidationRule

validator = InputValidator().register(
    "script",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.FORBIDDEN_SUBSTRINGS, ["__import__", "exec(", "eval("])
)
```

### 验证规则

| 规则 | 描述 |
|------|------|
| `IS_STRING` | 值必须是字符串 |
| `IS_NUMBER` | 值必须是数字 |
| `IS_BOOL` | 值必须是布尔值 |
| `MIN_LENGTH` | 字符串最小长度 |
| `MAX_LENGTH` | 字符串最大长度 |
| `PATTERN` | 正则表达式匹配 |
| `FORBIDDEN_SUBSTRINGS` | 禁止的子字符串 |
| `ALLOWED_VALUES` | 枚举限制 |

### 使用验证器

```python
malicious = {"script": "__import__('os').system('rm -rf /')"}

try:
    result = ctx.execute("run_script", malicious, validator, None)
except SandboxError as e:
    print(f"验证失败: {e}")
```

## ExecutionResult

沙箱操作执行结果。

```python
result = ctx.execute("list_objects", {}, None, None)

# 属性
result.outcome    # AuditOutcome 枚举
result.error       # 失败时的错误消息
result.duration_ms # 毫秒执行时长
result.output      # 操作输出数据
```

## 错误处理

```python
from dcc_mcp_core import SandboxError

try:
    ctx.execute("forbidden_action", {}, None, None)
except SandboxError as e:
    print(f"沙箱错误: {e}")
```

## 最佳实践

1. **始终使用白名单** — 从不允许任何操作开始，然后明确允许安全操作
2. **设置超时** — 防止失控脚本挂起 DCC
3. **启用审计日志** — 为安全审计保留记录
4. **使用只读模式** — 只查询数据时启用只读以防止意外修改
5. **验证所有输入** — 在到达 DCC 代码之前使用 InputValidator 阻止注入攻击
