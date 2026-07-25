"""package — Build a deployable marketplace package from a skill directory."""
from __future__ import annotations

import json
import os
import shutil
import subprocess
from typing import Any


def package(
    skill_path: str,
    version: str | None = None,
    output_dir: str | None = None,
) -> dict[str, Any]:
    """Build a deployable package.

    Args:
        skill_path: Path to the skill directory.
        version: Version string. Reads from SKILL.md when omitted.
        output_dir: Output directory. Defaults to ./dist/.

    Returns:
        Package result.
    """
    if not os.path.isdir(skill_path):
        return {"success": False, "error": "Skill directory not found: {}".format(skill_path)}

    skill_name = os.path.basename(os.path.abspath(skill_path))
    output_dir = output_dir or os.path.join(os.getcwd(), "dist")
    os.makedirs(output_dir, exist_ok=True)

    # Read version from SKILL.md
    if not version:
        skill_md = os.path.join(skill_path, "SKILL.md")
        if os.path.isfile(skill_md):
            import re
            with open(skill_md, "r", encoding="utf-8") as f:
                content = f.read()
            match = re.search(r'version:\s*"([^"]+)"', content)
            if match:
                version = match.group(1)
        if not version:
            version = "0.1.0"

    # Build package
    pkg_name = "{}-{}.zip".format(skill_name, version)
    pkg_path = os.path.join(output_dir, pkg_name)

    # Validate first
    from .validate import validate
    validation = validate(skill_path)
    if not validation.get("valid"):
        return {
            "success": False,
            "error": "Validation failed — fix errors before packaging.",
            "validation": validation,
            "validation_passed": False,
        }

    # Create zip
    try:
        # Use shutil.make_archive for cross-platform zip creation
        base_name = os.path.join(output_dir, "{}-{}".format(skill_name, version))
        shutil.make_archive(base_name, "zip", os.path.dirname(os.path.abspath(skill_path)), skill_name)
        pkg_path = base_name + ".zip"
        size_bytes = os.path.getsize(pkg_path) if os.path.isfile(pkg_path) else 0
    except Exception as e:
        return {
            "success": False,
            "error": "Packaging failed: {}".format(e),
            "validation_passed": True,
        }

    return {
        "success": True,
        "package_path": pkg_path,
        "version": version,
        "validation_passed": True,
        "size_bytes": size_bytes,
    }
