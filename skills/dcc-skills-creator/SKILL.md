---
name: dcc-skills-creator
description: "Create, validate, and scaffold DCC skills for the dcc-mcp-core ecosystem. Use when you need to create a new skill, validate an existing skill directory, or understand the skill format specification."
license: MIT
compatibility: "Python 3.7+, dcc-mcp-core 0.14.3+"
allowed-tools: Bash Read Write Edit
tags: [skill, scaffold, validate, creator, dcc]
dcc: python
version: "1.0.0"
search-hint: "create skill, validate skill, scaffold skill, SKILL.md, skill template, skill format"
tools:
  - name: create_skill
    description: "Scaffold a new skill directory with SKILL.md, scripts/, and metadata/ structure."
    source_file: scripts/create_skill.py
  - name: validate_skill_dir
    description: "Validate a skill directory against the dcc-mcp-core specification. Returns structured errors, warnings, and info."
    source_file: scripts/validate_skill_dir.py
  - name: skill_template
    description: "Return a SKILL.md template with all supported fields and documentation."
    source_file: scripts/skill_template.py
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
