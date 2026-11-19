---
name: workflow
description: >-
  Infrastructure skill — multi-step action orchestration: run a sequence of MCP
  tools in order, passing results between steps. Use when chaining two or more
  tools into a repeatable pipeline (select → rename → validate → export). Not
  for single-tool operations or DCC-specific business logic — use a domain skill
  for those. Repeated multi-step procedures should graduate to persisted
  `workflows_run` / `workflows_resume` runs with stable idempotency keys.
license: MIT
metadata:
  dcc-mcp:
    dcc: python
    recall-context:
      app_type: python
      domain: workflow
      workflow_stage: pipeline
      task_category: automate
    version: "1.0.0"
    layer: infrastructure
    search-hint: "chain, sequence, pipeline, multi-step, orchestration, workflow, batch, run steps, automate"
    tags: "workflow, orchestration, chain, pipeline, automation, infrastructure"
    tools: tools.yaml
---

# Workflow Orchestration

Multi-step action chaining for DCC pipelines.

## Overview

The `workflow__run_chain` tool lets an agent (or a human) execute a sequence
of dcc-mcp-core actions in order. Results from earlier steps flow into later
steps via context merging and `{key}` parameter interpolation.

## Example: Select → Rename → Validate → Export

```json
{
  "steps": [
    {
      "label": "List mesh objects",
      "action": "maya_scene__list_objects",
      "params": {"type": "mesh"}
    },
    {
      "label": "Rename with prefix",
      "action": "maya_scene__rename_objects",
      "params": {"prefix": "char_"}
    },
    {
      "label": "Validate naming",
      "action": "maya_pipeline__validate_naming",
      "params": {}
    },
    {
      "label": "Export FBX",
      "action": "maya_scene__export_fbx",
      "params": {"output": "/tmp/export.fbx"},
      "stop_on_failure": true
    }
  ]
}
```

## Error Recovery

If a step fails and `stop_on_failure` is `true`, the chain halts immediately
and returns the partial results so far, plus the error details. The agent can
then use `dcc_diagnostics__screenshot` or `dcc_diagnostics__audit_log` to
investigate before retrying.

## Context Interpolation

Use `{key}` placeholders in `params` to inject values from the running context:

```json
{"action": "export_fbx", "params": {"output": "{export_path}"}}
```

The context starts from the `context` input, then accumulates each step's
`context` output.

## When to graduate to persisted workflows

`workflow__run_chain` is a lightweight in-process sequence without a persisted
run identity. After the same procedure succeeds twice, use the core MCP tools
`workflows_run`, `workflows_get_status`, and `workflows_resume` instead. Give
mutating steps stable `idempotency_key` values, poll the returned `workflow_id`,
and recover an interrupted run with `workflows_resume` using `workflow_id` plus
optional `force_steps`, `expected_spec_hash`, and `strict`. The resume handler
requires `WorkflowStorage` and the `job-persist-sqlite` feature. Do not replay
completed work or re-send unchanged script source to resume a run.

For single script iterations, materialize once with
`materialize_script(content=..., reuse=True, reuse_key=...)`, execute by the
returned `file_path`, and change values through `params` only. See
`docs/guide/agents-reference.md#iteration-playbook`.
