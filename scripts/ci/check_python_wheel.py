"""Validate Python wheel tags, metadata, and native-extension contents."""

from __future__ import annotations

import argparse
from email.parser import Parser
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


_CORE_EXTENSION = re.compile(r"(?:^|/)dcc_mcp_core/_core(?:\.[^/]+)?\.(?:pyd|so)$")


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


def validate_wheel(path: Path, profile: str, contract: dict[str, Any]) -> list[str]:
    """Return every violation for one wheel and compatibility profile."""
    errors: list[str] = []
    profile_contract = contract["build"][profile]
    expected_tag = profile_contract["wheel_tag"]
    tag_marker = f"-{expected_tag}.whl" if profile == "lite_py37" else f"-{expected_tag}-"
    if tag_marker not in path.name:
        errors.append(f"filename must contain wheel tag {expected_tag}")

    try:
        with zipfile.ZipFile(str(path)) as archive:
            names = archive.namelist()
            has_core = any(_CORE_EXTENSION.search(name) for name in names)
            metadata = Parser().parsestr(_read_single_member(archive, ".dist-info/METADATA"))
            wheel_metadata = Parser().parsestr(_read_single_member(archive, ".dist-info/WHEEL"))
    except (OSError, ValueError, zipfile.BadZipFile, UnicodeDecodeError) as exc:
        return [f"cannot inspect wheel: {exc}"]

    expects_core = profile != "lite_py37"
    if has_core != expects_core:
        errors.append(f"compiled dcc_mcp_core._core presence is {has_core}, expected {expects_core}")

    requires_python = metadata.get("Requires-Python")
    expected_python = minimum_python_spec(contract)
    if requires_python != expected_python:
        errors.append(f"Requires-Python is {requires_python!r}, expected {expected_python!r}")

    root_is_pure = str(wheel_metadata.get("Root-Is-Purelib", "")).lower()
    expected_pure = "true" if profile == "lite_py37" else "false"
    if root_is_pure != expected_pure:
        errors.append(f"Root-Is-Purelib is {root_is_pure!r}, expected {expected_pure!r}")

    tags = wheel_metadata.get_all("Tag") or []
    filename_tags = _expanded_filename_tags(path)
    incompatible_tags = []
    for tag in tags:
        value = str(tag)
        compatible = value == expected_tag if profile == "lite_py37" else value.startswith(f"{expected_tag}-")
        if not compatible:
            incompatible_tags.append(value)
    if not tags:
        errors.append("WHEEL metadata must declare at least one Tag")
    elif incompatible_tags:
        errors.append(f"WHEEL metadata tags {incompatible_tags!r} do not match {expected_tag}")
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
    parser.add_argument("--profile", choices=("native_py37", "lite_py37", "abi3"), required=True)
    parser.add_argument("wheels", nargs="+")
    args = parser.parse_args(argv)

    contract = load_contract()
    failed = False
    for raw_path in args.wheels:
        raw = Path(raw_path)
        matches = list(raw.parent.glob(raw.name))
        paths = matches or [raw]
        for path in paths:
            errors = validate_wheel(path, args.profile, contract)
            if errors:
                failed = True
                for error in errors:
                    sys.stderr.write(f"{path}: {error}\n")
            else:
                sys.stdout.write(f"{path}: {args.profile} wheel contract OK\n")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
