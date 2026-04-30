---
name: dcc-skills-creator
description: >-
  Infrastructure skill — create, validate, and scaffold DCC skills for the
  dcc-mcp-core ecosystem. Use when you need to create a new skill, validate
  an existing skill directory, or generate a SKILL.md template. Not for
  executing DCC commands — use domain-specific skills for that.
license: MIT
compatibility: "Python 3.7+, dcc-mcp-core 0.14.3+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    version: "1.0.0"
    layer: infrastructure
    search-hint: "create skill, validate skill, scaffold skill, SKILL.md, skill template, skill format"
    tools: tools.yaml
---

# DCC Skills Creator

A first-class meta-skill for creating and validating DCC skills in the dcc-mcp-core ecosystem.

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
from dcc_mcp_core import create_skill

# Scaffold a minimal skill
create_skill("my-awesome-skill", "/path/to/skills/dir")

# Scaffold with DCC target
create_skill("maya-rigging", "/path/to/skills/dir", dcc="maya")
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
from dcc_mcp_core import skill_template

print(skill_template())  # Full template with all fields
```

## Skill Directory Structure

```
my-skill/
├── SKILL.md              # Required: metadata frontmatter + instructions
├── scripts/              # Optional: tool implementation scripts
│   ├── tool1.py
│   └── tool2.py
└── metadata/             # Optional: documentation and dependencies
    ├── depends.md
    └── help.md
```

## Validation Rules

The validator checks:

- **SKILL.md** exists and is readable
- **YAML frontmatter** is well-formed
- **Required fields**: `name`, `description`
- **Name format**: kebab-case, ≤64 chars, matches directory name
- **Field lengths**: description ≤1024, compatibility ≤500
- **Tool declarations**: non-empty names, no duplicates, snake_case format
- **Script files**: `source_file` references exist in `scripts/`
- **Sidecar files**: `metadata.dcc-mcp.tools/groups/prompts` references exist
- **Dependencies**: `depends` vs `metadata/depends.md` consistency
- **Legacy fields**: top-level extension keys (info-level notice)
