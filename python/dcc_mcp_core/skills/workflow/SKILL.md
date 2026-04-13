---
name: workflow
description: "Multi-step action orchestration — run a sequence of MCP tools in order, passing results between steps. Enables agents to chain complex operations (select → rename → validate → export) without custom code."
license: MIT
dcc: python
version: "1.0.0"
search-hint: "chain, sequence, pipeline, multi-step, orchestration, workflow, batch, run steps"
tags: [workflow, orchestration, chain, pipeline, automation]
metadata:
  category: workflow
tools:
  - name: run_chain
    description: "Execute a sequence of actions in order via the dcc-mcp-core ActionDispatcher. Each step's output context is merged into the next step's parameters. On failure, the chain stops and reports which step failed and why."
    input_schema:
      type: object
      required: [steps]
      properties:
        steps:
          type: array
          description: "Ordered list of steps to execute."
          items:
            type: object
            required: [action]
            properties:
              action:
                type: string
                description: "Action name (e.g. 'maya_scene__list_objects')."
              params:
                type: object
                description: "Parameters to pass to this action. Values from previous step context can be referenced using '{key}' syntax."
                default: {}
              stop_on_failure:
                type: boolean
                description: "If true (default), abort the chain when this step fails."
                default: true
              label:
                type: string
                description: "Human-readable label for this step (shown in results)."
        context:
          type: object
          description: "Initial context values available to all steps via '{key}' interpolation."
          default: {}
    read_only: false
    idempotent: false
    source_file: scripts/run_chain.py
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
