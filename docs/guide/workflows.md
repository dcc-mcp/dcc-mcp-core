# Workflows

> First-class, spec-driven, persistable, cancellable pipelines of MCP tool
> calls. Parsing, validation, and the full step execution engine all ship
> in issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348).

## What is a workflow?

A workflow is a YAML document that declares an ordered tree of **steps**.
Each step is either a `tool` call, a `tool_remote` call via the gateway,
or one of the control-flow kinds (`foreach`, `parallel`, `branch`,
`approve`). The top-level spec is parsed by
[`WorkflowSpec::from_yaml`](../api/workflow) and validated by
`WorkflowSpec::validate()`.

::: v-pre
```yaml
name: vendor_intake
description: "Import vendor Maya files, QC, export FBX, push to Unreal."
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

See the examples in `examples/workflows/` (to be added alongside the
executor PR) for full end-to-end demos.

## Step policies (issue #353)

Every step may declare an optional **policy** block that controls how the
executor runs it. All fields are optional; omitting the block yields a
default `StepPolicy` (no timeout, no retry, no idempotency key).

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
      jitter: 0.25                  # relative, clamped to [0.0, 1.0]
      retry_on: ["transient", "timeout"]
    idempotency_key: "export_{{scene_id}}_{{frame_range}}"
    idempotency_scope: workflow     # or "global" (default: "workflow")
    idempotency_ttl_secs: 86400     # optional; None / 0 = no expiry within scope
```
:::

### `timeout_secs`

Absolute wall-clock deadline for **one attempt** of the step. Must be
`> 0`. When the deadline fires the executor cancels the step and (if
`retry` is set and the failure kind is retryable) counts it as a failed
attempt. `None` = no timeout.

### `retry`

| Field              | Type       | Default        | Notes                                                           |
| ------------------ | ---------- | -------------- | --------------------------------------------------------------- |
| `max_attempts`     | `u32 >= 1` | *required*     | `1` = no retry.                                                 |
| `backoff`          | enum       | `exponential`  | `fixed` / `linear` / `exponential`.                             |
| `initial_delay_ms` | `u64`      | `500`          | Must be `<= max_delay_ms`.                                      |
| `max_delay_ms`     | `u64`      | `10_000`       | Upper clamp applied after shape + jitter.                       |
| `jitter`           | `f32`      | `0.0`          | Clamped to `[0.0, 1.0]` at parse time; out-of-range → warn.     |
| `retry_on`         | `[String]` | *all errors*   | Error-kind allowlist. `None` = every error retryable.           |

The executor sleeps
`min(base(attempt), max_delay) * (1 + rand(-jitter, +jitter))` between
attempts where `base` is the shape selected by `backoff`. The Rust-level
helper `RetryPolicy::next_delay(attempt_number)` returns the unjittered
base value and is the single source of truth for the formulas.

Attempt numbering is 1-indexed: `attempt_number == 1` is the initial
run (no pre-delay); `attempt_number == 2` is the first retry.

| Backoff       | Delay for attempt `n >= 2`     |
| ------------- | ------------------------------ |
| `fixed`       | `initial_delay`                |
| `linear`      | `initial_delay * (n - 1)`      |
| `exponential` | `initial_delay * 2^(n - 2)`    |

Cancellation of the enclosing workflow **interrupts the sleep** — retries
never outlive a `workflows.cancel` call. Each attempt is recorded as a
separate child job under the workflow's root job (parent-job id from
issue #318).

### `idempotency_key`

Mustache-style template rendered against the step context **just before**
execution. The executor consults the JobManager for an existing
completed job with matching (`step.tool`, `rendered_key`, `scope`); if
found, the prior result is returned and the step is skipped.

- **Reference check at parse time.** Every <code v-pre>`{{var}}`</code> root identifier
  must resolve to either a workflow input, one of the well-known roots
  (`inputs`, `steps`, `item`, `env`), or a step id declared anywhere in
  the tree. Unknown roots produce a
  `ValidationError::UnknownTemplateVar` during `WorkflowSpec::validate`.
- **Scope.** Default `workflow` — keys are unique within a single
  workflow invocation. Set `idempotency_scope: global` to make the key
  unique across every workflow invocation (use this for idempotency
  against a downstream service like an asset-tracking DB).
- **TTL.** Optional `idempotency_ttl_secs` bounds how long a cached
  entry survives. Defaults to `None` (and `Some(0)` is normalised to
  `None` so env-var plumbing such as `DCC_MCP_*_IDEMPOTENCY_TTL=0`
  cannot accidentally produce instant-expire rows).

### Persistent idempotency cache (issue #566)

By default, `IdempotencyCache` is process-local — entries die with the
executor. To survive server restarts so a re-run of the same spec
short-circuits steps that were already completed, plug in
`SqliteIdempotencyStore`:

```rust
use dcc_mcp_core::workflow::{
    IdempotencyStore, WorkflowExecutor,
    sqlite::{SqliteIdempotencyStore, WorkflowStorage},
};
use std::sync::Arc;

let storage = Arc::new(WorkflowStorage::open("workflows.db")?);
let executor = WorkflowExecutor::builder()
    .tool_caller(my_caller)
    .storage(Arc::clone(&storage))
    .idempotency_store(SqliteIdempotencyStore::new(storage))
    .build();
```

The store reuses the same SQLite connection pool that backs
`WorkflowStorage`; no second DB file is opened. Workflow-scoped rows
cascade-delete when their owning workflow row is removed (via an
`AFTER DELETE` trigger). Global-scoped rows live until their TTL fires
or until [`IdempotencyStore::purge_expired`] is called explicitly.

