#!/usr/bin/env python3
"""Extract the shared generated license text bundle from release ZIPs."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
from typing import List
from typing import Set
import zipfile

LICENSE_MEMBER = "THIRD_PARTY_LICENSES.txt"


def extract(archives: List[Path], output: Path) -> None:
    """Extract identical license text members from release archives."""
    contents: Set[bytes] = set()
    for archive_path in archives:
        with zipfile.ZipFile(archive_path) as archive:
            matches = [name for name in archive.namelist() if Path(name).name == LICENSE_MEMBER]
            if len(matches) != 1:
                raise ValueError(f"{archive_path}: expected one {LICENSE_MEMBER}, found {len(matches)}")
            contents.add(archive.read(matches[0]))
    if len(contents) != 1:
        raise ValueError("release archives contain different third-party license text bundles")
    output.write_bytes(contents.pop())


def main() -> int:
    """Extract the license text bundle from the supplied release ZIPs."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("archives", nargs="+", type=Path)
    args = parser.parse_args()
    try:
        extract(args.archives, args.output)
    except (OSError, ValueError, zipfile.BadZipFile) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    print(f"extracted {LICENSE_MEMBER} to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
