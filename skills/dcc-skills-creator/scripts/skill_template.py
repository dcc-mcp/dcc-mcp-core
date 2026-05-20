"""Return a SKILL.md template using the current dcc-mcp-core layout."""

from __future__ import annotations

SKILL_TEMPLATE = """---
name: my-skill
description: "Describe what this skill does and when an AI agent should use it. Keep under 1024 characters."
license: MIT
compatibility: "Python 3.7+; Maya 2022+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    layer: thin-harness
    tags: ["modeling", "animation", "example"]
    search-hint: "keywords, comma, separated, for, search"
    tools: tools.yaml
---

# My Skill

Write detailed instructions for the AI agent here.

## Usage

1. Load the skill
2. Call the tools
3. Handle results
"""


def skill_template() -> str:
    """Return a current SKILL.md template.

    Tool declarations live in the sibling ``tools.yaml`` referenced by
    ``metadata.dcc-mcp.tools``. Skill dependencies live in
    ``metadata/depends.md`` when needed.
    """
    return SKILL_TEMPLATE


if __name__ == "__main__":
    print(skill_template())
