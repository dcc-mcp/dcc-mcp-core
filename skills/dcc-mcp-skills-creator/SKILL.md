---
name: dcc-mcp-skills-creator
description: >-
  Infrastructure skill - create, validate, scaffold, and review DCC-MCP skills
  for the dcc-mcp-core ecosystem. Use when authoring SKILL.md, tools.yaml,
  scripts, groups, prompts, or skill taxonomy. Not for creating a full DCC-MCP
  adapter repository - use dcc-mcp-creator.
license: MIT-0
compatibility: "Python 3.7+, dcc-mcp-core 0.17+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    version: "0.17.49"  # x-release-please-version
    layer: infrastructure
    search-hint: "create dcc mcp skill, validate skill, scaffold skill, SKILL.md, tools.yaml, scripts, groups, prompts, skill taxonomy"
    tools: tools.yaml
    skill-reference-docs:
      - "references/*.md"
  openclaw:
    homepage: https://github.com/loonghao/dcc-mcp-core/blob/main/skills/dcc-mcp-skills-creator/SKILL.md
---

# DCC-MCP Skills Creator

A first-class meta-skill for creating, validating, and reviewing DCC-MCP skill
packages. It bundles scaffold/validation tools together with agent-facing
authoring guidance for `SKILL.md`, `tools.yaml`, scripts, groups, prompts, and
progressive-loading taxonomy.

Use `dcc-mcp-creator` when the task is to create a full adapter repository for
a host such as Nuke, Blender, 3ds Max, Unreal, ZBrush, Houdini, or Maya. Use
this skill when the task is to create or improve the skill packages loaded by
those adapters.

## Installation

This skill ships with dcc-mcp-core. Add it to your skill path:

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="${DCC_MCP_SKILL_PATHS}:$(python -c 'import dcc_mcp_core; print(dcc_mcp_core.__file__)')/../skills"

# Windows
set DCC_MCP_SKILL_PATHS=%DCC_MCP_SKILL_PATHS%;C:\path\to\dcc-mcp-core\skills
```

Or reference it directly when starting your MCP server:

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

server = create_skill_server(
    "maya",
    McpHttpConfig(port=8765),
    extra_paths=["/path/to/dcc-mcp-core/skills"],
)
```

## Quick Start

### Create a new skill

```python
# Call the loaded MCP tool:
# dcc_mcp_skills_creator__create_skill(
#     name="maya-rigging",
#     parent_dir="/path/to/skills/dir",
#     dcc="maya",
#     tool_name="create_locator",
#     affinity="main",
# )
```

### Validate an existing skill

```python
from dcc_mcp_core import validate_skill

report = validate_skill("/path/to/my-skill")
if report.has_errors:
    for issue in report.issues:
        print(f"[{issue.severity}] {issue.category}: {issue.message}")
else:
    print("Skill is valid!")
```

### Get a SKILL.md template

```python
# Call the loaded MCP tool:
# dcc_mcp_skills_creator__skill_template()
```

## Skill Directory Structure

```
my-skill/
|-- SKILL.md              # Required: metadata frontmatter + instructions
|-- tools.yaml            # Required when metadata.dcc-mcp.tools points here
|-- scripts/              # Optional: tool implementation scripts
|   `-- create_locator.py
`-- references/           # Optional: recipes, examples, and long-form docs
    |-- RECIPES.md
    `-- NOTES.md
```

## Current Tool Contract

Generated `tools.yaml` entries follow the modern contract:

- Local tool names are snake_case and client-safe. Do not use dotted names.
- Loaded tools are published as `<skill-name>__<tool_name>` when namespacing is needed.
- `input_schema` and `output_schema` are declared explicitly.
- `execution` is `sync` or `async`; use `async` for deferred/long-running work.
- `affinity` is explicit. Use `main` for host API or scene mutation work and `any` for pure work.
- `enforce_thread_affinity: true` is emitted so adapter dispatch stays honest.
- `annotations` use MCP hints: read-only, destructive, idempotent, open-world, and deferred.

## Authoring Workflow

1. Decide whether the skill is infrastructure, domain, thin-harness, or example.
2. Give the skill a kebab-case name and each local tool a snake_case name.
3. Keep host API calls inside scripts, with lazy imports so discovery works without the host running.
4. Import dependency-light runtime helpers from `dcc_mcp_core.skills_helper` first: JSON/YAML codecs, bounded HTTP helpers, safe file/path helpers, validation, cancellation checks, and result helpers.
5. Declare `execution`, `affinity`, `timeout_hint_secs`, schemas, annotations, and failure recovery chains in `tools.yaml`.
6. Put long examples, recipes, and host-specific notes under `references/`.
7. Validate with `validate_skill_dir` or `dcc_mcp_core.validate_skill()` before loading it in an adapter.
8. If the desired behavior requires parsing core internals or adapter-private YAML at runtime, stop and request a core API instead.

Read [AUTHORING_WORKFLOW.md](references/AUTHORING_WORKFLOW.md) and
[DCC_TOOL_CONTRACTS.md](references/DCC_TOOL_CONTRACTS.md) before changing a
production skill package.

## Validation Rules

The validator checks:

- **SKILL.md** exists and is readable
- **YAML frontmatter** is well-formed
- **Required fields**: `name`, `description`
- **Name format**: kebab-case, <=64 chars, matches directory name
- **Field lengths**: description <=1024, compatibility <=500
- **Tool declarations**: non-empty names, no duplicates, snake_case client-safe format
- **Script files**: `source_file` references exist in `scripts/`
- **Sidecar files**: `metadata.dcc-mcp.tools/groups/prompts` references exist
- **Dependencies**: `metadata.dcc-mcp.depends` consistency
- **Spec compliance**: non-standard top-level keys are frontmatter errors; dcc-mcp-core extensions must live under `metadata.dcc-mcp.*` and point to sibling files
- **Skill helper adoption**: `validate_skill_dir` emits `skill-helper-adoption` warnings when scripts import avoidable dependencies covered by `dcc_mcp_core.skills_helper`, such as `requests`, `httpx`, PyYAML, or local JSON/HTTP/file/path helper modules
