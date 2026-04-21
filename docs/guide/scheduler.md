# Scheduler — cron + webhook-triggered workflows

> Issue [#352](https://github.com/loonghao/dcc-mcp-core/issues/352).
> Opt-in via the Cargo `scheduler` feature. Off by default.

The scheduler subsystem fires pre-registered workflows (`WorkflowSpec` from
[#348](https://github.com/loonghao/dcc-mcp-core/issues/348)) on two kinds of
triggers:

- **Cron** — a next-fire-time loop on a `chrono-tz` timezone, optional
  uniform random jitter.
- **Webhook** — an HTTP POST endpoint on the main Axum router, optional
  HMAC-SHA256 validation via `X-Hub-Signature-256`.

The scheduler **does not execute workflows itself**. On fire it builds a
`TriggerFire` value and hands it to a caller-supplied `JobSink`. The sink
resolves the workflow name against the `WorkflowCatalog` and enqueues a
`WorkflowJob` through whatever dispatch path the host prefers.

## Sibling-file pattern ([#356](https://github.com/loonghao/dcc-mcp-core/issues/356))

Schedules live in `*.schedules.yaml` files alongside `SKILL.md`, never
embedded in the `SKILL.md` frontmatter itself. A skill points at them via
`metadata.dcc-mcp.workflow.schedules`:

```yaml
# SKILL.md
---
name: scene-maintenance
description: Nightly cleanup + upload validation for Maya scenes.
metadata:
  dcc-mcp:
    workflow:
      specs: [workflows.yaml]
      schedules: [schedules.yaml]
---
```

```yaml
# schedules.yaml (sibling of SKILL.md)
schedules:
  - id: nightly_cleanup
    workflow: scene_cleanup          # WorkflowSpec id
    inputs:
      scope: all-scenes
    trigger:
      kind: cron
      expression: "0 0 3 * * *"      # sec min hour day month weekday
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

### Cron expression format

The underlying [`cron`](https://crates.io/crates/cron) crate expects the
6-field form `sec min hour day_of_month month day_of_week` (seconds are
**required**). A classic 5-field expression like `"0 3 * * *"` will fail
to parse — use `"0 0 3 * * *"` for "every day at 03:00".

### Template variables

Webhook payloads are merged into workflow inputs via
`{{trigger.payload.<json-path>}}` placeholders:

- `{{trigger.payload.file_path}}` — dotted-path lookup (objects + numeric
  array indices).
- `{{trigger.schedule_id}}` / `{{trigger.workflow}}` — literal context.

A placeholder that is the **entire** string preserves the underlying
JSON type (number stays a number). Placeholders inside a larger string
are always stringified.

## HMAC-SHA256 validation

When `secret_env` is set on a webhook trigger:

1. The server reads the secret from the named env var **at startup**.
2. Each request must carry `X-Hub-Signature-256: sha256=<hex>`; the
   scheduler recomputes the HMAC and compares in constant time.
3. If the env var is set at startup but missing at request time, the
   endpoint replies `500 webhook_secret_missing` (fail-loud).
4. If the signature is wrong, the endpoint replies `401 invalid_signature`.

Use the GitHub convention — any existing webhook sender works without
reconfiguration.

## `max_concurrent` — skip-on-overlap

`max_concurrent` caps the number of in-flight fires per schedule id.
- `max_concurrent = 1` (default) — a fire is skipped if the previous
  invocation has not yet reached a terminal status.
- `max_concurrent = 0` — unlimited.

The host must call `SchedulerHandle::mark_terminal(schedule_id)` when it
observes a terminal workflow status (typically via a subscription to
`$/dcc.workflowUpdated`). The counter is decremented so future fires are
admitted again.

Webhook requests that hit the concurrency cap receive `429 Too Many
Requests` with a JSON body describing the in-flight / max values.

## Runtime surface

```rust
use std::sync::Arc;
use dcc_mcp_scheduler::{
    JobSink, SchedulerConfig, SchedulerService, TriggerFire,
};

struct MySink { /* workflow registry + dispatcher */ }
impl JobSink for MySink {
    fn enqueue(&self, fire: TriggerFire) -> Result<(), String> {
        // resolve fire.workflow, build a WorkflowJob, submit it.
        Ok(())
    }
}

let cfg = SchedulerConfig::from_dir("./schedules")?;
let (handle, webhook_router) = SchedulerService::new(cfg, Arc::new(MySink))
    .start();
// Merge webhook_router into your main Axum app:
//   app = app.merge(webhook_router);
// On terminal workflow status:
//   handle.mark_terminal("nightly_cleanup");
// On shutdown:
//   handle.shutdown();
```

## `McpHttpConfig` integration

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.enable_scheduler = True
cfg.schedules_dir = "/opt/dcc-mcp/schedules"
```

Or via builder:

```rust
use dcc_mcp_http::config::McpHttpConfig;
let cfg = McpHttpConfig::new()
    .with_scheduler("/opt/dcc-mcp/schedules");
```

The config fields are always present; they are no-ops when the
`dcc-mcp-scheduler` crate is not compiled in.

## Python surface

Only the **declarative** types are exposed:

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

# Parse a whole file:
specs = parse_schedules_yaml(open("./schedules.yaml").read())

# HMAC helpers (e.g. for webhook-sender tests):
sig = hmac_sha256_hex(b"shared-secret", request_body)
assert verify_hub_signature_256(b"shared-secret", request_body, sig)
```

The scheduler runtime itself is driven from Rust inside the HTTP server —
Python cannot currently construct a `SchedulerService` directly.

## Non-goals

- Distributed scheduling / leader election (single-node only).
- Hot-reload of schedule files (pick up on server restart).
- Fire-history / last-run UI (future issue).

## See also

- `crates/dcc-mcp-scheduler/src/lib.rs` — crate-level docs and example.
- `docs/proposals/workflow-orchestration-gap.md` §G — design rationale.
- Issue [#352](https://github.com/loonghao/dcc-mcp-core/issues/352).
