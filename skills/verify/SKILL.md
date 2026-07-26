---
name: verify
description: >-
  Task-oriented domain skill — verify DCC instance readiness, health, tool
  availability, and environment correctness. Use this skill first whenever you
  need to confirm a DCC is ready for work before dispatching tasks. Composes
  dcc-mcp CLI doctor/list/health, dcc_diagnostics__process_status, and
  dcc_introspect__* tools into single-call verification workflows. Not for
  diagnosing failures — use the debug skill for root-cause investigation.
license: MIT
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [verify, readiness, health, diagnostics, read-only]
    search-hint: >-
      verify DCC instance, check readiness, health check, is DCC running,
      is tool available, capability check, environment check, preflight,
      instance status, dispatch ready, gateway health, doctor
    search-aliases: [verify instance, check readiness, health check, preflight, capability check, instance status]
    intent: "Verify DCC instance readiness, health, tool availability, and environment correctness before dispatching tasks."
    recall-context:
      app_type: python
      domain: verification
      workflow_stage: preflight
      task_category: query
    preconditions:
      - dcc-mcp-cli on PATH or Python fallback available
    side-effects:
      creates: false
      modifies: false
      file_output: false
      targets: []
    produces: [verification_report]
    requires: [dcc-mcp]
    tools: tools.yaml
    depends: [dcc-mcp]
---

# Verify — Task-Oriented Instance Verification

> **Domain skill**: Start here before dispatching work to any DCC instance.

Verify is a task-oriented domain skill that composes `dcc-mcp` CLI commands and
`dcc_diagnostics__*` tools into single-call verification workflows. Use it to
confirm a DCC instance is ready, healthy, and has the tools you need — before
you send work.

## When to use

| Scenario | Tool |
|----------|------|
| Just connected to a DCC, unsure if it's ready | `verify_instance` |
| Need to check if a specific tool exists | `verify_capability` |
| Environment/config looks wrong | `verify_environment` |
| Full pre-dispatch checklist | `verify_preflight` |
| Check gateway connectivity | `verify_gateway` |

## When NOT to use

- **Diagnosing a failure** — use the `debug` skill instead
- **Interacting with UI** — use the `ui` skill
- **Building/deploying** — use the `build` skill
- **Asset operations** — use the `asset` skill

## Usage

**Prerequisites**: `dcc-mcp` skill loaded, a DCC instance registered and running
(`dcc-mcp-cli list` shows at least one instance).

### MCP-native agent (IDE)

```
search_skills("verify")         → find this skill
load_skill("verify")            → load tools into namespace
call("verify__verify_instance", {"dcc_type": "maya"})
call("verify__verify_preflight", {"capability_query": "create sphere", "dcc_type": "maya"})
```

### Shell/CLI agent

```bash
dcc-mcp-cli search-skills --query verify
dcc-mcp-cli load-skill verify
dcc-mcp-cli call <instance>.verify__verify_instance --json '{"dcc_type":"maya"}'
dcc-mcp-cli call <instance>.verify__verify_preflight --json '{"capability_query":"create sphere","dcc_type":"maya"}'
```

### Availability

These skills ship with the `dcc-mcp-core` wheel. After `pip install dcc-mcp-core`,
they are discovered automatically by `create_skill_server()` when the `skills/`
directory is in the skill path (default for core-managed servers). Adapters can
also reference them via `extra_paths` or `DCC_MCP_SKILL_PATHS`.

## Workflow

```
verify_instance → verify_capability → verify_environment → verify_preflight
      ↑                ↑                     ↑                    ↑
  "Is it alive?"   "Can it do X?"     "Is config right?"   "All clear?"
```

Each tool can be called independently. Use `verify_preflight` for a comprehensive
one-shot check.

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `verify_instance` | Readiness | Check if a DCC instance is dispatch-ready |
| `verify_capability` | Tooling | Check if specific tools/capabilities are available |
| `verify_environment` | Config | Validate environment, Python version, dependencies |
| `verify_gateway` | Connectivity | Check gateway health and profile |
| `verify_preflight` | Composite | Run all checks in one call |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md) — copy-pasteable verification sequences
- **Exploring live state**: [INTROSPECTION.md](references/INTROSPECTION.md) — how to query discovery surfaces
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md) — common verification failures and fixes

## Integration with other skills

- Runs `dcc-mcp-cli list`, `doctor`, `health`, `wait-ready` under the hood
- Uses `dcc_diagnostics__process_status` for process liveness
- Uses `dcc_introspect__search` for capability checks
- Feeds readiness state to `debug`, `ui`, `asset`, and `build` skills
