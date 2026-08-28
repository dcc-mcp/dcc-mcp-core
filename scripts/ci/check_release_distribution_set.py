#!/usr/bin/env python3
"""Verify an immutable dcc-mcp-core GitHub Release distribution set."""

from __future__ import annotations

import argparse
from email.parser import Parser
import hashlib
import json
from pathlib import Path
import re
import sys
import tarfile
import zipfile

try:
    from .archive_payload_policy import archive_member_errors
    from .check_python_wheel import forbidden_runtime_dependency_errors
    from .check_python_wheel import validate_wheel
    from .python_support_contract import load_contract
except ImportError:  # pragma: no cover - direct script execution
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from archive_payload_policy import archive_member_errors
    from check_python_wheel import forbidden_runtime_dependency_errors
    from check_python_wheel import validate_wheel
    from python_support_contract import load_contract


class DistributionSetError(ValueError):
    """Raised when archived Release distributions are incomplete or invalid."""


_EXPECTED_WHEEL_CONTRACTS = {
    "native_py37/linux-x86_64",
    "native_py37/windows-x86_64",
    "abi3/linux-x86_64",
    "abi3/windows-x86_64",
    "abi3/macos-universal2",
    "lite_py37/any",
}


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _classify_wheel(filename: str, version: str) -> tuple[str, str]:
    prefix = f"dcc_mcp_core-{version}-"
    if not filename.startswith(prefix) or not filename.endswith(".whl"):
        raise DistributionSetError(f"unexpected Core wheel filename {filename!r}")
    tags = filename[len(prefix) : -4]
    if tags == "py3-none-any":
        return "lite_py37", "any"
    if tags.startswith("cp37-cp37m-manylinux_"):
        return "native_py37", "linux-x86_64"
    if tags == "cp37-cp37m-win_amd64":
        return "native_py37", "windows-x86_64"
    if tags.startswith("cp38-abi3-manylinux_"):
        return "abi3", "linux-x86_64"
    if tags == "cp38-abi3-win_amd64":
        return "abi3", "windows-x86_64"
    if tags.startswith("cp38-abi3-macosx_") and "universal2" in tags:
        return "abi3", "macos-universal2"
    raise DistributionSetError(f"unexpected Core wheel tags in {filename!r}")


def _single_metadata(archive: zipfile.ZipFile) -> str:
    matches = [name for name in archive.namelist() if name.endswith(".dist-info/METADATA")]
    if len(matches) != 1:
        raise DistributionSetError(f"wheel must contain exactly one METADATA file, found {len(matches)}")
    return archive.read(matches[0]).decode("utf-8")


def _validate_metadata(raw: str, version: str, filename: str) -> None:
    metadata = Parser().parsestr(raw)
    name = str(metadata.get("Name", "")).lower().replace("_", "-")
    actual_version = str(metadata.get("Version", ""))
    if name != "dcc-mcp-core":
        raise DistributionSetError(f"{filename}: distribution name is {name!r}, expected 'dcc-mcp-core'")
    if actual_version != version:
        raise DistributionSetError(f"{filename}: metadata version is {actual_version!r}, expected {version!r}")


def _validate_wheels(dist_dir: Path, version: str, wheel_names: set[str]) -> None:
    observed_contracts = set()
    contract = load_contract()
    for filename in sorted(wheel_names):
        profile, platform = _classify_wheel(filename, version)
        contract_name = f"{profile}/{platform}"
        if contract_name in observed_contracts:
            raise DistributionSetError(f"duplicate wheel contract {contract_name!r}")
        observed_contracts.add(contract_name)
        path = dist_dir / filename
        try:
            with zipfile.ZipFile(path) as archive:
                corrupt_member = archive.testzip()
                if corrupt_member is not None:
                    raise DistributionSetError(f"{filename}: corrupt ZIP member {corrupt_member!r}")
                payload_errors = archive_member_errors(archive.infolist())
                if payload_errors:
                    raise DistributionSetError(f"{filename}: " + "; ".join(payload_errors))
                _validate_metadata(_single_metadata(archive), version, filename)
        except (OSError, UnicodeDecodeError, zipfile.BadZipFile) as exc:
            raise DistributionSetError(f"{filename}: cannot inspect wheel: {exc}") from exc
        errors = validate_wheel(path, profile, platform, contract)
        if errors:
            raise DistributionSetError(f"{filename}: " + "; ".join(errors))

    missing = sorted(_EXPECTED_WHEEL_CONTRACTS - observed_contracts)
    unexpected = sorted(observed_contracts - _EXPECTED_WHEEL_CONTRACTS)
    if missing or unexpected:
        raise DistributionSetError(
            f"Core wheel contract set is invalid; missing wheel contract(s)={missing}, unexpected={unexpected}"
        )


