"""Locator helpers for the bundled ``dcc-mcp-server`` Rust binary.

Usage::

    from dcc_mcp_server import binary_path
    import subprocess
    subprocess.Popen([str(binary_path()), "sidecar", "--dcc", "maya", ...])

Most users do not need this — after ``pip install dcc-mcp-server`` the
binary is on ``PATH`` and can be invoked directly. This helper exists
for DCC plugins / addons that want a stable absolute path to
``subprocess.Popen``.
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import sys

__all__ = ["__version__", "binary_path"]

# Kept in sync with pyproject.toml / Rust crate via release-please.
__version__ = "0.18.20"  # x-release-please-version

_BINARY_NAME = "dcc-mcp-server.exe" if os.name == "nt" else "dcc-mcp-server"


def binary_path() -> Path:
    """Return the filesystem path of the bundled ``dcc-mcp-server`` binary.

    Resolution order:

    1. ``DCC_MCP_SERVER_BIN`` env var — operator override.
    2. ``scripts/`` directory next to this package's installation (where
       maturin places ``bindings = "bin"`` artefacts inside the wheel).
    3. ``shutil.which("dcc-mcp-server")`` — the system PATH lookup that
       ``pip``'s ``console_scripts`` install path produces.
    4. ``sys.prefix/Scripts`` (Windows) or ``sys.prefix/bin`` (POSIX) —
       the venv-relative scripts dir for the *current* interpreter.

    Raises:
        FileNotFoundError: if no installation of the binary is found.

    """
    override = os.environ.get("DCC_MCP_SERVER_BIN")
    if override:
        p = Path(override).expanduser()
        if p.is_file():
            return p

    package_dir = Path(__file__).resolve().parent
    candidate = package_dir.parent / "scripts" / _BINARY_NAME
    if candidate.is_file():
        return candidate

    on_path = shutil.which("dcc-mcp-server")
    if on_path:
        return Path(on_path)

    scripts_dir = Path(sys.prefix) / ("Scripts" if os.name == "nt" else "bin")
    candidate = scripts_dir / _BINARY_NAME
    if candidate.is_file():
        return candidate

    raise FileNotFoundError(
        "dcc-mcp-server binary not found. Did you `pip install dcc-mcp-server`? "
        "You can also set the DCC_MCP_SERVER_BIN env var to an explicit path."
    )
