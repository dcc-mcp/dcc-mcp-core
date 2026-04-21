# Workflows

> First-class, spec-driven, persistable, cancellable pipelines of MCP tool
> calls. The crate is currently in the **skeleton** stage — parsing and
> validation are complete, step execution lands in a follow-up PR
> (issue [#348](https://github.com/loonghao/dcc-mcp-core/issues/348)).

## What is a workflow?

A workflow is a YAML document that declares an ordered tree of **steps**.
Each step is either a `tool` call, a `tool_remote` call via the gateway,
or one of the control-flow kinds (`foreach`, `parallel`, `branch`,
`approve`). The top-level spec is parsed by
[`WorkflowSpec::from_yaml`](../api/workflows.md) and validated by
`WorkflowSpec::validate()`.

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

See the examples in `examples/workflows/` (to be added alongside the
executor PR) for full end-to-end demos.

## Step policies (issue #353)

Every step may declare an optional **policy** block that controls how the
executor runs it. All fields are optional; omitting the block yields a
default `StepPolicy` (no timeout, no retry, no idempotency key).

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
```

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

- **Reference check at parse time.** Every `{{var}}` root identifier
  must resolve to either a workflow input, one of the well-known roots
  (`inputs`, `steps`, `item`, `env`), or a step id declared anywhere in
  the tree. Unknown roots produce a
  `ValidationError::UnknownTemplateVar` during `WorkflowSpec::validate`.
- **Scope.** Default `workflow` — keys are unique within a single
  workflow invocation. Set `idempotency_scope: global` to make the key
  unique across every workflow invocation (use this for idempotency
  against a downstream service like an asset-tracking DB).

Persistent idempotency tracking across server restarts is tied to the
SQLite persistence work in issue #328 and is out of scope for #353.

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
| `InvalidPolicy` (template parse)  | `idempotency_key` contains a malformed `{{...}}` segment.                     |

All three surface as `ValueError` on the Python side with the offending
step id in the message.

## Runtime enforcement

**Not yet implemented.** The types + parser + helpers in this PR are
consumed by the forthcoming executor in issue #348. Until that ships,
`workflows.run` returns `{"success": false, "error": "not_implemented"}`
deterministically — callers can depend on that shape.
