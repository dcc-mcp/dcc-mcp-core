"""Validate an installable skill directory using dcc_mcp_core.validate_skill."""

from __future__ import annotations

import ast
from pathlib import Path
import sys

_AVOIDABLE_IMPORTS = {
    "requests": "use http_request/http_get_json/http_post_json from dcc_mcp_core.skills_helper for bounded REST calls",
    "httpx": "use http_request/http_get_json/http_post_json from dcc_mcp_core.skills_helper for bounded REST calls",
    "yaml": "use yaml_loads/yaml_dumps or load_yaml_file/dump_yaml_file from dcc_mcp_core.skills_helper",
    "ruamel": "use yaml_loads/yaml_dumps or load_yaml_file/dump_yaml_file from dcc_mcp_core.skills_helper",
}
_LOCAL_HELPER_MODULES = {
    "file_utils",
    "http_utils",
    "json_utils",
    "path_utils",
    "yaml_utils",
}


def validate_skill_dir(skill_dir: str) -> dict:
    """Validate a skill directory and return a structured report.

    Args:
        skill_dir: Path to the installable skill directory containing SKILL.md.

    Returns:
        Dict with 'skill_dir', 'is_clean', 'has_errors', and 'issues' list.

    """
    from dcc_mcp_core import validate_skill

    report = validate_skill(skill_dir)
    issues = [{"severity": i.severity, "category": i.category, "message": i.message} for i in report.issues]
    issues.extend(_skill_helper_adoption_warnings(skill_dir))
    has_errors = any(issue["severity"] == "error" for issue in issues)
    has_warnings = any(issue["severity"] == "warning" for issue in issues)
    return {
        "skill_dir": report.skill_dir,
        "is_clean": not has_errors and not has_warnings,
        "has_errors": has_errors,
        "has_warnings": has_warnings,
        "issues": issues,
    }


def _skill_helper_adoption_warnings(skill_dir: str) -> list[dict]:
    """Warn when skill scripts import dependencies covered by skills_helper."""
    scripts_dir = Path(skill_dir) / "scripts"
    if not scripts_dir.is_dir():
        return []

    warnings: list[dict] = []
    seen: set[tuple[str, str]] = set()
    for script in sorted(scripts_dir.rglob("*.py")):
        try:
            tree = ast.parse(script.read_text(encoding="utf-8"), filename=str(script))
        except (OSError, SyntaxError):
            continue
        rel = script.relative_to(skill_dir).as_posix()
        for node in ast.walk(tree):
            for module, hint in _iter_import_hints(node):
                key = (rel, module)
                if key in seen:
                    continue
                seen.add(key)
                warnings.append(
                    {
                        "severity": "warning",
                        "category": "skill-helper-adoption",
                        "message": f"{rel} imports {module!r}; {hint}.",
                    }
                )
    return warnings


def _iter_import_hints(node: ast.AST):
    if isinstance(node, ast.Import):
        for alias in node.names:
            module = alias.name.split(".", 1)[0]
            hint = _AVOIDABLE_IMPORTS.get(module)
            if hint:
                yield module, hint
            elif module in _LOCAL_HELPER_MODULES:
                yield module, "review whether dcc_mcp_core.skills_helper already covers this local helper"
    elif isinstance(node, ast.ImportFrom):
        module = node.module or ""
        root = module.split(".", 1)[0]
        basename = module.rsplit(".", 1)[-1]
        hint = _AVOIDABLE_IMPORTS.get(root)
        if hint:
            yield root, hint
        elif node.level and basename in _LOCAL_HELPER_MODULES:
            yield basename, "review whether dcc_mcp_core.skills_helper already covers this local helper"
        elif root in _LOCAL_HELPER_MODULES:
            yield root, "review whether dcc_mcp_core.skills_helper already covers this local helper"


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python validate_skill_dir.py <skill_dir>")
        sys.exit(1)
    result = validate_skill_dir(sys.argv[1])
    print(f"Skill: {result['skill_dir']}")
    print(f"Clean: {result['is_clean']}")
    print(f"Errors: {result['has_errors']}")
    print(f"Warnings: {result['has_warnings']}")
    for issue in result["issues"]:
        print(f"  [{issue['severity']}] {issue['category']}: {issue['message']}")
