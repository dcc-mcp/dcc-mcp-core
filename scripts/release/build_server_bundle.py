#!/usr/bin/env python3
"""Build deployable dcc-mcp-server release bundles."""

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


def _archive_binary_name(kind: str, source: Path) -> str:
    suffix = ".exe" if source.suffix.lower() == ".exe" else ""
    return f"dcc-mcp-{kind}{suffix}"


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
    server_bin: Path,
    cli_bin: Path,
    out_dir: Path,
) -> Path:
    """Create a deployable server zip and return its path."""
    version = _validate_filename_part(version, label="version")
    platform = _validate_filename_part(platform, label="platform")

    out_dir.mkdir(parents=True, exist_ok=True)
    bundle_path = out_dir / f"dcc-mcp-server-{version}-{platform}.zip"

    with ZipFile(bundle_path, "w", ZIP_DEFLATED) as zf:
        _write_binary(zf, server_bin, _archive_binary_name("server", server_bin))
        _write_binary(zf, cli_bin, _archive_binary_name("cli", cli_bin))

    return bundle_path


def main() -> int:
    """Run the release bundle builder."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--server-bin", type=Path, required=True)
    parser.add_argument("--cli-bin", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, default=Path())
    args = parser.parse_args()

    try:
        bundle_path = build_bundle(
            version=args.version,
            platform=args.platform,
            server_bin=args.server_bin,
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
            fh.write(f"bundle_path={bundle_output}\n")
    print(bundle_output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
