#!/usr/bin/env python3
"""Validate legal notice files in release archives."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
import tarfile
import zipfile

# Maturin writes the project license metadata into the wheel metadata contract
# and the release package staging step adds the generated third-party texts to
# the importable package. The standalone Hatchling CLI wrapper is the exception
# because it owns all four files directly in its wheel payload.
REQUIRED_FILES = ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt")
MATURIN_WHEEL_FILES = ("THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt")
CORE_SDIST_FILES = ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt")
MATURIN_WHEEL_PREFIXES = ("dcc_mcp_core-", "dcc_mcp_core_semantic-", "dcc_mcp_server-")


def _expected_files(root: Path) -> dict[str, bytes]:
    expected: dict[str, bytes] = {}
    for name in REQUIRED_FILES:
        path = root / name
        if not path.is_file():
            raise ValueError(f"repository is missing required notice file {name!r}")
        expected[name] = path.read_bytes()
    return expected


def _required_files_for_archive(path: Path) -> tuple[str, ...]:
    """Return the notice files required by the archive packaging contract."""
    if path.suffix == ".whl" and path.name.startswith(MATURIN_WHEEL_PREFIXES):
        return MATURIN_WHEEL_FILES
    if path.name.startswith("dcc_mcp_core-") and path.name.endswith(".tar.gz"):
        return CORE_SDIST_FILES
    return REQUIRED_FILES


def _member_basename(name: str) -> str:
    return Path(name.replace("\\", "/")).name


def _validate_members(archive_name: str, members: dict[str, bytes], expected: dict[str, bytes]) -> list[str]:
    errors: list[str] = []
    for required_name, expected_bytes in expected.items():
        matches = [name for name in members if _member_basename(name).lower() == required_name.lower()]
        if not matches:
            errors.append(f"{archive_name}: missing {required_name}")
            continue
        mismatches = [name for name in matches if members[name] != expected_bytes]
        if mismatches:
            errors.append(f"{archive_name}: {required_name} does not match the repository notice in {mismatches}")
    return errors


def _zip_members(path: Path) -> dict[str, bytes]:
    with zipfile.ZipFile(path) as archive:
        corrupt_member = archive.testzip()
        if corrupt_member is not None:
            raise ValueError(f"corrupt ZIP member {corrupt_member!r}")
        return {name: archive.read(name) for name in archive.namelist() if not name.endswith("/")}


def _tar_members(path: Path) -> dict[str, bytes]:
    with tarfile.open(path, "r:gz") as archive:
        members: dict[str, bytes] = {}
        for member in archive.getmembers():
            if not member.isfile():
                continue
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"unreadable TAR member {member.name!r}")
            members[member.name] = stream.read()
        return members


def validate_archive(path: Path, expected: dict[str, bytes]) -> list[str]:
    """Return notice errors for one wheel, source archive, or release ZIP."""
    try:
        if path.name.endswith((".whl", ".zip")):
            members = _zip_members(path)
        elif path.name.endswith((".tar.gz", ".tgz")):
            members = _tar_members(path)
        else:
            return [f"{path}: unsupported release archive format"]
    except (OSError, tarfile.TarError, zipfile.BadZipFile, ValueError) as exc:
        return [f"{path}: cannot inspect archive: {exc}"]
    required = _required_files_for_archive(path)
    scoped_expected = {name: expected[name] for name in required}
    return _validate_members(path.name, members, scoped_expected)


def main() -> int:
    """Run the release archive notice gate."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    parser.add_argument("archives", nargs="+", type=Path)
    args = parser.parse_args()
    try:
        expected = _expected_files(args.root.resolve())
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    errors: list[str] = []
    for archive in args.archives:
        errors.extend(validate_archive(archive, expected))
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"release notice gate passed for {len(args.archives)} archive(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
