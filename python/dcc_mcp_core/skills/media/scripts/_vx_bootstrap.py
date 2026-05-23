"""Install vx with the official installer scripts for the media skill."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Callable
from typing import List
from typing import Optional
from typing import Tuple

VX_INSTALL_SH_URL = "https://raw.githubusercontent.com/loonghao/vx/main/install.sh"
VX_INSTALL_PS1_URL = "https://raw.githubusercontent.com/loonghao/vx/main/install.ps1"
VX_INSTALL_TIMEOUT_SECS = 600

ErrorFactory = Callable[..., Exception]


def auto_install_vx_enabled() -> bool:
    value = os.environ.get("DCC_MCP_MEDIA_AUTO_INSTALL_VX", "1").strip().lower()
    return value not in {"0", "false", "no", "off"}


def is_default_vx_command(command: Tuple[str, ...]) -> bool:
    if not command or os.environ.get("DCC_MCP_MEDIA_VX_BIN"):
        return False
    return Path(command[0]).name.lower() in {"vx", "vx.exe"}


def download_and_install_vx(error_cls: ErrorFactory) -> str:
    """Run the official vx installer and return an executable path."""
    existing = find_vx()
    if existing:
        return existing

    command = installer_command(error_cls)
    try:
        result = subprocess.run(
            command,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=VX_INSTALL_TIMEOUT_SECS,
        )
    except FileNotFoundError as exc:
        raise error_cls(
            "Cannot run the vx installer because a required shell is missing.",
            "vx_bootstrap_failed",
            prompt="Install vx manually or set DCC_MCP_MEDIA_VX_BIN to a known executable.",
            context={"detail": str(exc), "command": command},
        ) from exc
    except subprocess.TimeoutExpired as exc:
        raise error_cls(
            "vx installer timed out.",
            "vx_bootstrap_failed",
            context={
                "timeout_secs": VX_INSTALL_TIMEOUT_SECS,
                "command": command,
                "stdout": exc.stdout,
                "stderr": exc.stderr,
            },
        ) from exc

    if result.returncode != 0:
        raise error_cls(
            "vx installer failed.",
            "vx_bootstrap_failed",
            prompt="Install vx manually or set DCC_MCP_MEDIA_VX_BIN to a known executable.",
            context={
                "returncode": result.returncode,
                "stdout": (result.stdout or "")[-1200:],
                "stderr": (result.stderr or "")[-1200:],
                "command": command,
            },
        )

    installed = find_vx()
    if installed:
        return installed
    raise error_cls(
        "vx installer completed but the vx executable was not found.",
        "vx_bootstrap_failed",
        prompt="Restart the terminal, install vx manually, or set DCC_MCP_MEDIA_VX_BIN.",
        context={
            "expected_install_dir": str(default_install_dir()),
            "stdout": (result.stdout or "")[-1200:],
            "stderr": (result.stderr or "")[-1200:],
            "command": command,
        },
    )


def installer_command(error_cls: ErrorFactory) -> List[str]:
    """Return the official vx installer command for this platform."""
    if sys.platform == "win32":
        return [
            "powershell",
            "-c",
            f"irm {VX_INSTALL_PS1_URL} | iex",
        ]
    if sys.platform == "darwin" or sys.platform.startswith("linux"):
        return ["bash", "-lc", f"curl -fsSL {VX_INSTALL_SH_URL} | bash"]
    raise error_cls(
        "Automatic vx bootstrap is not supported on this platform.",
        "vx_bootstrap_unsupported",
        context={"platform": sys.platform},
    )


def find_vx() -> Optional[str]:
    path = shutil.which("vx")
    if path:
        return path

    install_dir = default_install_dir()
    _prepend_path(install_dir)
    candidate = install_dir / ("vx.exe" if sys.platform == "win32" else "vx")
    if candidate.is_file():
        return str(candidate)

    path = shutil.which("vx")
    if path:
        return path
    return None


def default_install_dir() -> Path:
    override = os.environ.get("VX_INSTALL_DIR")
    if override:
        return Path(override).expanduser()
    if sys.platform == "win32":
        home = os.environ.get("USERPROFILE") or str(Path.home())
        return Path(home) / ".local" / "bin"
    return Path.home() / ".local" / "bin"


def _prepend_path(directory: Path) -> None:
    text = str(directory)
    current = os.environ.get("PATH", "")
    entries = [item for item in current.split(os.pathsep) if item]
    if text not in entries:
        os.environ["PATH"] = text + os.pathsep + current if current else text
