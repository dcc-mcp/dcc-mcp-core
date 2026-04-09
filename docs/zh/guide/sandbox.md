# 沙箱指南

带 API 白名单和审计日志的脚本执行沙箱。

## 概述

企业用户（游戏工作室、VFX 影视制作公司）有严格的安全要求， vanilla Python DCC MCP 集成无法满足。沙箱提供：

- **API 白名单** — 限制 Agent 可以调用的 DCC 操作
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
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "get_selection"])

# 创建沙箱上下文
ctx = SandboxContext(policy)
ctx.set_actor("ai-agent")

# 执行允许的操作
result = ctx.execute_json("get_scene_info", "{}")
print(f"结果: {result}")

# 尝试禁止的操作（将被拒绝）
result = ctx.execute_json("delete_all", "{}")
print(f"拒绝: {result}")
```

## 策略配置

### 白名单模式

```python
# 只允许特定操作
policy = SandboxPolicy()
policy.allow_actions([
    "get_scene_info",
    "list_objects",
    "get_selection",
    "query_attributes"
])
```

### 只读模式

```python
# 只允许场景查询，不允许修改
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "query_attributes"])
policy.set_read_only(True)
```

### 设置超时

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info"])
policy.set_timeout_ms(5000)  # 5 秒超时
```

## SandboxContext

### 创建上下文

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])

ctx = SandboxContext(policy)
ctx.set_actor("my-agent")
```

### 执行操作

```python
# JSON 参数执行
result = ctx.execute_json("get_scene_info", "{}")
print(result)

# 带参数执行
params = json.dumps({"pattern": "*"})
result = ctx.execute_json("list_objects", params)
```

### 获取审计日志

```python
# 获取执行历史
log = ctx.audit_log()
for entry in log:
    print(f"执行者: {entry['actor']}")
    print(f"操作: {entry['action']}")
    print(f"结果: {entry['outcome']}")
```

## 输入验证

### 验证器配置

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()

# 注册验证规则
validator.set_rules([
    {"action": "run_script", "max_length": 10000},
    {"action": "read_file", "pattern": "^/project/.*"},
])

# 添加禁止的模式
validator.add_forbidden_patterns("run_script", [
    "__import__",
    "exec(",
    "eval(",
    "subprocess",
    "os.system"
])
```

### 使用验证器

```python
# 安全输入
safe_input = json.dumps({"script": "print('hello world')"})
result = ctx.execute_json("run_script", safe_input, validator=validator)
# 结果: success

# 恶意输入（被验证阻止）
malicious_input = json.dumps({"script": "__import__('os').system('rm -rf /')"})
result = ctx.execute_json("run_script", malicious_input, validator=validator)
# 结果: validation_failed
```

## 最佳实践

### 1. 从限制性开始

```python
# 初始最小权限
policy = SandboxPolicy()
# 最初不允许任何操作

# 按需添加
policy.allow_actions(["get_scene_info"])
```

### 2. 分离读写

```python
# 只读上下文
read_policy = SandboxPolicy()
read_policy.allow_actions(["get_*", "list_*", "query_*"])
read_policy.set_read_only(True)

read_ctx = SandboxContext(read_policy)
read_ctx.set_actor("query-agent")

# 写操作上下文
write_policy = SandboxPolicy()
write_policy.allow_actions(["create_*", "set_*", "delete_*"])

write_ctx = SandboxContext(write_policy)
write_ctx.set_actor("mutation-agent")
```

### 3. 监控和告警

```python
# 检查可疑模式
log = ctx.audit_log()
denials = [e for e in log if e["action"].startswith("delete") and e["outcome"] == "denied"]

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
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "query_attributes"])
policy.deny_actions(["delete_*", "set_*", "create_*"])
policy.set_read_only(True)

maya_sandbox = SandboxContext(policy)
maya_sandbox.set_actor("maya-agent")

# 包装 Maya 命令
def safe_maya_action(action, params):
    return maya_sandbox.execute_json(action, json.dumps(params))
```
