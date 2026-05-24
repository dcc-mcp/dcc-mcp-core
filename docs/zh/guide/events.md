# 事件系统

DCC-MCP-Core 提供发布/订阅事件系统 `EventBus`，用于解耦的动作生命周期通信。

## 使用方法

```python
from dcc_mcp_core import EventBus

bus = EventBus()

def on_sphere_done(event, **kwargs):
    print(f"Event: {event}")
    # kwargs 包含事件相关数据

# 订阅事件 — 返回整数订阅者 ID
sub_id = bus.subscribe("action.after_execute.create_sphere", on_sphere_done)

# 使用事件名称和订阅者 ID 取消订阅
bus.unsubscribe("action.after_execute.create_sphere", sub_id)

# 手动发布
bus.publish("my_custom_event", data="value")
```

::: tip
`subscribe()` 返回的是**订阅者 ID**（整数），不是回调函数。将该 ID 传给 `unsubscribe()`，而不是回调函数。
:::

## 事件发现

`EventBus` 是一个通用的发布/订阅系统。发布哪些事件取决于使用它的 DCC 适配器或服务。请参阅您特定 DCC 适配器的文档以获取完整的事件列表。

常见模式：

| 事件模式 | 说明 |
|----------|------|
| `action.before_execute.{name}` | 执行特定动作前 |
| `action.after_execute.{name}` | 特定动作完成后 |
| `action.error.{name}` | 特定动作出错时 |

## 通配符订阅

事件总线支持在事件名称中使用 `*` 作为通配符：

```python
bus = EventBus()

def on_any_after_execute(event, **kwargs):
    print(f"Action completed: {event}")

# 订阅所有 "after_execute" 事件
id1 = bus.subscribe("action.after_execute.*", on_any_after_execute)

# 订阅所有事件
id2 = bus.subscribe("*", on_any_event)
```

## 发布事件

发布自定义事件以实现解耦通信：

```python
bus = EventBus()

# 使用关键字参数发布
bus.publish("scene.saved", file_path="/tmp/scene.usda", size_kb=1024)
bus.publish("scene.opened", file_path="/tmp/scene.usda")
```

## Before Hook 与 Veto

`before()` 用于注册阻塞式策略 hook，只能绑定到支持 veto 的生命周期事件。
回调返回 `None` 或 `False` 表示放行；返回字符串、dict 或
`EventBus.veto(reason, code)` 表示拒绝该操作。

```python
from dcc_mcp_core import EventBus

def policy(event):
    if event["attributes"].get("tool_slug") == "delete_scene":
        return EventBus.veto("destructive tools are disabled", "policy_denied")
    return None

sub_id = bus.before("tool.dispatched", policy)
bus.unsubscribe_before("tool.dispatched", sub_id)
```

当前支持 veto 的事件包括 `skill.loading`、`tool.dispatched`、
`resource.subscribed` 和 `client.initialize`。工具调用被 veto 时会返回
`EVENT_VETOED`，并发布带 `error_kind="event_vetoed"`、`veto_code`、
`veto_reason` 的 `tool.failed` 事件。

## Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `EventBus(subscriptions=N)` |
