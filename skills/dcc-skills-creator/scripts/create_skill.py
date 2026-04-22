"""Scaffold a new DCC skill directory."""

from __future__ import annotations

from pathlib import Path
import sys


def create_skill(name: str, parent_dir: str, dcc: str = "python") -> str:
    """Create a new skill directory with the standard structure.

    Args:
        name: Skill name in kebab-case.
        parent_dir: Directory where the skill folder will be created.
        dcc: Target DCC (e.g. "maya", "blender", "houdini", "python").

    Returns:
        Absolute path to the created skill directory.

    """
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
tags: []
dcc: {dcc}
version: "0.1.0"
search-hint: ""
tools:
  - name: example_tool
    description: "TODO: describe this tool"
    source_file: scripts/example_tool.py
---

# {name.replace("-", " ").title()}

TODO: Write skill instructions here.
""",
        encoding="utf-8",
    )

    example_script = skill_dir / "scripts" / "example_tool.py"
    example_script.write_text(
        '"""Example tool implementation."""\n'
        "from dcc_mcp_core.skill import skill_entry, skill_success\n\n\n"
        "def main():\n"
        "    args = skill_entry()\n"
        "    # TODO: implement tool logic\n"
        '    return skill_success(result={"message": "Hello from example_tool!"})\n\n\n'
        'if __name__ == "__main__":\n'
        "    print(main())\n",
        encoding="utf-8",
    )

    return str(skill_dir)


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python create_skill.py <name> <parent_dir> [dcc]")
        sys.exit(1)
    path = create_skill(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "python")
    print(f"Created skill at: {path}")
