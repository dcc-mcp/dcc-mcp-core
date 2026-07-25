"""scaffold — Generate a new DCC-MCP skill skeleton."""
from __future__ import annotations

import os
from typing import Any


SKILL_MD_TEMPLATE = """---
name: {name}
description: >-
  {description}
license: MIT
metadata:
  dcc-mcp:
    dcc: {dcc}
    layer: {layer}
    version: "0.1.0"
    compatibility: "{compatibility}"
    tags: {tags}
    search-hint: "{search_hint}"
    tools: tools.yaml
---

# {title}

> **{layer_title} skill**: {description}

## When to use

| Scenario | Tool |
|----------|------|
| Example task | `{tool_name}` |

## Tools

| Tool | Category | Description |
|------|----------|-------------|
| `{tool_name}` | Action | Example tool — replace with real description |

## Progressive disclosure

- **Quick start**: [RECIPES.md](references/RECIPES.md)
- **Exploring**: [INTROSPECTION.md](references/INTROSPECTION.md)
- **Troubleshooting**: [ERRORS.md](references/ERRORS.md)
"""

TOOLS_YAML_TEMPLATE = """tools:
  - name: {tool_name}
    description: "Example tool — replace with a real description of what this tool does."
    input_schema:
      type: object
      properties:
        example_param:
          type: string
          description: "Example parameter — replace with real inputs."
    read_only: {read_only}
    destructive: false
    idempotent: {idempotent}
    execution: sync
    affinity: {affinity}
    source_file: scripts/{tool_name}.py
    annotations:
      read_only_hint: {read_only_lower}
      destructive_hint: false
      idempotent_hint: {idempotent_lower}
      open_world_hint: true
"""

SCRIPT_TEMPLATE = '''"""{tool_name} — Short description of what this tool does."""'
from __future__ import annotations

from typing import Any


def {tool_name}(example_param: str = "") -> dict[str, Any]:
    """Tool entry point.

    Args:
        example_param: Example parameter description.

    Returns:
        Result dictionary with success status.
    """
    if not example_param:
        return {{
            "success": False,
            "error": "example_param is required",
        }}

    # TODO: implement the actual tool logic here

    return {{
        "success": True,
        "message": "Tool executed successfully.",
    }}
'''


def _generate_search_hint(name: str, tool_name: str) -> str:
    """Generate a reasonable search-hint from the skill name."""
    words = name.replace("-", " ")
    tool_words = tool_name.replace("_", " ")
    return "{} {}".format(words, tool_words)


def _generate_description(name: str, dcc: str, tool_name: str) -> str:
    """Generate a default description."""
    words = name.replace("-", " ").title()
    tool_words = tool_name.replace("_", " ")
    return "Domain skill for {dcc}: {words} — {tool_words}.".format(
        dcc=dcc.upper() if dcc != "python" else "cross-DCC",
        words=words,
        tool_words=tool_words,
    )


def scaffold(
    name: str,
    dcc: str | None = None,
    layer: str = "domain",
    description: str | None = None,
    parent_dir: str | None = None,
    tool_name: str | None = None,
    affinity: str = "any",
    tags: list[str] | None = None,
    compatibility: str = "Python 3.7+, dcc-mcp-core 0.19+",
) -> dict[str, Any]:
    """Generate a new skill skeleton.

    Args:
        name: Skill name (kebab-case).
        dcc: Target DCC ('maya', 'blender', 'python', etc.).
        layer: Skill layer.
        description: Skill description.
        parent_dir: Parent directory.
        tool_name: First tool name (snake_case).
        affinity: Thread affinity.
        tags: Skill tags.
        compatibility: Compatibility string.

    Returns:
        Scaffold result with created file paths.
    """
    dcc = dcc or "python"
    tool_name = tool_name or name.replace("-", "_") + "__example"
    description = description or _generate_description(name, dcc, tool_name)
    parent_dir = parent_dir or os.getcwd()
    tags = tags or [layer, dcc]
    search_hint = _generate_search_hint(name, tool_name)

    skill_dir = os.path.join(parent_dir, name)
    scripts_dir = os.path.join(skill_dir, "scripts")
    refs_dir = os.path.join(skill_dir, "references")

    created: list[str] = []

    # Create directories
    for d in (skill_dir, scripts_dir, refs_dir):
        os.makedirs(d, exist_ok=True)

    # SKILL.md
    skill_md_path = os.path.join(skill_dir, "SKILL.md")
    title = name.replace("-", " ").title()
    layer_map = {
        "domain": "Domain",
        "infrastructure": "Infrastructure",
        "thin-harness": "Thin-Harness",
        "example": "Example",
    }
    layer_title = layer_map.get(layer, "Domain")
    read_only = affinity == "any"

    skill_md_content = SKILL_MD_TEMPLATE.format(
        name=name,
        description=description,
        dcc=dcc,
        layer=layer,
        layer_title=layer_title,
        compatibility=compatibility,
        tags=tags,
        search_hint=search_hint,
        title=title,
        tool_name=tool_name,
    )
    with open(skill_md_path, "w", encoding="utf-8") as f:
        f.write(skill_md_content)
    created.append(skill_md_path)

    # tools.yaml
    tools_yaml_path = os.path.join(skill_dir, "tools.yaml")
    tools_yaml_content = TOOLS_YAML_TEMPLATE.format(
        tool_name=tool_name,
        read_only="true" if read_only else "false",
        read_only_lower="true" if read_only else "false",
        idempotent="true" if read_only else "false",
        idempotent_lower="true" if read_only else "false",
        affinity=affinity,
    )
    with open(tools_yaml_path, "w", encoding="utf-8") as f:
        f.write(tools_yaml_content)
    created.append(tools_yaml_path)

    # Script
    script_path = os.path.join(scripts_dir, tool_name + ".py")
    script_content = SCRIPT_TEMPLATE.format(tool_name=tool_name)
    with open(script_path, "w", encoding="utf-8") as f:
        f.write(script_content)
    created.append(script_path)

    # Reference stubs
    ref_templates = {
        "RECIPES.md": "# {title} Recipes\n\nCopy-pasteable workflows for common {name} tasks.\n".format(
            title=title, name=name),
        "INTROSPECTION.md": "# {title} — Introspection\n\nHow to explore and discover {name} state.\n".format(
            title=title, name=name),
        "ERRORS.md": "# {title} — Common Errors\n\nDiagnostic patterns for {name} failures.\n".format(
            title=title, name=name),
    }
    for ref_name, ref_content in ref_templates.items():
        ref_path = os.path.join(refs_dir, ref_name)
        with open(ref_path, "w", encoding="utf-8") as f:
            f.write(ref_content)
        created.append(ref_path)

    next_steps = [
        "1. Edit {skill_dir}/SKILL.md — update description, search-hint, and tags".format(skill_dir=skill_dir),
        "2. Implement the tool logic in scripts/{tool_name}.py".format(tool_name=tool_name),
        "3. Update tools.yaml with real input_schema and annotations",
        "4. Run build__validate to check compliance",
        "5. Test with dcc-mcp-cli reload-skills && search",
    ]

    return {
        "success": True,
        "skill_path": skill_dir,
        "created_files": created,
        "next_steps": next_steps,
    }
