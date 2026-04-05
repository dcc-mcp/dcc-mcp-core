# 事件 API

`dcc_mcp_core.EventBus`

## EventBus

线程安全的发布/订阅事件总线，底层使用 DashMap 实现。

### 构造函数

```python
from dcc_mcp_core import EventBus
bus = EventBus()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `subscribe(event_name, callback)` | `int` | 订阅事件，返回订阅者 ID |
| `unsubscribe(event_name, subscriber_id)` | `bool` | 按 ID 取消订阅，返回是否找到 |
| `publish(event_name, **kwargs)` | — | 调用所有订阅者，传递关键字参数 |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `EventBus(subscriptions=N)` |

### 行为

- 订阅者通过 `publish(event_name, **kwargs)` 接收关键字参数
- 订阅者中的异常会通过 `tracing` 记录日志，但不会传播
- 回调在调用前会先收集，以避免 DashMap 死锁
- 每个事件支持多个订阅者
- 订阅者 ID 单调递增（从 1 开始）

### 示例

```python
bus = EventBus()

def on_action(action_name=None, **kwargs):
    print(f"Action: {action_name}")

sid = bus.subscribe("action.executed", on_action)
bus.publish("action.executed", action_name="create_sphere")
bus.unsubscribe("action.executed", sid)
```
