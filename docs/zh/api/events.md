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
| `before(event_name, callback)` | `int` | 为支持 veto 的生命周期事件注册阻塞式策略 hook |
| `unsubscribe(event_name, subscriber_id)` | `bool` | 按 ID 取消订阅，返回是否找到 |
| `unsubscribe_before(event_name, subscriber_id)` | `bool` | 按 ID 移除 before hook |
| `publish(event_name, **kwargs)` | — | 调用所有订阅者，传递关键字参数 |
| `emit(event_name, source=None, correlation=None, attributes=None)` | `dict` | 发布结构化事件 envelope |
| `veto(reason, code="vetoed")` | `dict` | 为 before hook 构造 veto 结果 |
| `vetoable_events()` | `list[str]` | 返回允许 before hook 的事件名 |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `EventBus(subscriptions=N)` |

### 行为

- 订阅者通过 `publish(event_name, **kwargs)` 接收关键字参数
- 订阅者通过 `emit(...)` 接收结构化事件 dict
- 订阅者中的异常会通过 `tracing` 记录日志，但不会传播
- before hook 仅支持 `skill.loading`、`tool.dispatched`、
  `resource.subscribed` 和 `client.initialize`
- before hook 返回 `None`/`False` 表示放行，返回字符串、dict 或
  `veto(...)` 结果表示拒绝
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

### Before Hook Veto

```python
def policy(event):
    if event["attributes"]["tool_slug"] == "delete_scene":
        return EventBus.veto("destructive tools are disabled", "policy_denied")
    return None

sid = bus.before("tool.dispatched", policy)
bus.unsubscribe_before("tool.dispatched", sid)
```

Tool veto 会表现为 `EVENT_VETOED` dispatch error，并发布带有
`error_kind="event_vetoed"`、`veto_code` 和 `veto_reason` 的 `tool.failed`
事件。

### Standalone Server Webhooks

`dcc-mcp-server` 可以把结构化 EventBus envelope 异步转发到 HTTP webhook。
设置 `DCC_MCP_WEBHOOKS_CONFIG` 指向 YAML 文件，或把 `webhooks.yaml` 放在
`~/dcc-mcp/etc` 下（`DCC_MCP_ETC_DIR` 可覆盖该目录）。Admin UI 的
Integrations 面板会写入这个本地文件，并把条目标记为 `pending_restart`，
因为 webhook runtime 在 server 启动时加载。

每个 `webhooks` 条目支持 `name`、`url`、`events`、可选 `kind`、
可选 `headers`、可选 `delivery` 重试设置、可选 dotted-path `filters`，
以及可选 `payload_template`。如果所有投递尝试都失败，server 会在同一个
EventBus 上发布 `webhook.delivery_failed`。

:::: v-pre

```yaml
queue_capacity: 1024
webhooks:
  - name: studio-events
    url: https://ops.example.invalid/dcc-mcp-events
    events:
      - tool.failed
      - gateway.instance.*
    headers:
      authorization: Bearer ${DCC_EVENTS_TOKEN}
    filters:
      - source.dcc_type: maya
    delivery:
      attempts: 3
      timeout_ms: 2000
      backoff_ms: [200, 1000, 5000]
    payload_template: |
      {"event":"{{name}}","tool":"{{attributes.tool_slug}}","dcc":"{{source.dcc_type}}"}
```

::::

`payload_template` 会用结构化 event envelope 中的 `&#123;&#123;path.to.field&#125;&#125;`
占位符填充。省略该字段时，runtime 会发送完整 envelope JSON。

### 企微消息推送

企业微信群机器人可以作为 `kind: wecom` 的 webhook 条目配置。runtime 会按
群机器人接口需要的 markdown payload 投递消息。

```yaml
webhooks:
  - name: wecom-message-push
    kind: wecom
    url: https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=${WECOM_ROBOT_KEY}
    events:
      - tool.failed
      - webhook.delivery_failed
    message_template: |
      DCC-MCP $event
      DCC: $dcc-type
      Tool: $tool-slug
      URL: $url
```

`message_template` 同时支持 <code v-pre>{{source.dcc_type}}</code> envelope path 和 dollar
变量。内置变量包括 `$event`、`$event-id`、`$dcc-type`、`$instance-id`、
`$tool-slug`、`$skill-name` 和 `$url`。

也可以不写 YAML，直接用环境变量启用同一个集成：

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `DCC_MCP_WECOM_WEBHOOK_URL` | 禁用 | 企业微信群机器人 webhook URL |
| `DCC_MCP_WECOM_EVENTS` | `tool.failed, webhook.delivery_failed` | 逗号或换行分隔的事件模式 |
| `DCC_MCP_WECOM_TEMPLATE` | 内置 markdown 模板 | 使用 `$...` 变量的消息正文 |

通过 Admin UI 配置时，企微会保存为共享本地 `webhooks.yaml` 里的
`wecom-message-push` 条目。保存时会保留其他 webhook，只替换已有
`kind: wecom` 或 `name: wecom-message-push` 条目。

---

## ToolRecorder

记录每个 Tool 的执行耗时与成功/失败计数。用于为代码中执行的任意 Action 收集性能遥测数据。

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
| `metrics(action_name)` | `ToolMetrics \| None` | 指定 Tool 的聚合指标；无数据时返回 `None` |
| `all_metrics()` | `list[ToolMetrics]` | 所有已记录 Tool 的聚合指标 |
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

单个 Tool 的性能指标只读快照。通过 `ToolRecorder.metrics()` 或 `ToolRecorder.all_metrics()` 获取。

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `action_name` | `str` | 该指标所属的 Tool 名称 |
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
