#!/usr/bin/env python3
"""Stage repository legal notices into standalone Python package roots."""

from __future__ import annotations

from pathlib import Path
import shutil

NOTICE_FILES = ("LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.txt")
PACKAGE_ROOTS = (
    Path("pkg/dcc-mcp-server-bin"),
    Path("pkg/dcc-mcp-core-semantic"),
    Path("pkg/dcc-mcp-cli-bin"),
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
