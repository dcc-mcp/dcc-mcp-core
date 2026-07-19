#!/usr/bin/env python3
"""Stage the version-matched Windows UI Control host for a core wheel."""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import sys

HOST_NAME = "dcc-mcp-ui-control-host.exe"


def stage_host(source: Path, python_root: Path) -> Path:
    """Validate and copy ``source`` into the mixed Python package tree."""
    if not source.is_file():
        raise FileNotFoundError(f"UI Control host was not built: {source}")
    if source.name.lower() != HOST_NAME:
        raise ValueError(f"UI Control host must be named {HOST_NAME}, got {source.name}")
    if source.read_bytes()[:2] != b"MZ":
        raise ValueError(f"UI Control host is not a Windows PE executable: {source}")

    destination = python_root / "dcc_mcp_core" / "bin" / HOST_NAME
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return destination


def main(argv: list[str] | None = None) -> int:
    """Run the host staging command."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, default=Path("target/release") / HOST_NAME)
    parser.add_argument("--python-root", type=Path, default=Path("python"))
    args = parser.parse_args(argv)
    try:
        destination = stage_host(args.source, args.python_root)
    except (OSError, ValueError) as exc:
        print(f"UI Control host staging failed: {exc}", file=sys.stderr)
        return 1
    print(destination.as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
