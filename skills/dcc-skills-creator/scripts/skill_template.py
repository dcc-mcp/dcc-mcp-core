"""Return a SKILL.md template with all supported fields."""

from __future__ import annotations

SKILL_TEMPLATE = """---
name: my-skill
description: "Describe what this skill does and when an AI agent should use it. Keep under 1024 characters."
license: MIT
compatibility: "Python 3.7+; Maya 2022+"
allowed-tools: Bash Read Write Edit
tags: [modeling, animation, example]
dcc: maya
version: "1.0.0"
search-hint: "keywords, comma, separated, for, search"
depends: [other-skill]
tools:
  - name: my_tool
    description: "What this tool does"
    source_file: scripts/my_tool.py
    group: basic
    execution: sync
---

# My Skill

Write detailed instructions for the AI agent here.

## Usage

1. Load the skill
2. Call the tools
3. Handle results
"""


def skill_template() -> str:
    """Return a full SKILL.md template."""
    return SKILL_TEMPLATE


if __name__ == "__main__":
    print(skill_template())
