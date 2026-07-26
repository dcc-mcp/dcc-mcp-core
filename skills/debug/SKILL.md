---
name: debug
description: >-
  Task-oriented domain skill — diagnose DCC tool failures, inspect live state,
  collect structured evidence, and trace execution end-to-end. Use this skill
  whenever a DCC tool returns an error, a workflow stalls, or you need to
  understand what happened. Composes dcc_diagnostics__*,
  dcc_introspect__*, and dcc-mcp-cli stats into single-call diagnosis
  workflows. Not for initial readiness checking — use the verify skill for
  preflight validation.
license: MIT
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [debug, diagnostics, error, troubleshooting, read-only]
    search-hint: >-
      debug DCC error, diagnose failure, error report, tool failed, why did it fail,
      inspect state, trace execution, collect logs, screenshot error, audit log,
      job history, root cause, troubleshoot, what went wrong
    search-aliases: [debug, diagnose, troubleshoot, error report, inspect state, trace tool, root cause]
    intent: "Diagnose DCC tool failures, inspect live state, collect evidence, and trace execution to find root causes."
    recall-context:
      app_type: python
      domain: debugging
      workflow_stage: diagnosis
      task_category: query
    preconditions:
      - dcc-mcp-cli on PATH or dcc_diagnostics tools available
    side-effects:
      creates: false
      modifies: false
      file_output: true
      targets: [screenshot_file]
    produces: [diagnosis_report]
    requires: [dcc-mcp, dcc-diagnostics]
    tools: tools.yaml
    depends: [dcc-mcp, dcc-diagnostics]
---

# Debug — Task-Oriented Error Diagnosis

> **Domain skill**: Start here when anything fails.

Debug composes `dcc_diagnostics__*` tools and `dcc-mcp-cli` commands into
single-call diagnosis workflows. Use it to understand why something failed
before attempting a fix.

## When to use

| Scenario | Tool |
|----------|------|
| Tool returned a vague error | `diagnose` — full diagnostic pipeline |
| Need to inspect live DCC state | `inspect_state` — scene, selection, loaded skills |
| Suspect a specific tool is broken | `trace_tool` — execute and capture full telemetry |
| Need evidence for a bug report | `collect_evidence` — logs + screenshot + metrics bundle |
| Sandbox blocking a legitimate call | `check_sandbox` — audit permissions |

## When NOT to use

- **Checking if DCC is ready** — use the `verify` skill
- **Interacting with UI** — use the `ui` skill
- **Building/deploying** — use the `build` skill

## Usage

**Prerequisites**: `dcc-mcp` skill loaded, a tool call has failed or behavior
is unexpected.

### MCP-native agent (IDE)

```
search_skills("debug")          → find this skill
load_skill("debug")             → load tools into namespace
call("debug__diagnose", {"dcc_name": "maya", "failed_action": "maya_primitives__create_sphere", "error_message": "..."})
call("debug__inspect_state", {"dcc_name": "maya"})
call("debug__trace_tool", {"tool_slug": "maya.xxx.primitives__create_sphere", "arguments": {}})
```

### Shell/CLI agent

```bash
dcc-mcp-cli search-skills --query debug
dcc-mcp-cli load-skill debug
dcc-mcp-cli call <instance>.debug__diagnose --json '{"dcc_name":"maya","failed_action":"maya_primitives__create_sphere"}'
dcc-mcp-cli call <instance>.debug__collect_evidence --json '{"dcc_name":"maya","output_dir":"/tmp/evidence"}'
```

### Availability

Ships with `dcc-mcp-core` wheel. Auto-discovered by `create_skill_server()`.
Depends on `dcc-mcp` and `dcc-diagnostics` skills — both must be loadable
before `debug` tools resolve.

## Diagnostic workflow

```
Tool fails
  → diagnose  (error_report + audit_log + tool_metrics + screenshot)
  → inspect_state  (understand current DCC state)
  → trace_tool  (reproduce with full telemetry)
  → collect_evidence  (bundle for bug report)
  → check_sandbox  (if denied/permission error)
```

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `diagnose` | Pipeline | Run full diagnosis: error report → audit → metrics → screenshot |
| `inspect_state` | Inspection | Snapshot current DCC state (scene, selection, loaded modules) |
| `trace_tool` | Tracing | Execute a tool with full telemetry capture |
| `collect_evidence` | Evidence | Bundle logs, screenshots, metrics, and job history |
| `check_sandbox` | Sandbox | Audit sandbox permissions for denied calls |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md) — copy-pasteable diagnosis sequences
- **Exploring live state**: [INTROSPECTION.md](references/INTROSPECTION.md) — how to query DCC internals
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md) — common failure patterns and fixes
