"""Build the py37-lite ``py3-none-any`` wheel without maturin or ``_core``."""

from __future__ import annotations

from email.message import EmailMessage
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PYTHON_ROOT = ROOT / "python"
PACKAGE_DIR = PYTHON_ROOT / "dcc_mcp_core"
DIST = ROOT / "dist"


def _ensure_wheel() -> None:
    try:
        import wheel.wheelfile  # noqa: F401
    except ImportError:
        subprocess.check_call([sys.executable, "-m", "pip", "install", "wheel"])


def _read_version() -> str:
    text = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    match = re.search(r'^version = "([^"]+)"', text, re.MULTILINE)
    if not match:
        raise RuntimeError("could not read project version from pyproject.toml")
    return match.group(1)


def _read_minimum_python_spec() -> str:
    contract_path = ROOT / "compatibility" / "python.json"
    contract = json.loads(contract_path.read_text(encoding="utf-8"))
    return ">={}".format(contract["support"]["minimum_python"])


def main() -> int:
    """Assemble a pure-Python wheel from ``python/dcc_mcp_core``."""
    _ensure_wheel()
    from wheel.wheelfile import WheelFile

    version = _read_version()
    dist_name = "dcc_mcp_core"
    dist_info = f"{dist_name}-{version}.dist-info"
    wheel_name = f"{dist_name}-{version}-py3-none-any.whl"
    DIST.mkdir(parents=True, exist_ok=True)
    wheel_path = DIST / wheel_name

    metadata = EmailMessage()
    metadata["Metadata-Version"] = "2.1"
    metadata["Name"] = "dcc-mcp-core"
    metadata["Version"] = version
    metadata["Requires-Python"] = _read_minimum_python_spec()
    metadata["License"] = "MIT"
    metadata["Summary"] = "Foundational library for the DCC Model Context Protocol (MCP) ecosystem"

    wheel_meta = "Wheel-Version: 1.0\nGenerator: build_py37_pure_wheel\nRoot-Is-Purelib: true\nTag: py3-none-any\n"

    with WheelFile(str(wheel_path), "w") as wf:
        for file_path in sorted(PACKAGE_DIR.rglob("*")):
            if not file_path.is_file():
                continue
            if "__pycache__" in file_path.parts:
                continue
            arcname = file_path.relative_to(PYTHON_ROOT).as_posix()
            wf.write(str(file_path), arcname)

        wf.writestr(f"{dist_info}/METADATA", metadata.as_string())
        wf.writestr(f"{dist_info}/WHEEL", wheel_meta)

    print(f"Built wheel: {wheel_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