def _validate_sdist(path: Path, version: str, contract: dict | None = None) -> None:
    root = f"dcc_mcp_core-{version}"
    pkg_info_name = f"{root}/PKG-INFO"
    try:
        with tarfile.open(path, "r:gz") as archive:
            members = archive.getmembers()
            names = [member.name for member in members]
            payload_errors = archive_member_errors(members)
            if payload_errors:
                raise DistributionSetError(f"{path.name}: " + "; ".join(payload_errors))
            # Consumers open literal TAR paths; portable normalization above
            # only rejects aliases/conflicts, never supplies missing files.
            if names.count(pkg_info_name) != 1:
                raise DistributionSetError(f"sdist must contain exactly one {pkg_info_name!r}")
            invalid_names = [name for name in names if name != root and not name.startswith(f"{root}/")]
            if invalid_names:
                raise DistributionSetError(f"sdist contains paths outside {root!r}: {invalid_names[:3]}")
            pkg_info = ""
            for member in members:
                if not member.isfile():
                    continue
                stream = archive.extractfile(member)
                if stream is None:
                    raise DistributionSetError(f"sdist member {member.name!r} is unreadable")
                data = stream.read()
                if member.name == pkg_info_name:
                    pkg_info = data.decode("utf-8")
            _validate_metadata(pkg_info, version, path.name)
            active_contract = contract if contract is not None else load_contract()
            distribution = active_contract.get("distributions", {}).get("dcc-mcp-core", {})
            dependency_errors = forbidden_runtime_dependency_errors(Parser().parsestr(pkg_info), distribution)
            if dependency_errors:
                raise DistributionSetError(f"{path.name}: " + "; ".join(dependency_errors))
    except (OSError, UnicodeDecodeError, tarfile.TarError) as exc:
        raise DistributionSetError(f"{path.name}: cannot inspect sdist: {exc}") from exc


def _release_assets(payload: dict, version: str) -> dict[str, dict]:
    assets_payload = payload.get("assets") if isinstance(payload, dict) else None
    if not isinstance(assets_payload, list) or any(not isinstance(item, dict) for item in assets_payload):
        raise DistributionSetError("GitHub Release asset JSON is invalid")
    prefix = f"dcc_mcp_core-{version}"
    assets = {}
    for asset in assets_payload:
        name = asset.get("name")
        if not isinstance(name, str) or not name.startswith(prefix):
            continue
        if Path(name).name != name or name in assets:
            raise DistributionSetError(f"GitHub Release has an invalid or duplicate Core asset name {name!r}")
        assets[name] = asset
    return assets


