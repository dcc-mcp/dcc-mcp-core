"""Scaffold a new DCC skill directory."""

from __future__ import annotations

from pathlib import Path
import re
import sys

_KEBAB_CASE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


def create_skill(name: str, parent_dir: str, dcc: str = "python") -> str:
    """Create a new skill directory with the standard structure.

    Args:
        name: Skill name in kebab-case.
        parent_dir: Directory where the skill folder will be created.
        dcc: Target DCC (e.g. "maya", "blender", "houdini", "python").

    Returns:
        Absolute path to the created skill directory.

    """
    if not _KEBAB_CASE.fullmatch(name):
        raise ValueError("Skill name must be kebab-case, e.g. 'maya-rigging-tools'")

    skill_dir = Path(parent_dir) / name
    if skill_dir.exists():
        raise FileExistsError(f"Skill directory already exists: {skill_dir}")

    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    (skill_dir / "metadata").mkdir()

    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text(
        f"""---
name: {name}
description: "TODO: describe what this skill does and when to use it"
license: MIT
compatibility: "Python 3.7+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: {dcc}
    version: "0.1.0"
    layer: thin-harness
    tags: ["generated", "{dcc}"]
    search-hint: "TODO: add search keywords for this skill"
    tools: tools.yaml
---

# {name.replace("-", " ").title()}

TODO: Write skill instructions here.
""",
        encoding="utf-8",
    )

    tools_yaml = skill_dir / "tools.yaml"
    tools_yaml.write_text(
        """tools:
  - name: example_tool
    description: "TODO: describe this tool"
    source_file: scripts/example_tool.py
    execution: sync
    affinity: any
    annotations:
      read_only_hint: true
      destructive_hint: false
      idempotent_hint: true
      open_world_hint: false
""",
        encoding="utf-8",
    )

    example_script = skill_dir / "scripts" / "example_tool.py"
    example_script.write_text(
        '"""Example tool implementation."""\n'
        "from dcc_mcp_core.skill import run_main, skill_entry, skill_success\n\n\n"
        "@skill_entry\n"
        "def example_tool():\n"
        "    # TODO: implement tool logic\n"
        '    return skill_success("Hello from example_tool!")\n\n\n'
        'if __name__ == "__main__":\n'
        "    run_main(example_tool)\n",
        encoding="utf-8",
    )

    return str(skill_dir)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python create_skill.py <name> <parent_dir> [dcc]")
        sys.exit(1)
    path = create_skill(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "python")
    print(f"Created skill at: {path}")
