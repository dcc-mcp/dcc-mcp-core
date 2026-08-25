#!/usr/bin/env python3
"""Validate the editable root package version recorded in ``uv.lock``."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import stat
import sys

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - exercised by the Python 3.7 CI lane
    import tomli as tomllib

EXPECTED_ROOT_PACKAGE = "dcc-mcp-core"
_PROJECT_VERSION_RE = re.compile(
    r"""
    ^\s*v?
    (?:[0-9]+!)?
    [0-9]+(?:\.[0-9]+)*
    (?:[-_.]?(?:a|b|c|rc|alpha|beta|pre|preview)[-_.]?[0-9]*)?
    (?:(?:-[0-9]+)|(?:[-_.]?(?:post|rev|r)[-_.]?[0-9]*))?
    (?:[-_.]?dev[-_.]?[0-9]*)?
    (?:\+[a-z0-9]+(?:[-_.][a-z0-9]+)*)?
    \s*$
    """,
    re.IGNORECASE | re.VERBOSE,
)


def _load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def _valid_project_version(value: object) -> bool:
    return isinstance(value, str) and _PROJECT_VERSION_RE.fullmatch(value) is not None


def _uv_lock_is_regular_file(path: Path) -> bool:
    try:
        metadata = os.lstat(str(path))
    except OSError:
        return False
    reparse_flag = getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    file_attributes = getattr(metadata, "st_file_attributes", 0)
    return (
        stat.S_ISREG(metadata.st_mode)
        and not stat.S_ISLNK(metadata.st_mode)
        and not bool(file_attributes & reparse_flag)
    )


def check_uv_lock_consistency(root: Path) -> list[str]:
    """Return stable errors for release metadata and editable-root lock drift."""
    config_path = root / "release-please-config.json"
    manifest_path = root / ".release-please-manifest.json"
    pyproject_path = root / "pyproject.toml"
    lock_path = root / "uv.lock"

    if not _uv_lock_is_regular_file(lock_path):
        return ["uv.lock must be a regular file and not a symlink or reparse point"]

    try:
        release_config = json.loads(config_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"cannot read {config_path.name}: {type(error).__name__}"]
    if not isinstance(release_config, dict):
        return ["release-please-config.json must contain a JSON object"]
    configured_packages = release_config.get("packages")
    if not isinstance(configured_packages, dict):
        return ["release-please-config.json packages must be a mapping"]
    configured_root = configured_packages.get(".")
    if not isinstance(configured_root, dict):
        return ['release-please-config.json packages["."] must be a mapping']
    configured_name = configured_root.get("package-name")
    if configured_name != EXPECTED_ROOT_PACKAGE:
        return [
            f"release-please-config.json root package-name {configured_name!r} "
            f"!= fixed identity {EXPECTED_ROOT_PACKAGE!r}"
        ]

    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"cannot read {manifest_path.name}: {type(error).__name__}"]
    if not isinstance(manifest, dict):
        return [".release-please-manifest.json must contain a JSON object"]
    expected_version = manifest.get(".")
    if not _valid_project_version(expected_version):
        return [f".release-please-manifest.json root version {expected_version!r} is not a valid project version"]

    try:
        pyproject = _load_toml(pyproject_path)
    except (OSError, tomllib.TOMLDecodeError) as error:
        return [f"cannot read {pyproject_path.name}: {type(error).__name__}"]
    if not isinstance(pyproject, dict):
        return ["pyproject.toml must contain a mapping"]
    project = pyproject.get("project")
    if not isinstance(project, dict):
        return ["pyproject.toml project must be a mapping"]
    project_name = project.get("name")
    project_version = project.get("version")
    if project_name != EXPECTED_ROOT_PACKAGE:
        return [f"pyproject.toml project.name {project_name!r} != fixed identity {EXPECTED_ROOT_PACKAGE!r}"]
    if not _valid_project_version(project_version):
        return [f"pyproject.toml project.version {project_version!r} is not a valid project version"]
    if project_version != expected_version:
        return [f"pyproject.toml project.version {project_version!r} != expected {expected_version!r}"]

    try:
        lock = _load_toml(lock_path)
    except (OSError, tomllib.TOMLDecodeError) as error:
        return [f"cannot read {lock_path.name}: {type(error).__name__}"]
    if not isinstance(lock, dict):
        return ["uv.lock must contain a mapping"]
    packages = lock.get("package")
    if not isinstance(packages, list):
        return ["uv.lock package must be a list"]
    if not all(isinstance(package, dict) for package in packages):
        return ["uv.lock package entries must be mappings"]
    sources = [package.get("source") for package in packages]
    if not all(source is None or isinstance(source, dict) for source in sources):
        return ["uv.lock package source values must be mappings"]
    editable_roots = [
        package
        for package in packages
        if isinstance(package.get("source"), dict) and package["source"].get("editable") == "."
    ]
    if len(editable_roots) != 1:
        return [f"uv.lock must contain exactly one source.editable='.' root package; found {len(editable_roots)}"]

    editable_root = editable_roots[0]
    lock_name = editable_root.get("name")
    if lock_name != EXPECTED_ROOT_PACKAGE:
        return [f"uv.lock editable root name {lock_name!r} != fixed identity {EXPECTED_ROOT_PACKAGE!r}"]
    lock_version = editable_root.get("version")
    if not _valid_project_version(lock_version):
        return [f"uv.lock editable root version {lock_version!r} is not a valid project version"]
    if lock_version != expected_version:
        return [
            f"uv.lock editable root {EXPECTED_ROOT_PACKAGE} version {lock_version!r} != expected {expected_version!r}"
        ]
    return []


def main() -> None:
    """Run the repository-root consistency check for CI."""
    errors = check_uv_lock_consistency(Path.cwd())
    if errors:
        for error in errors:
            print(f"::error::{error}", file=sys.stderr)
        raise SystemExit(1)
    print("uv.lock editable root version is consistent.")


if __name__ == "__main__":
    main()
