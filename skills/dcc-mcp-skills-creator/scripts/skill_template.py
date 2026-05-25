"""Return a SKILL.md template using the current dcc-mcp-core layout."""

from __future__ import annotations

SKILL_TEMPLATE = """---
name: my-skill
description: >-
  DCC skill - Describe the durable user intent this skill serves. Use when an
  AI agent needs that intent. Not for unrelated diagnostics or raw code eval.
license: MIT
compatibility: "Python 3.7+; dcc-mcp-core 0.17+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    layer: thin-harness
    stage: authoring
    tags: ["modeling", "animation", "example"]
    search-hint: "create object, inspect scene, export asset"
    tools: tools.yaml
---

# My Skill

Write concise instructions for the AI agent:

- when to load this skill;
- what each tool changes or reads;
- what to verify after success;
- what diagnostic tool to call after failure.

Tool names must be client-safe (`^[A-Za-z0-9_-]{1,64}$`). In `tools.yaml`, use
local snake_case names such as `create_locator`; dcc-mcp-core publishes loaded
tools as `<skill-name>__<tool_name>`.
"""


def skill_template() -> str:
    """Return a current SKILL.md template.

    Tool declarations live in the sibling ``tools.yaml`` referenced by
    ``metadata.dcc-mcp.tools``. Use the creator scaffold for a full
    SKILL.md + tools.yaml + scripts/ example with schemas, annotations, and
    thread-affinity metadata.
    """
    return SKILL_TEMPLATE


if __name__ == "__main__":
    print(skill_template())
