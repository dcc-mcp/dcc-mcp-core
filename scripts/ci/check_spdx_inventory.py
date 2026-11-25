#!/usr/bin/env python3
"""Build and validate the repository's SPDX dependency inventory."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
from typing import Any

SPDX_EXPRESSION = re.compile(r"^[A-Za-z0-9.+()/_\-\s]+$")
LICENSE_ASSIGNMENT = re.compile(
    r"^\s*license\s*=\s*(?:\{\s*text\s*=\s*)?['\"]([^'\"]+)['\"]",
    re.MULTILINE,
)
PROJECT_SECTION = re.compile(r"(?ms)^\[project\]\s*$\n(.*?)(?=^\[|\Z)")


def _project_license(path: Path) -> str | None:
    text = path.read_text(encoding="utf-8")
    match = PROJECT_SECTION.search(text)
    if not match:
        return None
    license_match = LICENSE_ASSIGNMENT.search(match.group(1))
    return license_match.group(1).strip() if license_match else None


def _validate_expression(value: str, source: str, errors: list[str]) -> None:
    if not value.strip() or not SPDX_EXPRESSION.fullmatch(value):
        errors.append(f"{source}: invalid SPDX expression {value!r}")


def _python_entries(root: Path, errors: list[str]) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("pyproject.toml")):
        if any(part in {".git", "target", "node_modules"} for part in path.parts):
            continue
        license_id = _project_license(path)
        if license_id is None:
            errors.append(f"{path.relative_to(root)}: missing [project] license metadata")
            continue
        _validate_expression(license_id, str(path.relative_to(root)), errors)
        entries.append(
            {
                "ecosystem": "python",
                "path": path.relative_to(root).as_posix(),
                "license": license_id,
            }
        )
    return entries


def _npm_entries(root: Path, errors: list[str]) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in sorted(root.rglob("package.json")):
        if any(part in {".git", "target", "node_modules"} for part in path.parts):
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("private") is True:
            license_id = "UNPUBLISHED"
        else:
            license_id = data.get("license")
            if not isinstance(license_id, str) or not license_id.strip():
                errors.append(f"{path.relative_to(root)}: missing package license metadata")
                continue
            license_id = license_id.strip()
            _validate_expression(license_id, str(path.relative_to(root)), errors)
        entries.append(
            {
                "ecosystem": "npm",
                "path": path.relative_to(root).as_posix(),
                "name": data.get("name"),
                "version": data.get("version"),
                "license": license_id,
                "private": data.get("private") is True,
            }
        )
    return entries


def _rust_entries(root: Path, errors: list[str]) -> list[dict[str, Any]]:
    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    try:
        completed = subprocess.run(
            command,
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", None) or str(exc)
        errors.append(f"cargo metadata failed: {detail.strip()}")
        return []

    metadata = json.loads(completed.stdout)
    entries: list[dict[str, Any]] = []
    for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
        license_id = package.get("license")
        if not license_id:
            if package["name"] == "workspace-hack" and package.get("source") is None:
                license_id = "MIT"
            else:
                errors.append(f"Rust package {package['name']} {package['version']}: missing license metadata")
                continue
        _validate_expression(license_id, f"Rust package {package['name']}", errors)
        entries.append(
            {
                "ecosystem": "rust",
                "name": package["name"],
                "version": package["version"],
                "license": license_id,
                "source": package.get("source") or "workspace",
            }
        )
    return entries


def build_inventory(root: Path) -> tuple[dict[str, Any], list[str]]:
    """Collect Python, npm, and Rust package license metadata."""
    errors: list[str] = []
    entries = _python_entries(root, errors)
    entries.extend(_npm_entries(root, errors))
    entries.extend(_rust_entries(root, errors))
    inventory = {
        "schema_version": 1,
        "generated_by": "scripts/ci/check_spdx_inventory.py",
        "repository_license": "MIT",
        "entries": entries,
    }
    return inventory, errors


def main() -> int:
    """Run the inventory gate and return its process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    inventory, errors = build_inventory(root)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    counts: dict[str, int] = {}
    for entry in inventory["entries"]:
        ecosystem = entry["ecosystem"]
        counts[ecosystem] = counts.get(ecosystem, 0) + 1
    print("SPDX inventory is complete: " + ", ".join(f"{key}={counts[key]}" for key in sorted(counts)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
