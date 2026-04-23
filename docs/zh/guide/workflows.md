# 工作流 (Workflows)

> 一等公民、基于 spec 的、可持久化、可取消的 MCP 工具调用流水线。
> 解析器、验证器以及完整的步骤执行引擎均已随 issue
> [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) 落地。

## 什么是工作流？

工作流是一个 YAML 文档，声明了一棵有序的**步骤**树。每一步要么是
`tool` 调用，要么是通过网关的 `tool_remote` 调用，要么是控制流类型
（`foreach`、`parallel`、`branch`、`approve`）。顶层 spec 由
[`WorkflowSpec::from_yaml`](../api/workflow) 解析并由
`WorkflowSpec::validate()` 验证。

::: v-pre
```yaml
name: vendor_intake
description: "导入供应商 Maya 文件、质检、导出 FBX、推送至 Unreal。"
inputs:
  date: { type: string, format: date }
steps:
  - id: list
    tool: vendor_intake__list_sftp
    args: { date: "{{inputs.date}}" }
  - id: per_file
    kind: foreach
    items: "$.list.files"
    as: file
    steps:
      - id: export
        tool: maya__export_fbx
```
:::

完整的端到端示例请见 `examples/workflows/`（随 executor PR 一并添加）。

## 步骤策略 (issue #353)

每一步都可声明一个可选的 **policy** 块来控制执行器如何运行它。
所有字段都是可选的；省略该块则使用默认的 `StepPolicy`
（无超时、无重试、无幂等键）。

::: v-pre
```yaml
steps:
  - id: export_fbx
    tool: maya_animation__export_fbx
    args: { scene: "{{inputs.scene_id}}" }
    timeout_secs: 300
    retry:
      max_attempts: 3
      backoff: exponential          # "fixed" | "linear" | "exponential"
      initial_delay_ms: 500
      max_delay_ms: 10000
      jitter: 0.25                  # 相对值，限制在 [0.0, 1.0]
      retry_on: ["transient", "timeout"]
    idempotency_key: "export_{{scene_id}}_{{frame_range}}"
    idempotency_scope: workflow     # 或 "global"（默认值: "workflow"）
```
:::

### `timeout_secs`

该步骤**单次尝试**的绝对 wall-clock 截止时间。必须 `> 0`。
当截止时执行器会取消该步骤，并且（如果设置了 `retry` 且失败类型
可重试）将其计为一次失败尝试。`None` = 无超时。

### `retry`

| 字段 | 类型 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `max_attempts` | `u32 >= 1` | *必填* | `1` = 不重试。 |
| `backoff` | 枚举 | `exponential` | `fixed` / `linear` / `exponential`。 |
| `initial_delay_ms` | `u64` | `500` | 必须 `<= max_delay_ms`。 |
| `max_delay_ms` | `u64` | `10_000` | 在 shape + jitter 之后应用的上限。 |
| `jitter` | `f32` | `0.0` | 在解析时限制到 `[0.0, 1.0]`；超出范围会告警。 |
| `retry_on` | `[String]` | *所有错误* | 错误类型白名单。`None` = 所有错误均可重试。 |

执行器在两次尝试之间休眠
`min(base(attempt), max_delay) * (1 + rand(-jitter, +jitter))`，
其中 `base` 由 `backoff` 选择的 shape 决定。Rust 层面的辅助函数
`RetryPolicy::next_delay(attempt_number)` 返回未加 jitter 的基准值，
是公式的唯一来源。

尝试编号从 1 开始：`attempt_number == 1` 是首次运行（无前置延迟）；
`attempt_number == 2` 是第一次重试。

| Backoff | 第 `n >= 2` 次尝试的延迟 |
| ------- | ------------------------ |
| `fixed` | `initial_delay` |
| `linear` | `initial_delay * (n - 1)` |
| `exponential` | `initial_delay * 2^(n - 2)` |

工作流的取消会**中断休眠** — 重试永远不会比 `workflows.cancel`
调用活得更久。每次尝试都被记录为工作流根作业下的一个独立子作业
（parent-job id 来自 issue #318）。

