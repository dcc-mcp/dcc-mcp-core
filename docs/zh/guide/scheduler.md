# 调度器 — cron + webhook 触发的工作流

> Issue [#352](https://github.com/loonghao/dcc-mcp-core/issues/352)。
> 通过 Cargo `scheduler` feature opt-in。默认关闭。

调度器子系统在两种触发器上触发预注册的工作流
（来自 [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) 的 `WorkflowSpec`）：

- **Cron** — 基于 `chrono-tz` 时区的下次触发时间循环，可选均匀随机 jitter。
- **Webhook** — 主 Axum 路由器上的 HTTP POST 端点，可选通过
  `X-Hub-Signature-256` 进行 HMAC-SHA256 验证。

调度器**本身不执行工作流**。触发时它构建一个 `TriggerFire` 值
并将其交给调用者提供的 `JobSink`。sink 针对 `WorkflowCatalog`
解析工作流名称，并通过主机首选的任何分派路径将 `WorkflowJob` 入队。

## 兄弟文件模式 ([#356](https://github.com/loonghao/dcc-mcp-core/issues/356))

调度计划存放在 `SKILL.md` 旁边的 `*.schedules.yaml` 文件中，
从不内联在 `SKILL.md` frontmatter 本身。技能通过
`metadata.dcc-mcp.workflow.schedules` 指向它们：

```yaml
# SKILL.md
---
name: scene-maintenance
description: Maya 场景的夜间清理 + 上传验证。
metadata:
  dcc-mcp:
    workflow:
      specs: [workflows.yaml]
      schedules: [schedules.yaml]
---
```

::: v-pre
```yaml
# schedules.yaml (SKILL.md 的兄弟文件)
schedules:
  - id: nightly_cleanup
    workflow: scene_cleanup          # WorkflowSpec id
    inputs:
      scope: all-scenes
    trigger:
      kind: cron
      expression: "0 0 3 * * *"      # 秒 分 时 日 月 星期
      timezone: UTC
      jitter_secs: 120
    enabled: true
    max_concurrent: 1

  - id: on_upload
    workflow: validate_upload
    inputs:
      path: "{{trigger.payload.file_path}}"
    trigger:
      kind: webhook
      path: /webhooks/upload
      secret_env: UPLOAD_WEBHOOK_SECRET
    enabled: true
```
:::

### Cron 表达式格式

底层 [`cron`](https://crates.io/crates/cron) crate 期望 6 字段形式
`sec min hour day_of_month month day_of_week`（**秒是必需的**）。
经典的 5 字段表达式如 `"0 3 * * *"` 会解析失败 — 使用
`"0 0 3 * * *"` 表示"每天 03:00"。

### 模板变量

Webhook payload 通过 <code v-pre>`{{trigger.payload.<json-path>}}`</code>
占位符合并到工作流输入中：

- <code v-pre>`{{trigger.payload.file_path}}`</code> — 点路径查找（对象 + 数字数组索引）。
- <code v-pre>`{{trigger.schedule_id}}`</code> / <code v-pre>`{{trigger.workflow}}`</code> — 字面量上下文。

作为**整个**字符串的占位符保留底层 JSON 类型（数字保持为数字）。
更大字符串内部的占位符总是被字符串化。

## HMAC-SHA256 验证

当 webhook 触发器上设置了 `secret_env` 时：

1. 服务器在启动时从命名的环境变量读取 secret。
2. 每个请求必须携带 `X-Hub-Signature-256: sha256=<hex>`；调度器
   重新计算 HMAC 并以常数时间比较。
3. 如果环境变量在启动时设置但在请求时缺失，端点回复
   `500 webhook_secret_missing`（fail-loud）。
4. 如果签名错误，端点回复 `401 invalid_signature`。

使用 GitHub 惯例 — 任何现有的 webhook 发送器都无需重新配置即可工作。

## `max_concurrent` — 重叠时跳过

`max_concurrent` 限制每个调度 id 的进行中触发次数。
- `max_concurrent = 1`（默认）— 如果上一次调用尚未达到终止状态，
  则跳过触发。
- `max_concurrent = 0` — 无限制。

主机必须在观察到终止工作流状态时调用
`SchedulerHandle::mark_terminal(schedule_id)`（通常通过订阅
`$/dcc.workflowUpdated`）。计数器递减，以便未来的触发再次被允许。

达到并发上限的 webhook 请求会收到 `429 Too Many Requests`，
以及描述进行中 / 最大值的 JSON body。

## 运行时接口

```rust
use std::sync::Arc;
use dcc_mcp_scheduler::{
    JobSink, SchedulerConfig, SchedulerService, TriggerFire,
};

struct MySink { /* workflow registry + dispatcher */ }
impl JobSink for MySink {
    fn enqueue(&self, fire: TriggerFire) -> Result<(), String> {
        // 解析 fire.workflow，构建 WorkflowJob，提交。
        Ok(())
    }
}

let cfg = SchedulerConfig::from_dir("./schedules")?;
let (handle, webhook_router) = SchedulerService::new(cfg, Arc::new(MySink))
    .start();
// 将 webhook_router 合并到你的主 Axum app 中:
//   app = app.merge(webhook_router);
// 在终止工作流状态时:
//   handle.mark_terminal("nightly_cleanup");
// 关闭时:
//   handle.shutdown();
```

## `McpHttpConfig` 集成

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.enable_scheduler = True
cfg.schedules_dir = "/opt/dcc-mcp/schedules"
```

或通过 builder：

```rust
use dcc_mcp_http::config::McpHttpConfig;
let cfg = McpHttpConfig::new()
    .with_scheduler("/opt/dcc-mcp/schedules");
```

配置字段始终存在；当 `dcc-mcp-scheduler` crate 未编译进来时
它们是 no-op。

## Python 接口

仅暴露**声明式**类型：

```python
from dcc_mcp_core import (
    ScheduleSpec, TriggerSpec,
    parse_schedules_yaml,
    hmac_sha256_hex, verify_hub_signature_256,
)

spec = ScheduleSpec(
    id="nightly_cleanup",
    workflow="scene_cleanup",
    trigger=TriggerSpec.cron("0 0 3 * * *", timezone="UTC", jitter_secs=120),
    inputs='{"scope": "all-scenes"}',
    max_concurrent=1,
)
spec.validate()

# 解析整个文件:
specs = parse_schedules_yaml(open("./schedules.yaml").read())

# HMAC 辅助函数（例如用于 webhook-sender 测试）:
sig = hmac_sha256_hex(b"shared-secret", request_body)
assert verify_hub_signature_256(b"shared-secret", request_body, sig)
```

调度器运行时本身在 HTTP 服务器内部由 Rust 驱动 —
Python 目前无法直接构造 `SchedulerService`。

## 非目标

- 分布式调度 / leader 选举（仅单节点）。
- 调度文件热重载（在服务器重启时加载）。
- 触发历史 / 上次运行 UI（未来 issue）。

## 参见

- `crates/dcc-mcp-scheduler/src/lib.rs` — crate 级文档和示例。
- `docs/proposals/workflow-orchestration-gap.md` §G — 设计原理。
- Issue [#352](https://github.com/loonghao/dcc-mcp-core/issues/352)。
