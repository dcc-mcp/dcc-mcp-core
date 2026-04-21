# Workflow API (skeleton)

> **Status — skeleton.** The types, YAML parser, validator, catalog glob
> reader and the four `workflows.*` built-in MCP tools ship here. **Step
> execution is intentionally deferred to a follow-up PR** — the three
> execution-facing tools return a stable `"step execution pending follow-up
> PR"` error so downstream callers (#349 / #351 / #353 / #354) can build
> against final type signatures in parallel. See
> [issue #348](https://github.com/loonghao/dcc-mcp-core/issues/348).

## Crate layout

- **`dcc-mcp-workflow`** (new) — all workflow types, catalog, DDL, tool
  registrations. Feature-gated at the workspace level behind the top-level
  `workflow` feature (off by default for one release).
- **`dcc-mcp-http`** — `McpHttpConfig::enable_workflows` gates registration
  of the built-in tools on `start()`.

## Types (Rust)

```rust
use dcc_mcp_workflow::{
    WorkflowSpec, WorkflowStatus, WorkflowJob, WorkflowProgress,
    Step, StepKind, StepId, WorkflowId,
    WorkflowCatalog, WorkflowSummary,
    register_builtin_workflow_tools,
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

### `WorkflowJob` (placeholder)

```rust
let mut job = WorkflowJob::pending(spec);
assert!(matches!(job.start(), Err(WorkflowError::NotImplemented(_))));
```

The signature is final so #349 / #353 can build against it; the body is
deliberately a no-op until the execution PR.

### `WorkflowCatalog`

Reads `SkillMetadata.metadata["dcc-mcp.workflows"]` as a glob (or
comma-separated list of globs) resolved relative to the skill root.
Parses only the YAML **header** (`name`, `description`, `inputs`) per file
into a `WorkflowSummary`. Full-body parse is deferred — marked
`TODO(#348-full)` in the source.

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

## Built-in MCP tools

`register_builtin_workflow_tools(&registry)` inserts four entries:

| Tool                      | Status     | Behaviour                                              |
|---------------------------|------------|--------------------------------------------------------|
| `workflows.run`           | stub       | Validates the spec; returns `"step execution pending"` |
| `workflows.get_status`    | stub       | Returns `"step execution pending"`                     |
| `workflows.cancel`        | stub       | Returns `"step execution pending"`                     |
| `workflows.lookup`        | functional | Read-only catalog search                               |

All four names pass `dcc_mcp_naming::validate_tool_name` (SEP-986).

## Python surface (skeleton)

Only `WorkflowSpec` and `WorkflowStatus` are Python-visible. Build the
wheel with the `workflow` feature to get them:

```python
from dcc_mcp_core import WorkflowSpec, WorkflowStatus

spec = WorkflowSpec.from_yaml_str(yaml_source)
spec.validate()            # raises ValueError on failure
print(spec.name, spec.step_count)

s = WorkflowStatus("running")
assert not s.is_terminal
```

If the wheel was built without the `workflow` feature, both symbols
import as `None` from `dcc_mcp_core`.

## Persistence DDL (`job-persist-sqlite` feature)

`dcc_mcp_workflow::sqlite::apply_migrations(&conn)` creates two tables:

- `workflows(id TEXT PK, name, status, spec_json, current_step_id, started_at, completed_at, created_at)`
- `workflow_steps(workflow_id FK, step_id, status, result_json, started_at, completed_at, PRIMARY KEY(workflow_id, step_id))`

No writer is wired in this skeleton — the tables exist but nothing
populates them. The execution PR will wire `WorkflowJob` progression into
these tables for crash recovery (see `WorkflowStatus::Interrupted`).

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