### `idempotency_key`

Mustache 风格的模板，在步骤执行前针对步骤上下文渲染。
执行器会在 `JobManager` 中查找是否存在已完成的作业，其
(`step.tool`, `rendered_key`, `scope`) 匹配；如果找到，则直接返回
之前的结果并跳过该步骤。

- **解析时引用检查。** 每个 <code v-pre>`{{var}}`</code> 的根标识符必须解析为
  工作流输入、某个已知根（`inputs`、`steps`、`item`、`env`）或树中
  任意位置声明的步骤 id。未知根会在 `WorkflowSpec::validate` 时产生
  `ValidationError::UnknownTemplateVar`。
- **作用域。** 默认 `workflow` — 键在单次工作流调用内唯一。
  设置 `idempotency_scope: global` 可使键在每次工作流调用之间都唯一
  （用于针对下游服务如资产追踪 DB 的幂等性）。

跨服务器重启的持久化幂等追踪与 issue #328 的 SQLite 持久化工作
绑定，对 #353 来说超出范围。

## Python 接口

```python
from dcc_mcp_core import (
    BackoffKind,
    RetryPolicy,
    StepPolicy,
    WorkflowSpec,
    WorkflowStep,
)

spec = WorkflowSpec.from_yaml_str(yaml_text)
spec.validate()

step: WorkflowStep = spec.steps[0]
policy: StepPolicy = step.policy
assert policy.timeout_secs == 300
retry: RetryPolicy = policy.retry
assert retry.max_attempts == 3
assert retry.backoff == BackoffKind.EXPONENTIAL
assert retry.next_delay_ms(2) == 500       # 第一次重试延迟
```

所有策略类都是 **frozen** — Python 无法修改已解析的 spec。
要更改任何内容，请重新解析 YAML。

## 验证错误

| 错误变体 | 触发条件 |
| -------- | -------- |
| `InvalidPolicy` | `max_attempts == 0`、`initial_delay_ms > max_delay_ms`、`timeout_secs == 0`。 |
| `UnknownTemplateVar` | `idempotency_key` 引用了已知集合之外的标识符。 |
| `InvalidPolicy` (template parse) | `idempotency_key` 包含格式错误的 <code v-pre>`{{...}}`</code> 段。 |

以上三种在 Python 侧都以 `ValueError` 形式出现，消息中包含出错的步骤 id。

## 执行引擎 (issue #348)

`WorkflowExecutor` 是一个由 Tokio 驱动的引擎，消费一个经过验证的
`WorkflowSpec` 并端到端运行每一种步骤类型。它与传输层无关：
本地工具调用通过 `ToolCaller`，远程调用通过 `RemoteCaller`，
通知通过 `WorkflowNotifier`。

```text
WorkflowExecutor::run(spec, inputs, parent)
   │
   ├─ 验证 spec
   ├─ 创建根作业 + CancellationToken
   ├─ 生成驱动任务
   │     │
   │     ├─ drive(steps) ── 顺序执行
   │     │     └─ 对每个步骤:
   │     │           ├─ policy: retry + timeout + idempotency
   │     │           ├─ 按 StepKind 分发
   │     │           │     ├─ Tool      → ToolCaller::call
   │     │           │     ├─ ToolRemote→ RemoteCaller::call
   │     │           │     ├─ Foreach   → 每项驱动(body)
   │     │           │     ├─ Parallel  → tokio::join! 分支
   │     │           │     ├─ Approve   → ApprovalGate::wait_handle
   │     │           │     └─ Branch    → JSONPath → then|else
   │     │           ├─ artefact 传递 (FileRef → ArtefactStore)
   │     │           ├─ SSE: $/dcc.workflowUpdated enter / exit
   │     │           └─ sqlite upsert (如果启用了 feature)
   │     └─ 发出 workflow_terminal
   └─ 返回 WorkflowRunHandle { workflow_id, root_job_id, cancel_token, join }
```

### 步骤类型一览

