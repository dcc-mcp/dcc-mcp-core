#!/usr/bin/env python3
"""Validate repository license metadata against the root license text."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import sys

LICENSE_ASSIGNMENT = re.compile(
    r"^\s*license\s*=\s*(?:\{\s*text\s*=\s*)?['\"]([^'\"]+)['\"]",
    re.MULTILINE,
)


def _section(text: str, name: str) -> str:
    match = re.search(
        rf"(?ms)^\[{re.escape(name)}\]\s*$\n(.*?)(?=^\[|\Z)",
        text,
    )
    return match.group(1) if match else ""


def _declared_license(path: Path, section: str | None = None) -> str | None:
    text = path.read_text(encoding="utf-8")
    if section:
        text = _section(text, section)
    match = LICENSE_ASSIGNMENT.search(text)
    return match.group(1).strip() if match else None


def _license_text_matches(spdx_id: str, text: str) -> bool:
    if spdx_id == "MIT":
        return "MIT License" in text and "Permission is hereby granted" in text
    if spdx_id == "Apache-2.0":
        return "Apache License" in text and "Version 2.0" in text
    if spdx_id == "BSD-3-Clause":
        return "Redistribution and use in source and binary forms" in text
    return f"SPDX-License-Identifier: {spdx_id}" in text


def validate(root: Path) -> list[str]:
    """Return license metadata validation errors for a repository root."""
    errors: list[str] = []
    license_path = root / "LICENSE"
    pyproject_path = root / "pyproject.toml"
    cargo_path = root / "Cargo.toml"

    if not license_path.is_file():
        return ["root LICENSE is missing"]
    license_text = license_path.read_text(encoding="utf-8")
    if not license_text.strip():
        errors.append("root LICENSE is empty")

    declared = []
    pyproject_license = _declared_license(pyproject_path)
    cargo_license = _declared_license(cargo_path, "workspace.package")
    if pyproject_license:
        declared.append(("pyproject.toml", pyproject_license))
    else:
        errors.append("pyproject.toml does not declare project license metadata")
    if cargo_license:
        declared.append(("Cargo.toml [workspace.package]", cargo_license))
    else:
        errors.append("Cargo.toml [workspace.package] does not declare license metadata")

    values = {value for _, value in declared}
    if len(values) > 1:
        details = ", ".join(f"{source}={value}" for source, value in declared)
        errors.append(f"license metadata disagrees: {details}")
    elif values:
        license_id = next(iter(values))
        if not _license_text_matches(license_id, license_text):
            errors.append(f"root LICENSE does not contain the expected {license_id} license text")

    return errors


def main() -> int:
    """Run the license metadata gate and return its process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    args = parser.parse_args()
    errors = validate(args.root.resolve())
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print("license metadata is consistent with the root LICENSE")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
