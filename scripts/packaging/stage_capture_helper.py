#!/usr/bin/env python3
"""Stage the version-matched Windows capture helper for a core wheel."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import sys

HELPER_NAME = "dcc-mcp-capture-helper.exe"


def default_source_path() -> Path:
    """Return the Cargo release output path for the active target."""
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    build_target = os.environ.get("CARGO_BUILD_TARGET")
    if build_target:
        target_dir /= build_target
    return target_dir / "release" / HELPER_NAME


def stage_helper(source: Path, python_root: Path) -> Path:
    """Validate and copy ``source`` into the mixed Python package tree."""
    if not source.is_file():
        raise FileNotFoundError(f"capture helper was not built: {source}")
    if source.name.lower() != HELPER_NAME:
        raise ValueError(f"capture helper must be named {HELPER_NAME}, got {source.name}")
    if source.read_bytes()[:2] != b"MZ":
        raise ValueError(f"capture helper is not a Windows PE executable: {source}")

    destination = python_root / "dcc_mcp_core" / "bin" / HELPER_NAME
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return destination


def main(argv: list[str] | None = None) -> int:
    """Run the helper staging command."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--source",
        type=Path,
        default=default_source_path(),
    )
    parser.add_argument("--python-root", type=Path, default=Path("python"))
    args = parser.parse_args(argv)
    try:
        destination = stage_helper(args.source, args.python_root)
    except (OSError, ValueError) as exc:
        print(f"capture helper staging failed: {exc}", file=sys.stderr)
        return 1
    print(destination.as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