def classify_distribution_assets(payload: dict, version: str) -> str:
    """Choose whether an immutable Core backfill can reuse or must rebuild."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[A-Za-z0-9.+-]*)", version):
        raise DistributionSetError(f"invalid release version {version!r}")
    assets = _release_assets(payload, version)
    if not assets:
        return "rebuild"

    sdist_name = f"dcc_mcp_core-{version}.tar.gz"
    wheel_names = {name for name in assets if name.endswith(".whl")}
    observed_contracts = {_classify_wheel(name, version) for name in wheel_names}
    expected_contracts = {tuple(contract.split("/", 1)) for contract in _EXPECTED_WHEEL_CONTRACTS}
    if set(assets) != {*wheel_names, sdist_name} or observed_contracts != expected_contracts:
        raise DistributionSetError(
            "GitHub Release has a partial Core distribution set; refuse to mix archived and rebuilt bytes"
        )
    return "reuse"


def validate_distribution_directory(dist_dir: Path, version: str) -> dict[str, int]:
    """Validate a freshly built seven-file Core distribution set before publication."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[A-Za-z0-9.+-]*)", version):
        raise DistributionSetError(f"invalid release version {version!r}")
    if not dist_dir.is_dir():
        raise DistributionSetError(f"distribution directory does not exist: {dist_dir}")
    paths = [path for path in dist_dir.iterdir() if path.is_file()]
    if any(path.is_symlink() for path in paths):
        raise DistributionSetError("distribution directory contains a symbolic link")
    downloaded = {path.name: path for path in paths}
    if len(downloaded) != len(paths):
        raise DistributionSetError("distribution directory contains duplicate filenames")

    sdist_name = f"dcc_mcp_core-{version}.tar.gz"
    wheel_names = {name for name in downloaded if name.endswith(".whl")}
    if set(downloaded) != {*wheel_names, sdist_name}:
        raise DistributionSetError("Core distribution set must contain six wheels and one exact-version sdist")
    _validate_wheels(dist_dir, version, wheel_names)
    _validate_sdist(dist_dir / sdist_name, version, load_contract())
    return {
        "asset_count": len(downloaded),
        "total_bytes": sum(path.stat().st_size for path in downloaded.values()),
    }


def verify_distribution_set(payload: dict, dist_dir: Path, version: str) -> dict[str, int]:
    """Verify the exact seven-file Core distribution set and return evidence."""
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[A-Za-z0-9.+-]*)", version):
        raise DistributionSetError(f"invalid release version {version!r}")
    if not dist_dir.is_dir():
        raise DistributionSetError(f"distribution directory does not exist: {dist_dir}")
    paths = [path for path in dist_dir.iterdir() if path.is_file()]
    downloaded = {path.name: path for path in paths}
    assets = _release_assets(payload, version)
    if set(downloaded) != set(assets):
        raise DistributionSetError(
            "downloaded Core distributions do not match GitHub Release assets; "
            f"missing={sorted(set(assets) - set(downloaded))}, extra={sorted(set(downloaded) - set(assets))}"
        )

    for name, path in downloaded.items():
        asset = assets[name]
        size = asset.get("size")
        digest = asset.get("digest")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0 or path.stat().st_size != size:
            raise DistributionSetError(f"{name}: size mismatch with GitHub Release metadata")
        if not isinstance(digest, str) or re.fullmatch(r"sha256:[0-9a-fA-F]{64}", digest) is None:
            raise DistributionSetError(f"{name}: GitHub Release SHA-256 digest is invalid")
        actual_digest = _sha256(path)
        if actual_digest != digest.split(":", 1)[1].lower():
            raise DistributionSetError(f"{name}: digest mismatch with GitHub Release metadata")

    return validate_distribution_directory(dist_dir, version)


def main(argv: list[str] | None = None) -> int:
    """Validate Release asset metadata and downloaded distributions."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets-json", type=Path)
    parser.add_argument("--dist-dir", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--classify-assets", action="store_true")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args(argv)
    if args.classify_assets:
        if args.assets_json is None:
            raise DistributionSetError("--assets-json is required with --classify-assets")
        try:
            payload = json.loads(args.assets_json.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise DistributionSetError(f"cannot read GitHub Release asset JSON: {exc}") from exc
        mode = classify_distribution_assets(payload, args.version)
        if args.github_output is not None:
            with args.github_output.open("a", encoding="utf-8", newline="\n") as stream:
                stream.write(f"mode={mode}\n")
        print(f"Core release asset backfill mode: {mode}")
        return 0
    if args.dist_dir is None:
        raise DistributionSetError("--dist-dir is required")
    if args.assets_json is None:
        evidence = validate_distribution_directory(args.dist_dir, args.version)
        print(f"Validated {evidence['asset_count']} fresh Core distribution(s), {evidence['total_bytes']} byte(s)")
        return 0
    try:
        payload = json.loads(args.assets_json.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise DistributionSetError(f"cannot read GitHub Release asset JSON: {exc}") from exc
    evidence = verify_distribution_set(payload, args.dist_dir, args.version)
    print(f"Verified {evidence['asset_count']} immutable Core distribution(s), {evidence['total_bytes']} byte(s)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except DistributionSetError as exc:
        print(f"::error::{exc}", file=sys.stderr)
        raise SystemExit(1) from exc
