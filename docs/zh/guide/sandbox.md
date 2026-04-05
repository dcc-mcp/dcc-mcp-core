# 沙箱指南

带 API 白名单和审计日志的脚本执行沙箱。

## 概述

企业用户（游戏工作室、VFX 影视制作公司）有严格的安全要求， vanilla Python DCC MCP 集成无法满足。沙箱提供：

- **API 白名单/黑名单** — 限制 Agent 可以调用的 DCC 操作
- **审计日志** — 每个操作调用的防篡改记录
- **输入验证** — 执行前的模式验证
- **只读模式** — Agent 只能查询不能修改场景
- **操作限流** — 每个会话的最大操作数
- **路径白名单** — 文件系统访问限制

## 快速开始

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

# 创建限制性策略
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects", "get_selection"]) \
    .timeout_ms(5000) \
    .build()

# 创建沙箱上下文
ctx = SandboxContext(policy)
ctx = ctx.with_actor("ai-agent")

# 执行允许的操作
result = ctx.execute("get_scene_info", json.dumps({}), None, None)
print(f"结果: {result.outcome}")

# 尝试禁止的操作（将被拒绝）
result = ctx.execute("delete_all", json.dumps({}), None, None)
print(f"被拒绝: {result.outcome == 'denied'}")
```

## 策略配置

### 白名单模式

```python
# 只允许特定操作
policy = SandboxPolicy.builder() \
    .allow_actions([
        "get_scene_info",
        "list_objects",
        "get_selection",
        "query_attributes"
    ]) \
    .build()
```

### 黑名单模式

```python
# 阻止危险操作
policy = SandboxPolicy.builder() \
    .allow_actions(["*"])  # 允许所有，除了...
    .deny_actions([
        "delete_all",
        "format_disk",
        "run_external_command"
    ]) \
    .build()
```

### 只读模式

```python
# 只允许场景查询，不允许修改
policy = SandboxPolicy.builder() \
    .allow_actions(["get_*", "list_*", "query_*"]) \
    .read_only(True) \
    .build()
```

## 审计日志

### 访问审计日志

```python
# 执行一些操作
ctx.execute("get_scene_info", json.dumps({}), None, None)
ctx.execute("list_objects", json.dumps({}), None, None)

# 获取审计日志
log = ctx.audit_log()

print(f"总操作数: {len(log)}")
print(f"成功: {len(log.successes())}")
print(f"拒绝: {len(log.denials())}")
print(f"失败: {len(log.failures())}")
```

### 审计条目详情

```python
log = ctx.audit_log()

for entry in log.entries:
    print(f"时间: {entry.timestamp}")
    print(f"执行者: {entry.actor}")
    print(f"操作: {entry.action}")
    print(f"参数: {entry.params}")
    print(f"结果: {entry.outcome}")
    print(f"耗时: {entry.duration_ms}ms")
    if entry.error:
        print(f"错误: {entry.error}")
```

## 输入验证

### 定义模式

```python
from dcc_mcp_core import InputValidator, FieldSchema, ValidationRule

validator = InputValidator()

# 为脚本执行注册模式
validator.register(
    "run_script",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.MAX_LENGTH, 10000)
        .rule(ValidationRule.FORBIDDEN_SUBSTRINGS, [
            "__import__",
            "exec(",
            "eval(",
            "subprocess",
            "os.system"
        ])
)

# 为文件操作注册模式
validator.register(
    "read_file",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.PATTERN, r"^/project/.*")
)
```

### 使用验证器

```python
# 安全输入
safe_input = {"script": "print('hello world')"}
result = ctx.execute("run_script", json.dumps(safe_input), validator, None)
# 结果: success

# 恶意输入（被验证阻止）
malicious_input = {"script": "__import__('os').system('rm -rf /')"}
result = ctx.execute("run_script", json.dumps(malicious_input), validator, None)
# 结果: validation_failed
```

## 最佳实践

### 1. 从限制性开始

```python
# 初始最小权限
policy = SandboxPolicy.builder() \
    .allow_actions([])  # 最初不允许任何操作
    .build()

# 按需添加
policy = policy.add_allowlist(["get_scene_info"])
```

### 2. 分离读写

```python
# 只读上下文
read_ctx = SandboxContext(read_policy)
read_ctx = read_ctx.with_actor("query-agent")

# 写操作上下文
write_ctx = SandboxContext(write_policy)
write_ctx = write_ctx.with_actor("mutation-agent")
```

### 3. 监控和告警

```python
# 检查可疑模式
log = ctx.audit_log()
denials = [e for e in log.denials() if "delete" in e.action]

if len(denials) > 5:
    alert_security_team(denials)
```

## 集成示例

### Maya 集成

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import maya.cmds as cmds
import json

# 创建 Maya 特定策略
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects", "query_attributes"]) \
    .deny_actions(["delete_*", "set_*", "create_*"]) \
    .read_only(True) \
    .build()

maya_sandbox = SandboxContext(policy)
maya_sandbox = maya_sandbox.with_actor("maya-agent")

# 包装 Maya 命令
def safe_maya_action(action, params):
    return maya_sandbox.execute(action, json.dumps(params), None, None)
```
