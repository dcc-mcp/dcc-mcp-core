# 事件系统

DCC-MCP-Core 提供发布/订阅事件系统 `EventBus`，用于解耦的动作生命周期通信。

## 使用方法

```python
from dcc_mcp_core.actions.events import event_bus

def on_action_done(data):
    print(f"动作 {data['action_name']} 完成: {data['result'].success}")

event_bus.subscribe("action.after_execute.create_sphere", on_action_done)
event_bus.unsubscribe("action.after_execute.create_sphere", on_action_done)
```

## 内置事件

| 事件 | 说明 |
|------|------|
| `action_manager.created` | 管理器创建 |
| `action_manager.before_discover_path` | 发现动作前 |
| `action_manager.after_discover_path` | 发现动作后 |
| `action.before_execute.{name}` | 执行特定动作前 |
| `action.after_execute.{name}` | 执行特定动作后 |
| `action.error.{name}` | 动作出错时 |
| `skill.loaded` | 技能包加载时 |
