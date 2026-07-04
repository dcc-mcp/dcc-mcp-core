#!/usr/bin/env python3
"""Build py37-lite pure-Python wheel for dcc-mcp-core.

Produces ``dist/dcc_mcp_core-{version}-py3-none-any.whl`` — a pure-Python
wheel installable on Python 3.7 (Maya 2022 / Blender 2.83).

The wheel does NOT contain the Rust native extension (_core.cp*/_core.pyd).
Users that need HTTP/MCP gateway functionality must install dcc-mcp-server
separately; pip automatically handles this on Python 3.8+ via the environment
marker in pyproject.toml.

Usage:
    python scripts/build_py37_lite_wheel.py

Requires:
    pip install build hatchling
"""

from __future__ import annotations

import hashlib
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PYTHON_SRC = REPO / "python" / "dcc_mcp_core"
DIST = REPO / "dist"


def get_version() -> str:
    """Read version from pyproject.toml."""
    text = (REPO / "pyproject.toml").read_text("utf-8")
    m = re.search(r'version = "([^"]+)"', text)
    if not m:
        raise RuntimeError("version not found in pyproject.toml")
    return m.group(1)


def _write_pyproject_toml(path: Path, version: str) -> None:
    """Write a minimal pyproject.toml for the pure-Python wheel build."""
    path.write_text(
        f"""\
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "dcc-mcp-core"
version = "{version}"
description = "Foundational library for the DCC Model Context Protocol (MCP) ecosystem"
requires-python = ">=3.7"
dependencies = [
    # dcc-mcp-server binary is optional on Python 3.7; required on 3.8+.
    "dcc-mcp-server>=0.18.17,<1.0.0; python_version >= '3.8'",
]
""",
        encoding="utf-8",
    )


def build_wheel() -> Path:
    """Build the py37-lite wheel and return the path."""
    version = get_version()
    DIST.mkdir(exist_ok=True)

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        pkg_dst = tmp / "dcc_mcp_core"
        shutil.copytree(PYTHON_SRC, pkg_dst, symlinks=True)

        _write_pyproject_toml(tmp / "pyproject.toml", version)

        result = subprocess.run(
            [sys.executable, "-m", "build", "--wheel", str(tmp)],
            capture_output=True,
            text=True,
            cwd=str(tmp),
        )
        if result.returncode != 0:
            print(result.stdout, file=sys.stderr)
            print(result.stderr, file=sys.stderr)
            raise RuntimeError(f"build failed (exit {result.returncode})")

        wheels = list((tmp / "dist").glob("*.whl"))
        if not wheels:
            raise RuntimeError("no wheel produced by build backend")

        src = wheels[0]
        dst = DIST / src.name
        shutil.copy2(src, dst)

        # SHA256 checksum
        sha = hashlib.sha256(src.read_bytes()).hexdigest()
        (DIST / f"{src.name}.sha256").write_text(sha + "\n", encoding="utf-8")

        size_kb = dst.stat().st_size / 1024
        print(f"Built: {dst} ({size_kb:.0f} KB)")
        print(f"SHA256: {sha}")

        return dst


def verify_wheel_tag(path: Path) -> None:
    """Verify the wheel filename contains py3-none-any tag."""
    name = path.name
    if "py3-none-any" not in name:
        print(f"::warning::Unexpected wheel tag (expected py3-none-any): {name}")
    else:
        print(f"OK: py3-none-any tag confirmed — {name}")


def main() -> None:
    wheel = build_wheel()
    verify_wheel_tag(wheel)


if __name__ == "__main__":
    main()
