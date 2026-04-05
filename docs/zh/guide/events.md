# 事件系统

DCC-MCP-Core 提供发布/订阅事件系统 `EventBus`，用于解耦的动作生命周期通信。

## 使用方法

```python
from dcc_mcp_core.actions.events import event_bus

def on_action_done(data):
    print(f"动作 {data['action_name']} 完成: {data['result'].success}")

# 订阅事件
event_bus.subscribe("action.after_execute.create_sphere", on_action_done)

# 取消订阅
event_bus.unsubscribe("action.after_execute.create_sphere", on_action_done)
```

## 内置事件

ActionManager 自动发布的事件：

| 事件 | 说明 |
|------|------|
| `action_manager.created` | 管理器实例创建 |
| `action_manager.before_discover_path` | 从路径发现动作前 |
| `action_manager.after_discover_path` | 从路径发现动作后 |
| `action_manager.before_refresh` | 刷新所有动作前 |
| `action_manager.after_refresh` | 刷新所有动作后 |
| `action.before_execute.{name}` | 执行特定动作前 |
| `action.after_execute.{name}` | 执行特定动作后 |
| `action.error.{name}` | 动作出错时 |
| `skill.loaded` | 技能包加载时 |

## 事件数据

事件处理器接收包含相关信息的 `data` 字典：

```python
def on_before_execute(data):
    action_name = data["action_name"]
    kwargs = data.get("kwargs", {})
    print(f"即将执行 {action_name}，参数: {kwargs}")

def on_after_execute(data):
    action_name = data["action_name"]
    result = data["result"]  # ActionResultModel
    print(f"{action_name}: {'成功' if result.success else '失败'}")

event_bus.subscribe("action.before_execute.create_sphere", on_before_execute)
event_bus.subscribe("action.after_execute.create_sphere", on_after_execute)
```
