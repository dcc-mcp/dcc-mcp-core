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
import sysconfig

__all__ = ["__version__", "binary_path"]

# Kept in sync with pyproject.toml / Rust crate via release-please.
__version__ = "0.20.51"  # x-release-please-version

_BINARY_NAME = "dcc-mcp-server.exe" if os.name == "nt" else "dcc-mcp-server"


def _user_scheme() -> str:
    """Use the installer's user scheme, including on Python 3.7-3.9."""
    preferred = getattr(sysconfig, "get_preferred_scheme", None)
    if preferred is not None:
        return preferred("user")
    if os.name == "nt":
        return "nt_user"
    if sys.platform == "darwin" and sysconfig.get_config_var("PYTHONFRAMEWORK"):
        return "osx_framework_user"
    return "posix_user"


def binary_path() -> Path:
    """Return the filesystem path of the bundled ``dcc-mcp-server`` binary.

    Resolution order:

    1. ``DCC_MCP_SERVER_BIN`` env var — operator override.
    2. ``scripts/`` directory next to this package's installation (where
       maturin places ``bindings = "bin"`` artefacts inside the wheel).
    3. ``sys.prefix/Scripts`` (Windows) or ``sys.prefix/bin`` (POSIX) —
       the scripts directory for the *current* interpreter environment.
    4. ``sysconfig`` scripts directories for the current interpreter and
       its user-install scheme (including ``PYTHONUSERBASE``).
    5. ``shutil.which("dcc-mcp-server")`` — a final system PATH fallback.

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

    scripts_dir = Path(sys.prefix) / ("Scripts" if os.name == "nt" else "bin")
    candidate = scripts_dir / _BINARY_NAME
    if candidate.is_file():
        return candidate

    # pip relocates wheel data/scripts into the selected installation scheme.
    # In particular, --user scripts need not be on a DCC host's PATH.
    for user in (False, True):
        try:
            scripts = sysconfig.get_path("scripts", scheme=_user_scheme()) if user else sysconfig.get_path("scripts")
        except (AttributeError, ImportError, KeyError, OSError, ValueError):
            # Embedded interpreters can omit schemes or build configuration.
            continue
        if scripts:
            candidate = Path(scripts) / _BINARY_NAME
            if candidate.is_file() and (os.name == "nt" or os.access(candidate, os.X_OK)):
                return candidate

    on_path = shutil.which("dcc-mcp-server")
    if on_path:
        return Path(on_path)

    raise FileNotFoundError(
        "dcc-mcp-server binary not found. Did you `pip install dcc-mcp-server`? "
        "You can also set the DCC_MCP_SERVER_BIN env var to an explicit path."
    )
