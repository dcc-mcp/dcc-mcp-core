---
name: build
description: >-
  Task-oriented domain skill — scaffold new skills, validate existing ones,
  build deployable packages, and publish to marketplace. Use this skill when
  creating, updating, or deploying DCC-MCP skill packages. Composes
  dcc-mcp-skills-creator, marketplace-create-extension, and
  marketplace-publish-extension tools into single-call build workflows.
  Not for running or operating DCC tools — use verify/debug/ui/asset for
  runtime operations.
license: MIT
allowed-tools: ["Bash", "Read", "Write", "Edit"]
metadata:
  dcc-mcp:
    dcc: python
    layer: domain
    version: "1.0.0"
    compatibility: "Python 3.7+, dcc-mcp-core 0.19+"
    tags: [build, scaffold, validate, package, publish, create]
    search-hint: >-
      build skill, create skill, scaffold skill, validate skill, package skill,
      publish skill, deploy skill, marketplace publish, create extension,
      skill template, new skill, build package
    search-aliases: [build, create, scaffold, validate, package, publish, deploy, new skill]
    intent: "Scaffold, validate, build, and publish DCC-MCP skill packages to marketplace."
    recall-context:
      app_type: python
      domain: build
      workflow_stage: development
      task_category: mutation
    preconditions:
      - dcc-mcp-cli on PATH or Python fallback available
    side-effects:
      creates: true
      modifies: true
      file_output: true
      targets: [skill_directory, package_file]
    produces: [skill_package, validation_report]
    requires: [dcc-mcp, dcc-mcp-skills-creator]
    tools: tools.yaml
    depends: [dcc-mcp-skills-creator]
---

# Build — Task-Oriented Skill Development

> **Domain skill**: Use this to create, validate, and publish DCC-MCP skills.

Build composes `dcc-mcp-skills-creator` and marketplace tools into single-call
development workflows. Use it to go from idea to published skill without
manually chaining CLI commands.

## When to use

| Scenario | Tool |
|----------|------|
| Create a new skill from template | `scaffold` — generate SKILL.md + tools.yaml + scripts/ |
| Validate an existing skill | `validate` — run full validation suite |
| Build a deployable package | `package` — run tests + create marketplace bundle |
| Publish to marketplace | `publish` — validate + package + upload |
| Check what's installed | `list_installed` — list all locally installed skills |

## When NOT to use

- **Running DCC operations** — use verify, debug, ui, or asset skills
- **Creating a full adapter repository** — use `dcc-mcp-creator` skill instead
- **Marketplace search/install** — use `dcc-mcp` skill's marketplace commands

## Usage

**Prerequisites**: `dcc-mcp` and `dcc-mcp-skills-creator` skills loaded. No live
DCC instance required — build operates on filesystem skill directories.

### MCP-native agent (IDE)

```
search_skills("build")          → find this skill
load_skill("build")             → load tools into namespace
call("build__scaffold", {"name": "maya-rigging", "dcc": "maya", "tool_name": "create_rig"})
call("build__validate", {"skill_path": "./skills/maya-rigging"})
call("build__publish", {"skill_path": "./skills/maya-rigging", "dry_run": true})
```

### Shell/CLI agent

```bash
dcc-mcp-cli search-skills --query build
dcc-mcp-cli load-skill build
dcc-mcp-cli call <instance>.build__scaffold --json '{"name":"maya-rigging","dcc":"maya"}'
dcc-mcp-cli call <instance>.build__list_installed --json '{}'
```

### Availability

Ships with `dcc-mcp-core` wheel. No live DCC required. `publish` is destructive —
obtain user consent before calling with `dry_run: false`.

## Development workflow

```
User idea → scaffold  (generate skeleton)
         → implement  (fill in scripts + references)
         → validate   (run validation suite)
         → package    (build deployable bundle)
         → publish    (upload to marketplace)
         → list_installed  (verify installation)
```

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `scaffold` | Create | Generate a new skill skeleton from template |
| `validate` | Quality | Run validation suite on a skill directory |
| `package` | Build | Build a deployable marketplace package |
| `publish` | Deploy | Validate, package, and publish to marketplace |
| `list_installed` | Inventory | List locally installed skills and their versions |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md) — copy-pasteable build sequences
- **Exploring skills**: [INTROSPECTION.md](references/INTROSPECTION.md) — how to query installed skills
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md) — common build/validation failures
