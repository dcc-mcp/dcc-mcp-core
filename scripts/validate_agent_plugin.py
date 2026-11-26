"""Validate a DCCMCP agent plugin manifest and its skill entrypoint."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
from typing import Any

SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)
PLUGIN_ID = re.compile(r"^[a-z0-9]+(?:[-.][a-z0-9]+)*$")
REQUIRED_TOP_LEVEL = {
    "schema_version",
    "id",
    "name",
    "version",
    "runtime",
    "entrypoints",
    "capabilities",
    "commands",
    "permissions",
}


class ManifestError(ValueError):
    """Raised when an agent plugin manifest violates the contract."""


def _require_object(value: Any, path: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ManifestError(f"{path} must be an object")
    return value


def _require_string(value: Any, path: str) -> str:
    if not isinstance(value, str) or not value:
        raise ManifestError(f"{path} must be a non-empty string")
    return value


def _require_string_list(value: Any, path: str, *, non_empty: bool = False) -> list[str]:
    if not isinstance(value, list) or (non_empty and not value):
        raise ManifestError(f"{path} must be a list with at least one item")
    if any(not isinstance(item, str) or not item for item in value):
        raise ManifestError(f"{path} must contain non-empty strings")
    if len(value) != len(set(value)):
        raise ManifestError(f"{path} must not contain duplicates")
    return value


def validate_manifest(manifest: dict[str, Any], skill_root: Path) -> list[str]:
    """Return validation errors for one manifest and its package root."""
    errors: list[str] = []
    unknown = set(manifest) - REQUIRED_TOP_LEVEL
    if unknown:
        errors.append(f"unknown top-level fields: {', '.join(sorted(unknown))}")
    if manifest.get("schema_version") != "1":
        errors.append("schema_version must be '1'")

    plugin_id = manifest.get("id")
    if not isinstance(plugin_id, str) or not PLUGIN_ID.fullmatch(plugin_id):
        errors.append("id must match ^[a-z0-9]+(?:[-.][a-z0-9]+)*$")
    for field in ("name", "version"):
        if not isinstance(manifest.get(field), str) or not manifest[field]:
            errors.append(f"{field} must be a non-empty string")
    if isinstance(manifest.get("version"), str) and not SEMVER.fullmatch(manifest["version"]):
        errors.append("version must be semantic versioning")

    runtime = manifest.get("runtime")
    if not isinstance(runtime, dict):
        errors.append("runtime must be an object")
    else:
        unknown_runtime = set(runtime) - {"kind", "minimum_cli", "transports"}
        if unknown_runtime:
            errors.append(f"unknown runtime fields: {', '.join(sorted(unknown_runtime))}")
        if runtime.get("kind") != "dccmcp-cli":
            errors.append("runtime.kind must be 'dccmcp-cli'")
        if not isinstance(runtime.get("minimum_cli"), str) or not SEMVER.fullmatch(runtime["minimum_cli"]):
            errors.append("runtime.minimum_cli must be semantic versioning")
        try:
            transports = _require_string_list(runtime.get("transports"), "runtime.transports", non_empty=True)
            if any(item not in {"streamable-http", "rest"} for item in transports):
                errors.append("runtime.transports contains an unsupported transport")
        except ManifestError as error:
            errors.append(str(error))

    entrypoints = manifest.get("entrypoints")
    if not isinstance(entrypoints, list) or not entrypoints:
        errors.append("entrypoints must be a non-empty list")
    else:
        for index, entrypoint in enumerate(entrypoints):
            try:
                entrypoint = _require_object(entrypoint, f"entrypoints[{index}]")
                unknown_entrypoint = set(entrypoint) - {"kind", "path"}
                if unknown_entrypoint:
                    raise ManifestError(f"unknown entrypoint fields: {', '.join(sorted(unknown_entrypoint))}")
                if entrypoint.get("kind") != "skill":
                    raise ManifestError(f"entrypoints[{index}].kind must be 'skill'")
                if entrypoint.get("path") != "SKILL.md":
                    raise ManifestError(f"entrypoints[{index}].path must be 'SKILL.md'")
            except ManifestError as error:
                errors.append(str(error))
        if not (skill_root / "SKILL.md").is_file():
            errors.append(f"missing skill entrypoint: {skill_root / 'SKILL.md'}")

    try:
        _require_string_list(manifest.get("capabilities"), "capabilities", non_empty=True)
    except ManifestError as error:
        errors.append(str(error))

    commands = manifest.get("commands")
    if not isinstance(commands, list) or not commands:
        errors.append("commands must be a non-empty list")
    else:
        has_primary_command = False
        for index, command in enumerate(commands):
            try:
                command = _require_object(command, f"commands[{index}]")
                unknown_command = set(command) - {"name", "compatibility_aliases"}
                if unknown_command:
                    raise ManifestError(f"unknown command fields: {', '.join(sorted(unknown_command))}")
                command_name = _require_string(command.get("name"), f"commands[{index}].name")
                aliases = _require_string_list(
                    command.get("compatibility_aliases"),
                    f"commands[{index}].compatibility_aliases",
                )
                if command_name == "dccmcp" and "dcc-mcp-cli" in aliases:
                    has_primary_command = True
            except ManifestError as error:
                errors.append(str(error))
        if not has_primary_command:
            errors.append("commands must declare dccmcp with dcc-mcp-cli as a compatibility alias")

    permissions = manifest.get("permissions")
    if not isinstance(permissions, dict):
        errors.append("permissions must be an object")
    else:
        unknown_permissions = set(permissions) - {"network", "filesystem", "process"}
        if unknown_permissions:
            errors.append(f"unknown permission groups: {', '.join(sorted(unknown_permissions))}")
        for group in ("network", "filesystem", "process"):
            try:
                _require_string_list(permissions.get(group), f"permissions.{group}")
            except ManifestError as error:
                errors.append(str(error))
        process = permissions.get("process")
        if isinstance(process, list):
            for executable in ("dccmcp", "dcc-mcp-cli"):
                if executable not in process:
                    errors.append(f"permissions.process must include {executable!r}")

    return errors


def parse_args() -> argparse.Namespace:
    """Parse the manifest path and optional skill-root override."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument(
        "--skill-root",
        type=Path,
        help="Package root containing SKILL.md. Defaults to the manifest directory.",
    )
    return parser.parse_args()


def main() -> int:
    """Validate the requested manifest and return a process exit code."""
    args = parse_args()
    manifest_path = args.manifest.resolve()
    skill_root = (args.skill_root or manifest_path.parent).resolve()
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if not isinstance(manifest, dict):
            raise ManifestError("manifest root must be an object")
        errors = validate_manifest(manifest, skill_root)
    except (OSError, json.JSONDecodeError, ManifestError) as error:
        errors = [str(error)]

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"valid agent plugin: {manifest_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
