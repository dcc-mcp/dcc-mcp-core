# Workflow API

> **状态**: 完全实现 (issue #348)。基于 spec 的流水线引擎，
> 包含六种步骤类型、步骤级策略、制品传递、取消级联和 SQLite 持久化。
>
> 概念指南请参见 [`docs/guide/workflows.md`](../guide/workflows.md)。

## Crate 布局

- **`dcc-mcp-workflow`** — 所有工作流类型、目录、DDL、工具注册
  以及 `WorkflowExecutor` 引擎。在工作区级别通过顶层 `workflow`
  feature 门控（默认关闭）。
- **`dcc-mcp-http`** — `McpHttpConfig::enable_workflows` 在 `start()` 时
  门控内置工具的注册。

## 类型 (Rust)

```rust
use dcc_mcp_workflow::{
    WorkflowSpec, WorkflowStatus, WorkflowJob, WorkflowProgress,
    Step, StepKind, StepId, WorkflowId,
    WorkflowCatalog, WorkflowSummary,
    WorkflowExecutor, WorkflowHost, WorkflowRunHandle,
    register_builtin_workflow_tools, register_workflow_handlers,
    WorkflowError,
};
```

所有结构类型都是 `Serialize + Deserialize + Clone`。ID 是 newtype
(`WorkflowId(Uuid)`, `StepId(String)`)，带有透明 serde。

### `WorkflowSpec`

```rust
let spec = WorkflowSpec::from_yaml(yaml_source)?;
spec.validate()?;
```

验证检查：

- 至少一个步骤。
- 每个步骤 id 非空且在完整树中唯一。
- 每个 `tool` / `tool_remote` 名称通过 `dcc_mcp_naming::validate_tool_name`。
- 每个 `branch.on` 和 `foreach.items` 表达式在 `jsonpath-rust 1.x` 下解析。
- 步骤策略格式正确 (`max_attempts >= 1`,
  `initial_delay_ms <= max_delay_ms`, `timeout_secs > 0` 等)。

### `WorkflowExecutor`

```rust
let handle = WorkflowExecutor::run(spec, inputs, parent_job_id)?;
```

执行流水线：

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

### 步骤类型

| 类型 | 驱动器 | 关键策略开关 |
| ---- | ------ | ---------- |
| `tool` | `ToolCaller::call(name, args)` | timeout, retry, idempotency_key |
| `tool_remote` | `RemoteCaller::call(dcc, name, args)` | 同上 |
| `foreach` | JSONPath → 每项 body, 并发 >= 1 | 子 body 继承策略 |
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

### `WorkflowJob`

```rust
let mut job = WorkflowJob::pending(spec);
job.start()?;   // 通过 WorkflowExecutor 开始执行
```

### `WorkflowCatalog`

读取 `SkillMetadata.metadata["dcc-mcp.workflows"]` 作为 glob（或
逗号分隔的 glob 列表），相对于技能根目录解析。
将完整 YAML body 解析为 `WorkflowSummary`。

```rust
use dcc_mcp_workflow::WorkflowCatalog;

let catalog = WorkflowCatalog::from_skill(&skill_meta, &skill_root)?;
for s in catalog.entries() {
    println!("{}/{}: {}", s.skill, s.name, s.description);
}
```

元数据键 (`dcc-mcp.workflows`) 在 issue #348 的修正案下以
`dcc-mcp.*` 为命名空间 — 它故意**不**引入新的顶级 SKILL.md 字段，
因此 `skills-ref validate` 保持通过。

## 步骤策略 (issue #353)

每一步都可声明一个可选的 `policy` 块。所有字段都是可选的；
省略该块则使用默认的 `StepPolicy`（无超时、无重试、无幂等键）。

```yaml
steps:
  - id: export_fbx
    tool: maya_animation__export_fbx
    args: { scene: "{{inputs.scene_id}}" }
    timeout_secs: 300
    retry:
      max_attempts: 3
      backoff: exponential
      initial_delay_ms: 500
      max_delay_ms: 10000
      jitter: 0.25
      retry_on: ["transient", "timeout"]
    idempotency_key: "export_{{scene_id}}_{{frame_range}}"
    idempotency_scope: workflow
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `timeout_secs` | `u64 > 0` | 无 | 每次尝试的 wall-clock 截止时间。 |
| `retry.max_attempts` | `u32 >= 1` | 如果存在 retry 则必填 | `1` = 无重试。 |
| `retry.backoff` | 枚举 | `exponential` | `fixed` / `linear` / `exponential`。 |
| `retry.initial_delay_ms` | `u64` | `500` | `<= max_delay_ms`。 |
| `retry.max_delay_ms` | `u64` | `10_000` | shape + jitter 后的上限。 |
| `retry.jitter` | `f32` | `0.0` | 限制到 `[0.0, 1.0]`。 |
| `retry.retry_on` | `[String]` | 所有错误 | 错误类型白名单。 |
| `idempotency_key` | string | 无 | 执行前渲染的 Mustache 模板。 |
| `idempotency_scope` | 枚举 | `workflow` | `workflow` 或 `global`。 |

退避公式: `min(base(n), max_delay) * (1 + rand(-jitter, +jitter))`
其中 `base` 对 `fixed` 是 `initial_delay`，对 `linear` 是
`initial_delay * (n-1)`，对 `exponential` 是
`initial_delay * 2^(n-2)`。

尝试编号从 1 开始：`attempt_number == 1` 是首次运行
（无前置延迟）；`attempt_number == 2` 是第一次重试。

工作流的取消会**中断休眠** — 重试永远不会比 `workflows.cancel`
调用活得更久。每次尝试都被记录为工作流根作业下的一个独立子作业
（parent-job id 来自 issue #318）。

## 内置 MCP 工具

由 `register_builtin_workflow_tools(&registry)` 注册。功能型 handler
由 `register_workflow_handlers(&dispatcher, &host)` 绑定。

| 工具 | 说明 | ToolAnnotations |
|------|------|-----------------|
| `workflows.run` | 启动运行（YAML 或 JSON spec + inputs）。 | `destructive_hint=true, open_world_hint=true` |
| `workflows.get_status` | 轮询终止状态 + 进度。 | `read_only_hint=true, idempotent_hint=true` |
| `workflows.cancel` | 通过 `workflow_id` 取消运行（级联）。 | `destructive_hint=true, idempotent_hint=true` |
| `workflows.lookup` | 目录搜索（只读）。 | `read_only_hint=true` |

## Python 接口

```python
from dcc_mcp_core import (
    WorkflowSpec, WorkflowStep, StepPolicy,
    RetryPolicy, BackoffKind, WorkflowStatus,
)

spec = WorkflowSpec.from_yaml_str(yaml_source)
spec.validate()            # 失败时抛出 ValueError

step: WorkflowStep = spec.steps[0]
policy: StepPolicy = step.policy
retry: RetryPolicy = policy.retry
assert retry.next_delay_ms(2) == 500       # 第一次重试延迟（未加 jitter）
```

所有策略类都是 **frozen** — Python 无法修改已解析的 spec。
要运行工作流，请从 MCP 客户端侧调用 MCP 工具
(`workflows.run` / `workflows.get_status` / `workflows.cancel`) —
它们注册在任何调用了 `register_builtin_workflow_tools` 加上
`register_workflow_handlers` 的技能服务器上。

## 审批门控

```yaml
steps:
  - id: human_gate
    kind: approve
    prompt: "继续执行 vendor drop？"
    timeout_secs: 300
```

执行器暂停工作流并发出 `$/dcc.workflowUpdated`，其
`detail.kind == "approve_requested"` 并附带提示文本。MCP 服务器将
入站的 `notifications/$/dcc.approveResponse` 消息桥接到
`ApprovalGate::resolve`。超时时门控以
`approved=false, reason="timeout"` 解析，步骤失败。

## 制品传递 (issue #349)

输出中包含 `file_refs` 数组的工具会被自动通过
`ArtefactStore::put` 捕获；生成的 `FileRef` URI 会出现在下游步骤
上下文中，通过 <code v-pre>`{{steps.<id>.file_refs[<i>].uri}}`</code> 访问。
原始 JSON 输出仍可通过 <code v-pre>`{{steps.<id>.output.*}}`</code> 访问。

## 持久化 (#328)

在 `job-persist-sqlite` feature flag 下，每次工作流运行写入两张表：

- `workflows(workflow_id, root_job_id, spec_json, inputs_json, status,
  current_step_id, step_outputs_json, created_at, completed_at)`
- `workflow_steps(workflow_id, step_id, status, attempt, result_json,
  updated_at)` — 每次转换一行。

启动时，`WorkflowExecutor::recover_persisted()` 将所有非终止行
翻转为 `interrupted` 并发出最终的 `$/dcc.workflowUpdated`。
运行**不会**自动恢复 — `interrupted` 是终止状态；客户端可以
在其上实现一个恢复工具（如果需要）。

## HTTP 服务器门控

```python
from dcc_mcp_core import McpHttpConfig
cfg = McpHttpConfig(port=8765)
cfg.enable_workflows = True     # 默认 False
```

## 发现模型

工作流是 `SKILL.md` 旁边的**兄弟 YAML 文件**，通过单个 `metadata` glob
指向：

```yaml
# SKILL.md (agentskills.io-valid)
---
name: vendor-intake
description: "导入供应商 Maya 文件、运行 QC、导出 FBX、传递给 Unreal。"
metadata:
  dcc-mcp.workflows: "workflows/*.workflow.yaml"
  dcc-mcp.workflows.search-hint: "vendor intake, nightly cleanup, batch import"
---
```

这使 SKILL.md 保持小巧且可组合 — 完整原理参见 issue #348 上的
修正案注释（渐进式披露、可 diff、可重用）。
