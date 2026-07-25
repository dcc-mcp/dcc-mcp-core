"""publish — Validate, package, and publish a skill to marketplace."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def publish(
    skill_path: str,
    version: str = "auto",
    dry_run: bool = False,
) -> dict[str, Any]:
    """Publish a skill to the marketplace.

    Args:
        skill_path: Path to the skill directory.
        version: Version bump strategy.
        dry_run: Validate + package without uploading.

    Returns:
        Publish result.
    """
    # Step 1: Validate
    from .validate import validate
    validation = validate(skill_path)
    if not validation.get("valid"):
        return {
            "success": False,
            "published": False,
            "validation": validation,
            "error": "Validation failed — cannot publish.",
        }

    # Step 2: Package
    from .package import package as do_package
    pkg_result = do_package(skill_path, version=None if version == "auto" else version)
    if not pkg_result.get("success"):
        return {
            "success": False,
            "published": False,
            "validation": validation,
            "error": "Packaging failed: {}".format(pkg_result.get("error", "unknown")),
        }

    pkg_path = pkg_result.get("package_path", "")
    pkg_version = pkg_result.get("version", version)

    if dry_run:
        return {
            "success": True,
            "published": False,
            "dry_run": True,
            "package_version": pkg_version,
            "validation": validation,
            "package_path": pkg_path,
            "message": "Dry run complete. Package ready at: {}".format(pkg_path),
        }

    # Step 3: Publish via marketplace CLI
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "marketplace", "publish", "--package", pkg_path, "--output", "json"],
            capture_output=True, text=True, timeout=30,
        )
        if result.returncode == 0:
            pub_data = json.loads(result.stdout)
            return {
                "success": True,
                "published": True,
                "package_version": pkg_version,
                "validation": validation,
                "package_path": pkg_path,
                "marketplace_url": pub_data.get("url", ""),
            }
        else:
            return {
                "success": False,
                "published": False,
                "package_version": pkg_version,
                "validation": validation,
                "package_path": pkg_path,
                "error": "Marketplace publish failed: {}".format(result.stderr),
            }
    except Exception as e:
        return {
            "success": False,
            "published": False,
            "package_version": pkg_version,
            "validation": validation,
            "package_path": pkg_path,
            "error": "Marketplace publish error: {}".format(e),
        }
