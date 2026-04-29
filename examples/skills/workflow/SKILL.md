---
name: workflow
description: >-
  Infrastructure skill — multi-step action orchestration: run a sequence of MCP
  tools in order, passing results between steps. Use when chaining two or more
  tools into a repeatable pipeline (select → rename → validate → export). Not
  for single-tool operations or DCC-specific business logic — use a domain skill
  for those.
license: MIT
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: infrastructure
  dcc-mcp.search-hint: "chain, sequence, pipeline, multi-step, orchestration, workflow, batch, run steps, automate"
  dcc-mcp.tags: "workflow, orchestration, chain, pipeline, automation, infrastructure"
  dcc-mcp.tools: tools.yaml
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
