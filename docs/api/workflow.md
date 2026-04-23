# Workflow API

> **Status**: fully implemented (issue #348). Spec-driven pipeline engine
> with six step kinds, step-level policies, artefact hand-off,
> cancellation cascade, and SQLite persistence.
>
> For the conceptual guide see [`docs/guide/workflows.md`](../guide/workflows.md).

## Crate layout

- **`dcc-mcp-workflow`** — all workflow types, catalog, DDL, tool
  registrations, and the `WorkflowExecutor` engine. Feature-gated at the
  workspace level behind the top-level `workflow` feature (off by default).
- **`dcc-mcp-http`** — `McpHttpConfig::enable_workflows` gates registration
  of the built-in tools on `start()`.

## Types (Rust)

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

All structural types are `Serialize + Deserialize + Clone`. IDs are
newtypes (`WorkflowId(Uuid)`, `StepId(String)`) with transparent serde.

### `WorkflowSpec`

```rust
let spec = WorkflowSpec::from_yaml(yaml_source)?;
spec.validate()?;
```

Validation checks:

- At least one step.
- Every step id is non-empty and unique across the full tree.
- Every `tool` / `tool_remote` name passes `dcc_mcp_naming::validate_tool_name`.
- Every `branch.on` and `foreach.items` expression parses under
  `jsonpath-rust 1.x`.
- Step policies are well-formed (`max_attempts >= 1`,
  `initial_delay_ms <= max_delay_ms`, `timeout_secs > 0`, etc.).

### `WorkflowExecutor`

```rust
let handle = WorkflowExecutor::run(spec, inputs, parent_job_id)?;
```

Execution pipeline:

```text
WorkflowExecutor::run(spec, inputs, parent)
   │
   ├─ validates spec
   ├─ creates root job + CancellationToken
   ├─ spawns driver task
   │     │
   │     ├─ drive(steps) ── sequential
   │     │     └─ for each step:
   │     │           ├─ policy: retry + timeout + idempotency
   │     │           ├─ dispatch by StepKind
   │     │           │     ├─ Tool      → ToolCaller::call
   │     │           │     ├─ ToolRemote→ RemoteCaller::call
   │     │           │     ├─ Foreach   → drive(body) per item
   │     │           │     ├─ Parallel  → tokio::join! branches
   │     │           │     ├─ Approve   → ApprovalGate::wait_handle
   │     │           │     └─ Branch    → JSONPath → then|else
   │     │           ├─ artefact handoff (FileRef → ArtefactStore)
   │     │           ├─ SSE: $/dcc.workflowUpdated enter / exit
   │     │           └─ sqlite upsert (if feature enabled)
   │     └─ emit workflow_terminal
   └─ returns WorkflowRunHandle { workflow_id, root_job_id, cancel_token, join }
```

### Step kinds

| Kind | Driver | Key policy knobs |
|------|--------|------------------|
| `tool` | `ToolCaller::call(name, args)` | timeout, retry, idempotency_key |
| `tool_remote` | `RemoteCaller::call(dcc, name, args)` | same |
| `foreach` | JSONPath → body per item, concurrency >= 1 | per-body policy inherited |
| `parallel` | `tokio::join!` over branches | `on_any_fail: abort \| continue` |
| `approve` | `ApprovalGate::wait_handle` + timeout | timeout_secs |
| `branch` | JSONPath condition → `then` or `else` | n/a |

### Cancellation cascade

The root `CancellationToken` is handed to every step driver and every
caller. On `cancel`:

1. No new steps start.
2. In-flight `ToolCaller` / `RemoteCaller` receive the token and should
   honour it cooperatively.
3. Sleeps (retry backoff, `Approve` timeout) are aborted via
   `tokio::select!`.
4. Workflow status becomes `cancelled`; a final `$/dcc.workflowUpdated`
   fires.

Round-trip from `WorkflowHost::cancel` → every in-flight step observing
the token is bounded by one cooperative checkpoint (typically < 200 ms).

### `WorkflowJob`

```rust
let mut job = WorkflowJob::pending(spec);
job.start()?;   // begins execution via WorkflowExecutor
```

### `WorkflowCatalog`

Reads `SkillMetadata.metadata["dcc-mcp.workflows"]` as a glob (or
comma-separated list of globs) resolved relative to the skill root.
Parses the full YAML body into a `WorkflowSummary`.

```rust
use dcc_mcp_workflow::WorkflowCatalog;

let catalog = WorkflowCatalog::from_skill(&skill_meta, &skill_root)?;
for s in catalog.entries() {
    println!("{}/{}: {}", s.skill, s.name, s.description);
}
```

The metadata key (`dcc-mcp.workflows`) is namespaced under `dcc-mcp.*`
per the amendment on issue #348 — it deliberately does **not** introduce a
new top-level SKILL.md field, so `skills-ref validate` stays green.

## Step policies (issue #353)

Every step may declare an optional `policy` block. All fields are optional;
omitting the block yields a default `StepPolicy` (no timeout, no retry, no
idempotency key).

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

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `timeout_secs` | `u64 > 0` | none | Per-attempt wall-clock deadline. |
| `retry.max_attempts` | `u32 >= 1` | required if retry present | `1` = no retry. |
| `retry.backoff` | enum | `exponential` | `fixed` / `linear` / `exponential`. |
| `retry.initial_delay_ms` | `u64` | `500` | `<= max_delay_ms`. |
| `retry.max_delay_ms` | `u64` | `10_000` | Upper clamp after shape + jitter. |
| `retry.jitter` | `f32` | `0.0` | Clamped to `[0.0, 1.0]`. |
| `retry.retry_on` | `[String]` | all errors | Error-kind allowlist. |
| `idempotency_key` | string | none | Mustache template rendered before execution. |
| `idempotency_scope` | enum | `workflow` | `workflow` or `global`. |

Backoff formula: `min(base(attempt), max_delay) * (1 + rand(-jitter, +jitter))`
where `base` is `initial_delay` (fixed), `initial_delay * (n-1)` (linear),
or `initial_delay * 2^(n-2)` (exponential).

Attempt numbering is 1-indexed: `attempt_number == 1` is the initial run
(no pre-delay); `attempt_number == 2` is the first retry.

Cancellation of the enclosing workflow **interrupts the sleep** — retries
never outlive a `workflows.cancel` call. Each attempt is recorded as a
separate child job under the workflow's root job (parent-job id from
issue #318).

## Built-in MCP tools

Registered by `register_builtin_workflow_tools(&registry)`. Functional
handlers are bound by `register_workflow_handlers(&dispatcher, &host)`.

| Tool | Description | ToolAnnotations |
|------|-------------|-----------------|
| `workflows.run` | Start a run (YAML or JSON spec + inputs). | `destructive_hint=true, open_world_hint=true` |
| `workflows.get_status` | Poll terminal status + progress. | `read_only_hint=true, idempotent_hint=true` |
| `workflows.cancel` | Cancel a run by `workflow_id` (cascade). | `destructive_hint=true, idempotent_hint=true` |
| `workflows.lookup` | Catalog search (read-only). | `read_only_hint=true` |

## Python surface

```python
from dcc_mcp_core import (
    WorkflowSpec, WorkflowStep, StepPolicy,
    RetryPolicy, BackoffKind, WorkflowStatus,
)

spec = WorkflowSpec.from_yaml_str(yaml_source)
spec.validate()            # raises ValueError on failure

step: WorkflowStep = spec.steps[0]
policy: StepPolicy = step.policy
retry: RetryPolicy = policy.retry
assert retry.next_delay_ms(2) == 500       # first retry delay (unjittered)
```

All policy classes are **frozen** — Python cannot mutate a parsed spec.
To run workflows, call the MCP tools (`workflows.run` /
`workflows.get_status` / `workflows.cancel`) from the MCP client side —
they are registered on any skill server that calls
`register_builtin_workflow_tools` plus `register_workflow_handlers`.

## Approval gating

```yaml
steps:
  - id: human_gate
    kind: approve
    prompt: "Proceed with vendor drop?"
    timeout_secs: 300
```

The executor pauses the workflow and emits a `$/dcc.workflowUpdated`
with `detail.kind == "approve_requested"` and the prompt. The MCP
server bridges inbound `notifications/$/dcc.approveResponse` messages
into `ApprovalGate::resolve`. On timeout the gate resolves with
`approved=false, reason="timeout"` and the step fails.

## Artefact hand-off (issue #349)

A tool whose output contains a `file_refs` array is automatically
captured via `ArtefactStore::put`; the resulting `FileRef` URIs appear
in the downstream step context as
<code v-pre>`{{steps.<id>.file_refs[<i>].uri}}`</code>. The raw JSON output is still
accessible via <code v-pre>`{{steps.<id>.output.*}}`</code>.

## Persistence (#328)

With the `job-persist-sqlite` feature flag, each workflow run writes to
two tables:

- `workflows(workflow_id, root_job_id, spec_json, inputs_json, status,
  current_step_id, step_outputs_json, created_at, completed_at)`
- `workflow_steps(workflow_id, step_id, status, attempt, result_json,
  updated_at)` — one row per step per transition.

On startup, `WorkflowExecutor::recover_persisted()` flips every
non-terminal row to `interrupted` and emits a final
`$/dcc.workflowUpdated`. Runs are **not** auto-resumed —
`interrupted` is terminal; clients may implement a resume tool on top
if desired.

## HTTP server gate

```python
from dcc_mcp_core import McpHttpConfig
cfg = McpHttpConfig(port=8765)
cfg.enable_workflows = True     # default False
```

## Discovery model

Workflows are **sibling YAML files** next to `SKILL.md`, pointed at via a
single `metadata` glob:

```yaml
# SKILL.md (agentskills.io-valid)
---
name: vendor-intake
description: "Import vendor Maya files, run QC, export FBX, hand off to Unreal."
metadata:
  dcc-mcp.workflows: "workflows/*.workflow.yaml"
  dcc-mcp.workflows.search-hint: "vendor intake, nightly cleanup, batch import"
---
```

This keeps SKILL.md tiny and composable — see the amendment comment on
issue #348 for the full rationale (progressive disclosure, diffability,
reusability).
