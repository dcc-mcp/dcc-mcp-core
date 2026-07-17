#!/usr/bin/env python3
"""Inject the companion capture helper into a Windows server binary wheel."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import sys
import tempfile
import zipfile

HELPER_NAME = "dcc-mcp-capture-helper.exe"


def _build_record_csv(filenames_and_data: list[tuple[str, bytes]], *, record_path: str) -> str:
    """Produce a wheel RECORD CSV for the given entries."""
    lines: list[str] = []
    for filename, data in filenames_and_data:
        digest = hashlib.sha256(data).hexdigest()
        size = len(data)
        lines.append(f"{filename},sha256={digest},{size}")
    # RECORD itself, per PEP 427.
    lines.append(f"{record_path},,")
    return "\n".join(lines) + "\n"


def inject_helper(wheel_path: Path, helper: Path) -> None:
    """Rewrite ``wheel_path`` with ``helper`` beside its server script."""
    if not helper.is_file():
        raise FileNotFoundError(f"capture helper not found: {helper}")
    if helper.name.lower() != HELPER_NAME or helper.read_bytes()[:2] != b"MZ":
        raise ValueError(f"invalid Windows capture helper: {helper}")

    with tempfile.TemporaryDirectory(dir=wheel_path.parent) as temp_dir:
        replacement = Path(temp_dir) / wheel_path.name
        with zipfile.ZipFile(wheel_path) as source:
            script_members = [
                name
                for name in source.namelist()
                if ".data/scripts/" in name and name.lower().endswith("dcc-mcp-server.exe")
            ]
            if len(script_members) != 1:
                raise ValueError(f"expected one bundled dcc-mcp-server.exe, found {script_members}")
            helper_member = str(Path(script_members[0]).parent / HELPER_NAME).replace("\\", "/")
            records = [name for name in source.namelist() if name.endswith(".dist-info/RECORD")]
            if len(records) != 1:
                raise ValueError(f"expected one wheel RECORD, found {records}")
            record = records[0]

            entries: list[tuple[str, bytes]] = []
            with zipfile.ZipFile(str(replacement), "w") as destination:
                for info in source.infolist():
                    if info.filename in {record, helper_member}:
                        continue
                    data = source.read(info.filename)
                    destination.writestr(info, data)
                    entries.append((info.filename, data))
                helper_data = helper.read_bytes()
                destination.writestr(helper_member, helper_data)
                entries.append((helper_member, helper_data))
                # Rebuild RECORD — written last (PEP 427).
                entries.sort(key=lambda e: e[0])
                record_csv = _build_record_csv(entries, record_path=record)
                destination.writestr(record, record_csv)
        replacement.replace(wheel_path)


def main(argv: list[str] | None = None) -> int:
    """Run the server-wheel injection command."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--wheel-dir", type=Path, default=Path("pkg/dcc-mcp-server-bin/wheels"))
    parser.add_argument("--helper", type=Path, required=True)
    args = parser.parse_args(argv)
    wheels = sorted(args.wheel_dir.glob("dcc_mcp_server-*-win_amd64.whl"))
    if len(wheels) != 1:
        print(f"expected one Windows server wheel under {args.wheel_dir}, found {wheels}", file=sys.stderr)
        return 1
    try:
        inject_helper(wheels[0], args.helper)
    except (OSError, ValueError) as exc:
        print(f"capture helper injection failed: {exc}", file=sys.stderr)
        return 1
    print(wheels[0].as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