| 类型 | 驱动器 | 关键策略开关 |
| ---- | ------ | ---------- |
| `tool` | `ToolCaller::call(name, args)` | timeout, retry, idempotency_key |
| `tool_remote` | `RemoteCaller::call(dcc, name, args)` | 同上 |
| `foreach` | JSONPath → 每项 body, 并发>=1 | 子 body 继承策略 |
| `parallel` | `tokio::join!` 分支 | `on_any_fail: abort \| continue` |
| `approve` | `ApprovalGate::wait_handle` + timeout | timeout_secs |
| `branch` | JSONPath 条件 → `then` 或 `else` | 无 |

### 取消级联

根 `CancellationToken` 传递给每个步骤驱动器和每个调用器。
调用 `cancel` 时：

1. 不再启动新步骤。
2. 进行中的 `ToolCaller` / `RemoteCaller` 收到 token 并应协作地遵守它。
3. 休眠（重试退避、`Approve` 超时）通过 `tokio::select!` 中止。
4. 工作流状态变为 `cancelled`；发出最终的 `$/dcc.workflowUpdated`。

从 `WorkflowHost::cancel` → 每个进行中的步骤观察到 token 的往返
时间被限制在一个协作检查点之内（通常 < 200 ms）。

### Artefact 传递 (#349)

输出中包含 `file_refs` 数组的工具会被自动通过
`ArtefactStore::put` 捕获；生成的 `FileRef` URI 会出现在下游步骤
上下文中，通过 <code v-pre>`{{steps.<id>.file_refs[<i>].uri}}`</code> 访问。
原始 JSON 输出仍可通过 <code v-pre>`{{steps.<id>.output.*}}`</code> 访问。

### 持久化 (#328)

在 `job-persist-sqlite` feature flag 下，每次工作流运行写入两张表：

- `workflows(workflow_id, root_job_id, spec_json, inputs_json, status,
  current_step_id, step_outputs_json, created_at, completed_at)`
- `workflow_steps(workflow_id, step_id, status, attempt, result_json,
  updated_at)` — 每次转换一行。

启动时，`WorkflowExecutor::recover_persisted()` 将所有非终止行
翻转为 `interrupted` 并发出最终的 `$/dcc.workflowUpdated`。
运行**不会**自动恢复 — `interrupted` 是终止状态；如需恢复，
客户端可以在其上实现一个恢复工具。

### 内置 MCP 工具

由 `register_builtin_workflow_tools(&registry)` 注册。功能型 handler
由 `register_workflow_handlers(&dispatcher, &host)` 绑定。

| 工具 | 说明 | ToolAnnotations |
| ---- | ---- | --------------- |
| `workflows.run` | 启动一次运行（YAML 或 JSON spec + inputs）。 | `destructive_hint=true, open_world_hint=true` |
| `workflows.get_status` | 轮询终止状态 + 进度。 | `read_only_hint=true, idempotent_hint=true` |
| `workflows.cancel` | 通过 `workflow_id` 取消运行（级联）。 | `destructive_hint=true, idempotent_hint=true` |
| `workflows.lookup` | 目录搜索（只读）。 | `read_only_hint=true` |

### 审批门控

```yaml
steps:
  - id: human_gate
    kind: approve
    prompt: "继续执行 vendor drop？"
    timeout_secs: 300          # 可选 — 默认无限期
```

执行器暂停工作流并发出 `$/dcc.workflowUpdated`，其
`detail.kind == "approve_requested"` 并附带提示文本。MCP 服务器将
入站的 `notifications/$/dcc.approveResponse` 消息桥接到
`ApprovalGate::resolve`。超时时门控以
`approved=false, reason="timeout"` 解析，步骤失败。

### 用于运行的 Python 接口

目前 Python 层仅暴露 spec + policy 查看器。要运行工作流，请从
MCP 客户端侧调用 MCP 工具（`workflows.run` / `workflows.get_status`
/ `workflows.cancel`）— 它们注册在任何调用了
`register_builtin_workflow_tools` 加上
`register_workflow_handlers` 的技能服务器上。原生的 `WorkflowHost`
Python 类作为后续跟进追踪；MCP 工具路径是推荐的入口点，
因为它与 agent 工具链的其余部分组合良好。
