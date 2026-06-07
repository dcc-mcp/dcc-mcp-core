"""Scaffold a new marketplace extension package.

Creates a directory with the standard dcc-mcp skill layout:
  <name>/
  ├── SKILL.md       # MIT-0 licensed frontmatter + body
  ├── tools.yaml     # Tool declarations with schemas and annotations
  └── scripts/
      └── <action_name>.py

Follows the same patterns as dcc-mcp-skills-creator.
"""

from __future__ import annotations

from pathlib import Path
import re
import sys

_KEBAB_CASE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
_TOOL_NAME = re.compile(r"^[a-z0-9]+(?:_[a-z0-9]+)*$")
_INSTALL_TYPES = {"git", "path", "zip"}


def _validate_skill_name(name: str) -> None:
    if not _KEBAB_CASE.fullmatch(name):
        raise ValueError("Extension name must be kebab-case, e.g. 'my-maya-tools'")


def _validate_tool_name(tool_name: str) -> None:
    if not _TOOL_NAME.fullmatch(tool_name):
        raise ValueError(
            "Action name must be snake_case and client-safe, e.g. 'run_export'; dotted names are not supported"
        )


def create_extension(
    name: str,
    parent_dir: str,
    *,
    description: str = "",
    dcc_targets: list[str] | None = None,
    install_type: str = "git",
    author: str = "",
    version: str = "0.1.0",
    action_name: str = "run",
) -> str:
    """Create a new marketplace extension directory.

    Args:
        name: Extension name in kebab-case.
        parent_dir: Directory where the extension folder will be created.
        description: Human-readable description.
        dcc_targets: Target DCCs (e.g. ["maya", "blender"]). Empty = python.
        install_type: Install source type ("git", "path", or "zip").
        author: Extension author or maintainer name.
        version: Semantic version string.
        action_name: Snake_case name for the placeholder action script.

    Returns:
        Absolute path to the created extension directory.

    """
    dcc_targets = dcc_targets or []
    description = description.strip()
    author = author.strip()
    version = version.strip() or "0.1.0"
    action_name = action_name.strip() or "run"

    _validate_skill_name(name)
    _validate_tool_name(action_name)

    if install_type not in _INSTALL_TYPES:
        raise ValueError(f"install_type must be one of: {', '.join(sorted(_INSTALL_TYPES))}")

    skill_dir = Path(parent_dir).expanduser().resolve() / name
    if skill_dir.exists():
        raise FileExistsError(f"Extension directory already exists: {skill_dir}")

    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()

    primary_dcc = dcc_targets[0] if dcc_targets else "python"
    script_file = f"scripts/{action_name}.py"

    # --- SKILL.md ---
    skill_md = skill_dir / "SKILL.md"
    skill_md.write_text(_make_skill_md(name, description, primary_dcc, version, action_name, author), encoding="utf-8")

    # --- tools.yaml ---
    tools_yaml = skill_dir / "tools.yaml"
    tools_yaml.write_text(_make_tools_yaml(action_name, description), encoding="utf-8")

    # --- action script ---
    action_script = skill_dir / script_file
    action_script.write_text(_make_action_script(name, action_name), encoding="utf-8")

    return str(skill_dir)


# ── Templates ───────────────────────────────────────────────────────────────────


