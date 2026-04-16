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

---

## ToolRecorder

记录每个 Action 的执行耗时与成功/失败计数。用于为代码中执行的任意 Action 收集性能遥测数据。

### 构造函数

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("my-service")
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `scope` | `str` | 该 recorder 实例的逻辑名称（如服务名或模块名） |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `start(action_name, dcc_name)` | `RecordingGuard` | 开始计时；返回 RAII 守卫对象 |
| `metrics(action_name)` | `ToolMetrics \| None` | 指定 Action 的聚合指标；无数据时返回 `None` |
| `all_metrics()` | `list[ToolMetrics]` | 所有已记录 Action 的聚合指标 |
| `reset()` | `None` | 清空所有内存统计数据 |

### 示例

```python
from dcc_mcp_core import ToolRecorder

recorder = ToolRecorder("maya-skill-server")

# 手动守卫方式
guard = recorder.start("create_sphere", "maya")
try:
    # ... 执行工作 ...
    guard.finish(success=True)
except Exception:
    guard.finish(success=False)
    raise

# 上下文管理器方式（无异常时自动 success=True）
with recorder.start("delete_mesh", "maya"):
    pass  # 在此执行工作

# 查询指标
m = recorder.metrics("create_sphere")
if m:
    print(f"调用次数={m.invocation_count}, 成功率={m.success_rate():.2%}")
    print(f"均值={m.avg_duration_ms:.1f}ms  P95={m.p95_duration_ms:.1f}ms")
```

---

## RecordingGuard

`ToolRecorder.start()` 返回的 RAII 守卫对象，自动记录耗时与执行结果。

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `finish(success)` | `None` | 以指定成功标志提交记录 |
| `__enter__` | `RecordingGuard` | 上下文管理器入口 |
| `__exit__` | `None` | 上下文管理器出口（无异常时 success=True） |

---

## ToolMetrics

单个 Action 的性能指标只读快照。通过 `ToolRecorder.metrics()` 或 `ToolRecorder.all_metrics()` 获取。

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `action_name` | `str` | 该指标所属的 Action 名称 |
| `invocation_count` | `int` | 总调用次数 |
| `success_count` | `int` | 成功次数 |
| `failure_count` | `int` | 失败次数 |
| `avg_duration_ms` | `float` | 平均执行耗时（毫秒） |
| `p95_duration_ms` | `float` | P95 执行耗时（毫秒） |
| `p99_duration_ms` | `float` | P99 执行耗时（毫秒） |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `success_rate()` | `float` | 成功率，范围 `[0.0, 1.0]` |

### 示例

```python
recorder = ToolRecorder("server")

for _ in range(10):
    with recorder.start("ping", "maya"):
        pass

all_m = recorder.all_metrics()
for m in all_m:
    print(
        f"{m.action_name}: "
        f"{m.invocation_count} 次调用, "
        f"{m.success_rate():.0%} 成功率, "
        f"均值 {m.avg_duration_ms:.1f}ms"
    )
```