## Python surface

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
assert retry.next_delay_ms(2) == 500       # first retry delay
```

All policy classes are **frozen** — Python cannot mutate a parsed spec.
Re-parse the YAML to change anything.

## Validation errors

| Error variant                     | Raised when …                                                                 |
| --------------------------------- | ----------------------------------------------------------------------------- |
| `InvalidPolicy`                   | `max_attempts == 0`, `initial_delay_ms > max_delay_ms`, `timeout_secs == 0`.  |
| `UnknownTemplateVar`              | `idempotency_key` references an identifier outside the known set.             |
| `InvalidPolicy` (template parse)  | `idempotency_key` contains a malformed <code v-pre>`{{...}}`</code> segment.                     |

All three surface as `ValueError` on the Python side with the offending
step id in the message.

## Execution engine (issue #348)

The `WorkflowExecutor` is the Tokio-driven engine that consumes a
validated `WorkflowSpec` and runs every step kind end-to-end. It is
transport-agnostic: local tool calls go through a `ToolCaller`, remote
calls through a `RemoteCaller`, notifications through a `WorkflowNotifier`.

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

### Step kinds at a glance

| Kind          | Driver                                   | Key policy knobs            |
| ------------- | ---------------------------------------- | --------------------------- |
| `tool`        | `ToolCaller::call(name, args)`           | `timeout`, `retry`, `idempotency_key` |
| `tool_remote` | `RemoteCaller::call(dcc, name, args)`    | same                        |
| `foreach`     | JSONPath → body per item, concurrency≥1  | per-body policy inherited   |
| `parallel`    | `tokio::join!` over branches             | `on_any_fail: abort | continue` |
| `approve`     | `ApprovalGate::wait_handle` + timeout    | `timeout_secs`              |
| `branch`      | JSONPath condition → `then` or `else`    | n/a                         |

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

### Artefact handoff (#349)

A tool whose output contains a `file_refs` array is automatically
captured via `ArtefactStore::put`; the resulting `FileRef` URIs appear
in the downstream step context as
<code v-pre>`{{steps.<id>.file_refs[<i>].uri}}`</code>. The raw JSON output is still
accessible via <code v-pre>`{{steps.<id>.output.*}}`</code>.

### Persistence (#328)

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

### Built-in MCP tools

Registered by `register_builtin_workflow_tools(&registry)`. Functional
handlers are bound by `register_workflow_handlers(&dispatcher, &host)`.

| Tool                   | Description                                      | ToolAnnotations                               |
| ---------------------- | ------------------------------------------------ | --------------------------------------------- |
| `workflows.run`        | Start a run (YAML or JSON spec + inputs).        | `destructive_hint=true, open_world_hint=true` |
| `workflows.get_status` | Poll terminal status + progress.                 | `read_only_hint=true, idempotent_hint=true`   |
| `workflows.cancel`     | Cancel a run by `workflow_id` (cascade).         | `destructive_hint=true, idempotent_hint=true` |
| `workflows.lookup`     | Catalog search (read-only).                      | `read_only_hint=true`                         |
| `workflows.resume`     | Resume a persisted run from storage; skips `completed` steps; honours `force_steps` + `expected_spec_hash` (#565). Requires the executor to be built with `WorkflowStorage`. | `destructive_hint=true, idempotent_hint=true, open_world_hint=true` |

### Resume (issue #565)

For long-running workflows that survive a server restart, `workflows.resume`
re-drives the persisted spec from the first non-completed step. The
executor reads the persisted spec + inputs + per-step status from
`WorkflowStorage`, hydrates its context with every recorded
`completed` step's output (so downstream `steps.X.output` Mustache
references stay live), then drives forward — emitting a single
`step_skipped_resume` event per skipped step.

Wire shape:

```jsonc
{
  "workflow_id": "<uuid>",
  "force_steps": ["qc"],                 // optional: re-run even if completed
  "expected_spec_hash": "abc123...",     // optional: caller-asserted hash
  "strict": true                          // optional: refuse on hash mismatch
}
```

`expected_spec_hash` is the SHA-256 hex of the canonical spec JSON;
compute it with `dcc_mcp_core::workflow::sqlite::compute_spec_hash`. With
`strict=false` (default) a hash mismatch logs a `WARN` and proceeds
using the persisted spec. With `strict=true` resume returns
`SpecChanged` and the operator must reconcile the catalog before
retrying. `force_steps` is the "re-export this step after a downstream
correction" knob; it is the only way to re-run a step that already
reached `completed`.

Resume requires that the executor was built with both:

- `WorkflowExecutorBuilder::storage(Arc<WorkflowStorage>)`, and
- the `dcc-mcp-workflow/job-persist-sqlite` Cargo feature.

Without storage, `workflows.resume` returns `NoStorage` immediately.

### Approval gating

```yaml
steps:
  - id: human_gate
    kind: approve
    prompt: "Proceed with vendor drop?"
    timeout_secs: 300          # optional — default is indefinite
```

The executor pauses the workflow and emits a `$/dcc.workflowUpdated`
with `detail.kind == "approve_requested"` and the prompt. The MCP
server bridges inbound `notifications/$/dcc.approveResponse` messages
into `ApprovalGate::resolve`. On timeout the gate resolves with
`approved=false, reason="timeout"` and the step fails.

### Python surface for runs

Today the Python layer exposes the spec + policy viewers only. To run
workflows, call the MCP tools (`workflows.run` / `workflows.get_status`
/ `workflows.cancel`) from the MCP client side — they are registered
on any skill server that calls `register_builtin_workflow_tools` plus
`register_workflow_handlers`. A native `WorkflowHost` Python class is
tracked as a follow-up; the MCP tool path is the recommended entry
point since it composes with the rest of the agent toolbelt.
