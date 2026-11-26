#!/usr/bin/env python3
"""Build a standalone binary release zip."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import sys
from typing import Optional
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile
from zipfile import ZipInfo

REPO_ROOT = Path(__file__).resolve().parents[2]
NOTICE_FILES = ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt")


def _github_error(message: str) -> None:
    print(f"::error::{message}", file=sys.stderr)


def _validate_filename_part(name: str, *, label: str) -> str:
    if not name:
        raise ValueError(f"{label} is required")
    if any(sep in name for sep in ("/", "\\")):
        raise ValueError(f"{label} must be a filename component, got {name!r}")
    return name


def _write_binary(zf: ZipFile, source: Path, archive_name: str) -> None:
    if not source.is_file():
        raise FileNotFoundError(f"release binary not found: {source}")

    info = ZipInfo.from_file(source, arcname=archive_name)
    info.compress_type = ZIP_DEFLATED
    with source.open("rb") as src, zf.open(info, "w") as dst:
        shutil.copyfileobj(src, dst)


def _write_notice_files(zf: ZipFile) -> None:
    for name in NOTICE_FILES:
        source = REPO_ROOT / name
        if name == "THIRD_PARTY_LICENSES.txt" and not source.is_file():
            continue
        if not source.is_file():
            raise FileNotFoundError(f"release notice not found: {source}")
        _write_binary(zf, source, name)


def build_bundle(
    *,
    version: str,
    platform: str,
    binary_name: Optional[str] = None,
    binary_path: Optional[Path] = None,
    out_dir: Path,
    cli_bin: Optional[Path] = None,
    name_prefix: Optional[str] = None,
) -> Path:
    """Create a deployable standalone binary zip and return its path.

    ``binary_name``/``binary_path`` remain supported for release workflows and
    older callers. ``name_prefix``/``cli_bin`` are the clearer CLI-oriented
    aliases used by newer integrations.
    """
    binary_name = name_prefix or binary_name or "dcc-mcp-cli"
    binary_path = cli_bin or binary_path
    if binary_path is None:
        raise ValueError("binary_path or cli_bin is required")
    version = _validate_filename_part(version, label="version")
    platform = _validate_filename_part(platform, label="platform")
    binary_name = _validate_filename_part(binary_name, label="binary name")

    out_dir.mkdir(parents=True, exist_ok=True)
    bundle_path = out_dir / f"{binary_name}-{version}-{platform}.zip"
    suffix = ".exe" if binary_path.suffix.lower() == ".exe" else ""

    with ZipFile(bundle_path, "w", ZIP_DEFLATED) as zf:
        _write_notice_files(zf)
        _write_binary(zf, binary_path, f"{binary_name}{suffix}")

    return bundle_path


def main() -> int:
    """Run the standalone bundle builder."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--binary-name")
    parser.add_argument("--binary-path", type=Path)
    parser.add_argument("--name-prefix")
    parser.add_argument("--cli-bin", type=Path)
    parser.add_argument("--out-dir", type=Path, default=Path())
    args = parser.parse_args()

    try:
        if args.binary_path is None and args.cli_bin is None:
            parser.error("one of --binary-path or --cli-bin is required")
        bundle_path = build_bundle(
            version=args.version,
            platform=args.platform,
            binary_name=args.binary_name,
            binary_path=args.binary_path,
            out_dir=args.out_dir,
            cli_bin=args.cli_bin,
            name_prefix=args.name_prefix,
        )
    except Exception as exc:
        _github_error(str(exc))
        raise SystemExit(1) from exc

    bundle_output = bundle_path.as_posix()
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as fh:
            fh.write(f"bundle_path={bundle_output}\n")
    print(bundle_output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
