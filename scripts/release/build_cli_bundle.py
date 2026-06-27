#!/usr/bin/env python3
"""Build a standalone dcc-mcp-cli release zip."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import sys
from zipfile import ZIP_DEFLATED
from zipfile import ZipFile
from zipfile import ZipInfo


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


def build_bundle(
    *,
    version: str,
    platform: str,
    cli_bin: Path,
    out_dir: Path,
) -> Path:
    """Create a deployable CLI zip and return its path."""
    version = _validate_filename_part(version, label="version")
    platform = _validate_filename_part(platform, label="platform")

    out_dir.mkdir(parents=True, exist_ok=True)
    bundle_path = out_dir / f"dcc-mcp-cli-{version}-{platform}.zip"

    suffix = ".exe" if cli_bin.suffix.lower() == ".exe" else ""
    archive_name = f"dcc-mcp-cli{suffix}"

    with ZipFile(bundle_path, "w", ZIP_DEFLATED) as zf:
        _write_binary(zf, cli_bin, archive_name)

    return bundle_path


def main() -> int:
    """Run the CLI bundle builder."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--cli-bin", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, default=Path())
    args = parser.parse_args()

    try:
        bundle_path = build_bundle(
            version=args.version,
            platform=args.platform,
            cli_bin=args.cli_bin,
            out_dir=args.out_dir,
        )
    except Exception as exc:
        _github_error(str(exc))
        raise SystemExit(1) from exc

    bundle_output = bundle_path.as_posix()
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as fh:
            fh.write(f"cli_bundle_path={bundle_output}\n")
    print(bundle_output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
