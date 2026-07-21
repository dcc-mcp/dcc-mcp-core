"""Validate Python wheel tags, metadata, and native-extension contents."""

from __future__ import annotations

import argparse
from email.parser import Parser
import fnmatch
from pathlib import Path
import re
import sys
from typing import Any
import zipfile

try:
    from .python_support_contract import load_contract
    from .python_support_contract import minimum_python_spec
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from python_support_contract import load_contract
    from python_support_contract import minimum_python_spec


def _read_single_member(archive: zipfile.ZipFile, suffix: str) -> str:
    matches = [name for name in archive.namelist() if name.endswith(suffix)]
    if len(matches) != 1:
        raise ValueError(f"expected exactly one *{suffix}, found {len(matches)}")
    return archive.read(matches[0]).decode("utf-8")


def _expanded_filename_tags(path: Path) -> set[str]:
    """Expand the compressed tag triple encoded by a wheel filename."""
    components = path.name[:-4].split("-")[-3:]
    if len(components) != 3:
        return set()
    python_tags, abi_tags, platform_tags = (component.split(".") for component in components)
    return {
        f"{python_tag}-{abi_tag}-{platform_tag}"
        for python_tag in python_tags
        for abi_tag in abi_tags
        for platform_tag in platform_tags
    }


def _contains_extension(names: list[str], module_path: str | None) -> bool:
    if not module_path:
        return False
    extension = re.compile(rf"(?:^|/){re.escape(module_path)}(?:\.[^/]+)?\.(?:pyd|so)$")
    return any(extension.search(name) for name in names)


def _platform_tag_allowed(platform_tag: str, policy: dict[str, Any]) -> bool:
    allowed = policy.get("allowed_platform_tags", [])
    patterns = policy.get("allowed_platform_tag_patterns", [])
    return platform_tag in allowed or any(fnmatch.fnmatchcase(platform_tag, pattern) for pattern in patterns)


def _release_tuple(version: str) -> tuple[int, int, int] | None:
    """Return the leading SemVer release tuple used by versioned wheel policy."""
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)(?:\D.*)?$", version)
    if match is None:
        return None
    return tuple(int(part) for part in match.groups())


def _core_ui_control_contract_errors(archive: zipfile.ZipFile, names: list[str]) -> list[str]:
    """Validate the post-0.19.63 Python/skill UI Control naming cutover."""
    errors: list[str] = []
    contracts_member = "dcc_mcp_core/adapter_contracts.py"
    skill_member = "dcc_mcp_core/skills/ui-control/SKILL.md"
    tools_member = "dcc_mcp_core/skills/ui-control/tools.yaml"
    for member in (contracts_member, skill_member, tools_member):
        if member not in names:
            errors.append(f"wheel is missing required UI Control member {member!r}")
    legacy_members = sorted(name for name in names if name.startswith("dcc_mcp_core/skills/app-ui/"))
    if legacy_members:
        errors.append("wheel contains removed app-ui skill members")
    if errors:
        return errors

    contracts = archive.read(contracts_member).decode("utf-8")
    skill = archive.read(skill_member).decode("utf-8")
    tools = archive.read(tools_member).decode("utf-8")
    if "class UiControlPolicy:" not in contracts or "class UiControlAuditRecord:" not in contracts:
        errors.append("adapter_contracts.py is missing canonical UI Control contracts")
    if "class AppUiPolicy:" in contracts or "class AppUiAuditRecord:" in contracts:
        errors.append("adapter_contracts.py contains removed App UI contracts")
    if "name: ui-control" not in skill or "ui_control__snapshot" not in skill:
        errors.append("bundled UI Control skill does not expose the canonical ui_control contract")
    if "app_ui__" in skill or "app_ui__" in tools:
        errors.append("bundled UI Control skill contains removed app_ui tool names")
    return errors


