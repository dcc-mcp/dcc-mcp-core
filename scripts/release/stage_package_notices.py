#!/usr/bin/env python3
"""Stage repository legal notices into standalone Python package roots."""

from __future__ import annotations

from pathlib import Path
import shutil

# Maturin owns the standard license metadata and places LICENSE/NOTICE in the
# wheel's dist-info/licenses directory. The generated third-party texts are
# project resources, so stage those explicitly into each Maturin package.
NOTICE_FILES = ("THIRD_PARTY_NOTICES.txt", "THIRD_PARTY_LICENSES.txt")
PACKAGE_ROOTS = (
    Path("python/dcc_mcp_core"),
    Path("pkg/dcc-mcp-server-bin"),
    Path("pkg/dcc-mcp-server-bin/python/dcc_mcp_server"),
    Path("pkg/dcc-mcp-core-semantic"),
    Path("pkg/dcc-mcp-core-semantic/python/dcc_mcp_core_semantic"),
)


def stage(root: Path) -> None:
    """Copy root legal notices into every standalone package root."""
    for package_root in PACKAGE_ROOTS:
        destination_root = root / package_root
        for name in NOTICE_FILES:
            source = root / name
            if not source.is_file():
                raise FileNotFoundError(f"repository notice not found: {source}")
            destination_root.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination_root / name)


def main() -> int:
    """Stage package notices from the current repository root."""
    stage(Path.cwd().resolve())
    print(f"staged {len(NOTICE_FILES)} legal notice files into {len(PACKAGE_ROOTS)} package roots")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
