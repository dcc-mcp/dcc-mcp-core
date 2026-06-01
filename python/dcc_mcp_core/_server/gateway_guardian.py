"""Best-effort standalone gateway bootstrap for embedded Python adapters."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import time
from typing import Any
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import urlopen


def _is_healthy(host: str, port: int, timeout: float) -> bool:
    url = f"http://{host}:{port}/health"
    try:
        with urlopen(url, timeout=timeout) as resp:
            return int(getattr(resp, "status", 0)) == 200
    except HTTPError as err:
        return int(getattr(err, "code", 0)) == 200
    except (URLError, OSError, ValueError):
        return False


def _resolve_server_bin() -> str:
    explicit = (os.environ.get("DCC_MCP_SERVER_BIN") or "").strip()
    if explicit:
        return explicit
    found = shutil.which("dcc-mcp-server")
    return found or "dcc-mcp-server"


def ensure_gateway_daemon(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str | None,
    dcc_type: str,
    timeout_secs: float = 5.0,
) -> dict[str, Any]:
    """Ensure a machine-wide gateway daemon is healthy on ``gateway_port``."""
    if gateway_port <= 0:
        return {"ok": False, "reason": "gateway_port_not_configured"}
    if _is_healthy(gateway_host, gateway_port, timeout=0.5):
        return {"ok": True, "reason": "already_healthy"}

    exe = _resolve_server_bin()
    cmd = [
        exe,
        "gateway",
        "--host",
        gateway_host,
        "--port",
        str(gateway_port),
    ]

    env = os.environ.copy()
    if not env.get("DCC_MCP_GATEWAY_PORT"):
        env["DCC_MCP_GATEWAY_PORT"] = str(gateway_port)
    if registry_dir:
        env["DCC_MCP_REGISTRY_DIR"] = str(registry_dir)
    if dcc_type and not env.get("DCC_MCP_DCC_TYPE"):
        env["DCC_MCP_DCC_TYPE"] = dcc_type

    kwargs: dict[str, Any] = {
        "env": env,
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "cwd": str(Path.cwd()),
        "close_fds": os.name != "nt",
    }
    if os.name == "nt":
        flags = 0
        flags |= getattr(subprocess, "DETACHED_PROCESS", 0)
        flags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        flags |= getattr(subprocess, "CREATE_NO_WINDOW", 0)
        kwargs["creationflags"] = flags

    try:
        subprocess.Popen(cmd, **kwargs)
    except Exception as exc:
        return {"ok": False, "reason": "spawn_failed", "error": str(exc), "command": cmd}

    deadline = time.time() + max(timeout_secs, 0.2)
    while time.time() < deadline:
        if _is_healthy(gateway_host, gateway_port, timeout=0.5):
            return {"ok": True, "reason": "spawned", "command": cmd}
        time.sleep(0.1)

    return {"ok": False, "reason": "spawn_timeout", "command": cmd}