def validate_wheel(
    path: Path,
    profile: str,
    platform: str,
    contract: dict[str, Any],
) -> list[str]:
    """Return every violation for one wheel and compatibility profile."""
    errors: list[str] = []
    profile_contract = contract["wheel_profiles"].get(profile)
    if profile_contract is None:
        return [f"unknown wheel profile {profile!r}"]
    platform_policy = profile_contract["platforms"].get(platform)
    if platform_policy is None:
        return [f"profile {profile!r} does not support platform {platform!r}"]
    expected_tag = profile_contract["wheel_tag"]

    try:
        with zipfile.ZipFile(str(path)) as archive:
            names = archive.namelist()
            has_extension = _contains_extension(names, profile_contract.get("extension_module"))
            metadata = Parser().parsestr(_read_single_member(archive, ".dist-info/METADATA"))
            wheel_metadata = Parser().parsestr(_read_single_member(archive, ".dist-info/WHEEL"))
            actual_version = str(metadata.get("Version", ""))
            actual_release = _release_tuple(actual_version)
            if (
                profile_contract["distribution"] == "dcc-mcp-core"
                and actual_release is not None
                and actual_release >= (0, 19, 63)
            ):
                errors.extend(_core_ui_control_contract_errors(archive, names))
    except (OSError, ValueError, zipfile.BadZipFile, UnicodeDecodeError) as exc:
        return [f"cannot inspect wheel: {exc}"]

    expected_distribution = profile_contract["distribution"]
    actual_distribution = str(metadata.get("Name", "")).lower().replace("_", "-")
    if actual_distribution != expected_distribution:
        errors.append(f"Name is {actual_distribution!r}, expected {expected_distribution!r}")

    expects_extension = profile_contract["expects_extension"]
    if has_extension != expects_extension:
        module = profile_contract.get("extension_module", "compiled extension")
        errors.append(f"compiled {module} presence is {has_extension}, expected {expects_extension}")

    required_members = platform_policy.get("required_members", [])
    required_from = platform_policy.get("required_members_from_version")
    if required_from is not None:
        actual_version = str(metadata.get("Version", ""))
        actual_release = _release_tuple(actual_version)
        required_release = _release_tuple(str(required_from))
        if actual_release is None:
            errors.append(f"Version {actual_version!r} is not a valid release version")
            required_members = []
        elif required_release is None:
            errors.append(f"required_members_from_version {required_from!r} is invalid")
            required_members = []
        elif actual_release < required_release:
            required_members = []
    for member in required_members:
        if member not in names:
            errors.append(f"wheel is missing required member {member!r}")
    for member, required_from in platform_policy.get("versioned_required_members", {}).items():
        required_release = _release_tuple(str(required_from))
        if actual_release is None:
            errors.append(f"Version {actual_version!r} is not a valid release version")
        elif required_release is None:
            errors.append(f"versioned required member {member!r} has invalid version {required_from!r}")
        elif actual_release >= required_release and member not in names:
            errors.append(f"wheel is missing required member {member!r}")
    for member in platform_policy.get("forbidden_members", []):
        if member in names:
            errors.append(f"wheel contains forbidden member {member!r}")

    requires_python = metadata.get("Requires-Python")
    expected_python = minimum_python_spec(contract)
    if requires_python != expected_python:
        errors.append(f"Requires-Python is {requires_python!r}, expected {expected_python!r}")

    root_is_pure = str(wheel_metadata.get("Root-Is-Purelib", "")).lower()
    expected_pure = "true" if profile_contract["root_is_purelib"] else "false"
    if root_is_pure != expected_pure:
        errors.append(f"Root-Is-Purelib is {root_is_pure!r}, expected {expected_pure!r}")

    tags = wheel_metadata.get_all("Tag") or []
    filename_tags = _expanded_filename_tags(path)
    incompatible_tags: list[str] = []
    incompatible_platform_tags: list[str] = []
    observed_platform_tags: list[str] = []
    for tag in tags:
        value = str(tag)
        components = value.split("-", 2)
        if len(components) != 3 or "-".join(components[:2]) != expected_tag:
            incompatible_tags.append(value)
            continue
        if not _platform_tag_allowed(components[2], platform_policy):
            incompatible_platform_tags.append(components[2])
        observed_platform_tags.append(components[2])
    if not tags:
        errors.append("WHEEL metadata must declare at least one Tag")
    elif incompatible_tags:
        errors.append(f"WHEEL metadata tags {incompatible_tags!r} do not match {expected_tag}")
    if incompatible_platform_tags:
        errors.append(
            f"WHEEL platform tags {sorted(set(incompatible_platform_tags))!r} are not allowed for {profile}/{platform}"
        )
    for pattern in platform_policy.get("required_platform_tag_patterns", []):
        if not any(fnmatch.fnmatchcase(tag, pattern) for tag in observed_platform_tags):
            errors.append(f"WHEEL platform tags must include a tag matching {pattern!r}")
    metadata_tags = set(tags)
    if tags and metadata_tags != filename_tags:
        errors.append(
            f"WHEEL metadata tags {sorted(metadata_tags)!r} do not match "
            f"expanded filename tags {sorted(filename_tags)!r}"
        )
    return errors


def main(argv: list[str] | None = None) -> int:
    """Validate all wheel paths supplied on the command line."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("wheels", nargs="+")
    args = parser.parse_args(argv)

    contract = load_contract()
    failed = False
    for raw_path in args.wheels:
        raw = Path(raw_path)
        matches = list(raw.parent.glob(raw.name))
        paths = matches or [raw]
        for path in paths:
            errors = validate_wheel(path, args.profile, args.platform, contract)
            if errors:
                failed = True
                for error in errors:
                    sys.stderr.write(f"{path}: {error}\n")
            else:
                sys.stdout.write(f"{path}: {args.profile} wheel contract OK\n")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