def _make_skill_md(
    name: str,
    description: str,
    primary_dcc: str,
    version: str,
    action_name: str,
    author: str,
) -> str:
    """Generate SKILL.md content with MIT-0 license frontmatter."""
    desc = description or "DCC skill - TODO: describe the user intent this extension serves."
    layer = "domain" if primary_dcc != "python" else "infrastructure"
    hint = f"{name} {desc}".lower().replace("-", " ").split()[:12]
    search_hint = ", ".join(hint)
    tags = f'["marketplace", "extension", "{primary_dcc}"]'
    author_block = f"\n    maintainer: {author}" if author else ""

    return f"""---
name: {name}
description: >-
  {desc}
license: MIT-0
compatibility: "Python 3.7+, dcc-mcp-core 0.17+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: {primary_dcc}
    version: "{version}"
    layer: {layer}
    search-hint: "{search_hint}"
    tags: {tags}
    tools: tools.yaml{author_block}
---

# {name.replace("-", " ").title()}

{desc}

Keep instructions focused on when an agent should use this extension, what
each tool returns, and what to inspect after success or failure.

Tool names must stay client-safe. Use local snake_case tool names in
`tools.yaml`; dcc-mcp-core publishes them as `{name}__tool_name` after the
extension is loaded.

Runtime scripts should import dependency-light helpers from
`dcc_mcp_core.skills_helper` first. It provides JSON/YAML, bounded HTTP,
safe file/path helpers, validation, cancellation checks, and result helpers.

## License

MIT-0 — see <https://github.com/aws/mit-0> for details.
"""


def _make_tools_yaml(action_name: str, description: str) -> str:
    """Generate a tools.yaml skeleton in the skills/ directory format."""
    desc = description or "Example scaffold tool. Replace with a concrete extension intent."
    return f"""tools:
  - name: {action_name}
    description: "{desc}"
    source_file: scripts/{action_name}.py
    input_schema:
      type: object
      properties:
        label:
          type: string
          description: "Optional label echoed by the scaffold implementation."
          default: "example"
        dry_run:
          type: boolean
          description: "Keep true until this scaffold performs real work."
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
    execution: sync
    affinity: any
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
"""


def _make_action_script(name: str, action_name: str) -> str:
    """Generate a placeholder action script using the skills_helper pattern."""
    return f'''"""Action: {action_name} — {name} extension.

Replace this scaffold with a concrete marketplace extension operation.

MIT-0 — see <https://github.com/aws/mit-0> for details.
"""
from __future__ import annotations

from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success


@skill_entry
def main(label: str = "example", dry_run: bool = True, **params):
    """Replace this scaffold with a concrete extension operation."""
    return skill_success(
        "Example tool completed",
        prompt="Replace this scaffold with a concrete extension operation.",
        label=label,
        dry_run=dry_run,
        extra_params=params,
    )


if __name__ == "__main__":
    run_main(main)
'''


# ── CLI entry point ─────────────────────────────────────────────────────────────


def _cli_main() -> None:
    """Scaffold a marketplace extension package from CLI arguments."""
    import argparse
    import json

    parser = argparse.ArgumentParser(description="Scaffold a marketplace extension package.")
    parser.add_argument("--name", required=True, help="Package name (kebab-case)")
    parser.add_argument("--description", default="", help="Human-readable description")
    parser.add_argument("--dcc_targets", nargs="*", default=[], help="DCC targets (space-separated)")
    parser.add_argument("--install_type", default="git", choices=["git", "path", "zip"])
    parser.add_argument("--author", default="", help="Extension author or maintainer")
    parser.add_argument("--output_dir", default=".", help="Parent output directory")
    parser.add_argument("--version", default="0.1.0", help="Semantic version")
    parser.add_argument("--action_name", default="run", help="Placeholder action script name")
    args = parser.parse_args()

    try:
        path = create_extension(
            name=args.name,
            parent_dir=args.output_dir,
            description=args.description,
            dcc_targets=args.dcc_targets,
            install_type=args.install_type,
            author=args.author,
            version=args.version,
            action_name=args.action_name,
        )
        print(
            json.dumps(
                {
                    "success": True,
                    "message": f"Created marketplace extension package: {args.name}",
                    "context": {
                        "package_name": args.name,
                        "output_path": path,
                        "install_type": args.install_type,
                        "dcc_targets": args.dcc_targets,
                        "version": args.version,
                        "author": args.author,
                        "license": "MIT-0",
                    },
                }
            )
        )
    except Exception as e:
        print(
            json.dumps(
                {
                    "success": False,
                    "message": str(e),
                }
            )
        )
        sys.exit(1)


if __name__ == "__main__":
    _cli_main()
