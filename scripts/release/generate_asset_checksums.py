#!/usr/bin/env python3
"""Write SHA-256 sidecars and a release checksum manifest."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import sys
from typing import Iterable, List


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_checksums(assets: Iterable[Path], output: Path) -> Path:
    checked = []
    for asset in assets:
        if not asset.is_file():
            raise FileNotFoundError(f"release asset not found: {asset}")
        digest = sha256(asset)
        sidecar = asset.with_name(asset.name + ".sha256")
        sidecar.write_text(f"{digest}  {asset.name}\n", encoding="utf-8")
        checked.append(f"{digest}  {asset.name}")

    output.write_text("\n".join(checked) + "\n", encoding="utf-8")
    return output


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--asset", type=Path, action="append", required=True)
    args = parser.parse_args()
    try:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        print(write_checksums(args.asset, args.output))
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
