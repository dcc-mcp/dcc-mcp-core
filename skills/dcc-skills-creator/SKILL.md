---
name: dcc-skills-creator
description: >-
  Compatibility skill - legacy entrypoint for creating, validating, and
  scaffolding DCC-MCP skills. Prefer dcc-mcp-skills-creator for new work.
  Not for creating full adapter repositories - use dcc-mcp-creator.
license: MIT
compatibility: "Python 3.7+, dcc-mcp-core 0.17+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    version: "1.0.0"
    layer: infrastructure
    search-hint: "legacy skill creator, create skill, validate skill, scaffold skill, SKILL.md, tools.yaml"
    tools: tools.yaml
---

# DCC Skills Creator

Compatibility entrypoint. Prefer `dcc-mcp-skills-creator` for new work; it
combines this scaffold/validation tooling with the DCC-MCP skill authoring
workflow.

A first-class meta-skill for creating and validating DCC skills in the dcc-mcp-core ecosystem.
It emits the current `metadata.dcc-mcp.*` layout, sibling `tools.yaml`, explicit
schemas, annotations, execution mode, and thread-affinity metadata.

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
# dcc_skills_creator__create_skill(
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
# dcc_skills_creator__skill_template()
```

## Skill Directory Structure

```
my-skill/
├── SKILL.md              # Required: metadata frontmatter + instructions
├── tools.yaml            # Required when metadata.dcc-mcp.tools points here
├── scripts/              # Optional: tool implementation scripts
│   └── create_locator.py
└── metadata/             # Optional: documentation and dependencies
    ├── depends.md
    └── help.md
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

## Python Script Helpers

Generated Python scripts should import standard result helpers from
`dcc_mcp_core.skills_helper` when the full wheel is available:

```python
from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success
```

Use the same namespace for dependency-light JSON/YAML codecs, bounded HTTP
requests, file/path safety, hashing, compression, schema validation, argument
normalization, and cancellation checks. Keep `requests`, PyYAML, or domain
libraries only when the helper namespace does not cover the behavior, such as
sessions, streaming, multipart upload, custom auth/retry flows, YAML comment
preservation, or host SDKs.

## Validation Rules

The validator checks:

- **SKILL.md** exists and is readable
- **YAML frontmatter** is well-formed
- **Required fields**: `name`, `description`
- **Name format**: kebab-case, ≤64 chars, matches directory name
- **Field lengths**: description ≤1024, compatibility ≤500
- **Tool declarations**: non-empty names, no duplicates, snake_case client-safe format
- **Script files**: `source_file` references exist in `scripts/`
- **Sidecar files**: `metadata.dcc-mcp.tools/groups/prompts` references exist
- **Dependencies**: `depends` vs `metadata/depends.md` consistency
- **Spec compliance**: non-standard top-level keys are frontmatter errors; dcc-mcp-core extensions must live under `metadata.dcc-mcp.*` and point to sibling files
