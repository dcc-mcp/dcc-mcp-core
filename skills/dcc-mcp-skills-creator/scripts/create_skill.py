"""Scaffold a new DCC skill directory using the current DCC-MCP contract."""

from __future__ import annotations

from pathlib import Path
import re
import sys

_KEBAB_CASE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
_DCC_NAME = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
_TOOL_NAME = re.compile(r"^[a-z0-9]+(?:_[a-z0-9]+)*$")
_AFFINITIES = {"any", "main"}
_EXECUTION_MODES = {"sync", "async"}


def _validate_skill_name(name: str) -> None:
    if not _KEBAB_CASE.fullmatch(name):
        raise ValueError("Skill name must be kebab-case, e.g. 'maya-rigging-tools'")


def _validate_dcc_name(dcc: str) -> None:
    if not _DCC_NAME.fullmatch(dcc):
        raise ValueError("DCC name must be client-safe: letters, digits, '_' or '-', max 64 chars")


def _validate_tool_name(tool_name: str) -> None:
    if not _TOOL_NAME.fullmatch(tool_name):
        raise ValueError(
            "Tool name must be snake_case and client-safe, e.g. 'create_locator'; dotted names are not supported"
        )


def _validate_choice(value: str, allowed: set, label: str) -> None:
    if value not in allowed:
        expected = ", ".join(sorted(allowed))
        raise ValueError(f"{label} must be one of: {expected}")


def _stage_line(stage: str) -> str:
    return f"    stage: {stage}\n" if stage else ""


def _depends_line(depends) -> str:
    deps = [dep.strip() for dep in (depends or []) if dep and dep.strip()]
    for dep in deps:
        _validate_skill_name(dep)
    if not deps:
        return ""
    quoted = ", ".join(f'"{dep}"' for dep in deps)
    return f"    depends: [{quoted}]\n"


def create_skill(
    name: str,
    parent_dir: str,
    dcc: str = "python",
    *,
    tool_name: str = "example_tool",
    layer: str = "thin-harness",
    stage: str = "",
    depends=None,
    affinity: str = "any",
    execution: str = "sync",
) -> str:
    """Create a new skill directory with the modern standard structure.

    Args:
        name: Skill name in kebab-case.
        parent_dir: Directory where the skill folder will be created.
        dcc: Target DCC (e.g. "maya", "blender", "houdini", "python").
        tool_name: First generated tool name. Must be snake_case, never dotted.
        layer: Skill taxonomy layer, usually thin-harness, infrastructure, domain, or example.
        stage: Optional progressive-loading stage such as scene, authoring, or pipeline.
        depends: Optional iterable of prerequisite DCC-MCP skill names.
        affinity: Tool thread affinity. Use "main" for host API / scene work.
        execution: "sync" or "async".

    Returns:
        Absolute path to the created skill directory.

    """
    dcc = dcc.strip()
    tool_name = tool_name.strip()
    layer = layer.strip() or "thin-harness"
    stage = stage.strip()
    affinity = affinity.strip().lower() or "any"
    execution = execution.strip().lower() or "sync"
    depends_line = _depends_line(depends)

    _validate_skill_name(name)
    _validate_dcc_name(dcc)
    _validate_tool_name(tool_name)
    _validate_choice(affinity, _AFFINITIES, "affinity")
    _validate_choice(execution, _EXECUTION_MODES, "execution")

    skill_dir = Path(parent_dir).expanduser() / name
    if skill_dir.exists():
        raise FileExistsError(f"Skill directory already exists: {skill_dir}")

    skill_dir.mkdir(parents=True)
    (skill_dir / "agents").mkdir()
    (skill_dir / "scripts").mkdir()
    (skill_dir / "references").mkdir()

    title = name.replace("-", " ").title()
    script_file = f"scripts/{tool_name}.py"

    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text(
        f"""---
name: {name}
description: >-
  DCC skill - TODO: describe the user intent this skill serves. Use when an
  agent needs TODO. Not for TODO.
license: MIT
compatibility: "Python 3.7+; dcc-mcp-core 0.17+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: {dcc}
    version: "0.1.0"
    layer: {layer}
{_stage_line(stage)}{depends_line}    tags: ["generated", "{dcc}"]
    search-hint: "TODO: add search keywords for this skill"
    tools: tools.yaml
---

# {title}

Keep instructions focused on when an agent should use this skill, what each
tool returns, prerequisite skills to load first, and what to inspect after
success or failure.

Tool names must stay client-safe (`^[A-Za-z0-9_-]{{1,64}}$`). Use local
snake_case tool names in `tools.yaml`; dcc-mcp-core publishes them as
`{name}__tool_name` after the skill is loaded.

Runtime scripts should import dependency-light helpers from
`dcc_mcp_core.skills_helper` first. It provides JSON/YAML, bounded HTTP, safe
file/path helpers, validation, cancellation checks, and result helpers without
adding small Python dependencies to DCC hosts.
""",
        encoding="utf-8",
    )

    openai_yaml = skill_dir / "agents" / "openai.yaml"
    openai_yaml.write_text(
        f'''interface:
  display_name: "{title}"
  short_description: "Run a structured DCC-MCP workflow"
  default_prompt: "Use ${name} to complete this DCC-MCP workflow."
''',
        encoding="utf-8",
    )

    tools_yaml = skill_dir / "tools.yaml"
    tools_yaml.write_text(
        f"""tools:
  - name: {tool_name}
    description: "Example read-only scaffold. Replace with one concrete DCC intent and describe side effects."
    source_file: {script_file}
    input_schema:
      type: object
      properties:
        label:
          type: string
          description: "Optional label echoed by the scaffold implementation."
        dry_run:
          type: boolean
          description: "Keep true until this scaffold performs real host work."
          default: true
      additionalProperties: false
    output_schema:
      type: object
      properties:
        success:
          type: boolean
        message:
          type: string
        context:
          type: object
    execution: {execution}
    affinity: {affinity}
    enforce_thread_affinity: true
    timeout_hint_secs: 30
    annotations:
      read_only_hint: true
      destructive_hint: false
      idempotent_hint: true
      open_world_hint: false
      deferred_hint: false
    next-tools:
      on-success: []
      on-failure: []
""",
        encoding="utf-8",
    )

    example_script = skill_dir / script_file
    example_script.write_text(
        '"""Example DCC-MCP skill tool implementation."""\n'
        "from __future__ import annotations\n\n"
        "from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success\n\n\n"
        "@skill_entry\n"
        'def main(label: str = "example", dry_run: bool = True, **params):\n'
        '    """Replace this scaffold with a concrete DCC operation."""\n'
        "    return skill_success(\n"
        '        "Example tool completed",\n'
        '        prompt="Replace this scaffold with a concrete DCC operation.",\n'
        "        label=label,\n"
        "        dry_run=dry_run,\n"
        "        extra_params=params,\n"
        "    )\n\n\n"
        'if __name__ == "__main__":\n'
        "    run_main(main)\n",
        encoding="utf-8",
    )

    return str(skill_dir.resolve())


if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python create_skill.py <name> <parent_dir> [dcc]")
        sys.exit(1)
    path = create_skill(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "python")
    print(f"Created skill at: {path}")
