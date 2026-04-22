"""Validate a skill directory using dcc_mcp_core.validate_skill."""

from __future__ import annotations

import sys


def validate_skill_dir(skill_dir: str) -> dict:
    """Validate a skill directory and return a structured report.

    Args:
        skill_dir: Path to the skill directory.

    Returns:
        Dict with 'skill_dir', 'is_clean', 'has_errors', and 'issues' list.

    """
    from dcc_mcp_core import validate_skill

    report = validate_skill(skill_dir)
    return {
        "skill_dir": report.skill_dir,
        "is_clean": report.is_clean,
        "has_errors": report.has_errors,
        "issues": [{"severity": i.severity, "category": i.category, "message": i.message} for i in report.issues],
    }


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python validate_skill_dir.py <skill_dir>")
        sys.exit(1)
    result = validate_skill_dir(sys.argv[1])
    print(f"Skill: {result['skill_dir']}")
    print(f"Clean: {result['is_clean']}")
    print(f"Errors: {result['has_errors']}")
    for issue in result["issues"]:
        print(f"  [{issue['severity']}] {issue['category']}: {issue['message']}")
