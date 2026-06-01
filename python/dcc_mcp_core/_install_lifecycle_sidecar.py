"""Import-light per-DCC sidecar launch helpers.

This module intentionally uses only the Python standard library and never
imports :mod:`dcc_mcp_core._core`.
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"
ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"


def build_sidecar_command(
    *,
    dcc_type: str,
    host_rpc: str,
    watch_pid: int,
    registry_dir: Optional[Any] = None,
    server_bin: Optional[str] = None,
    instance_id: Optional[str] = None,
    display_name: Optional[str] = None,
    adapter_version: Optional[str] = None,
    gateway_port: Optional[int] = None,
    gateway_host: Optional[str] = None,
    gateway_name: Optional[str] = None,
    gateway_remote_host: Optional[str] = None,
    gateway_remote_port: Optional[int] = None,
    connect_timeout_secs: Optional[int] = None,
    no_ensure_gateway: bool = False,
    legacy_gateway_election: bool = False,
    extra_args: Optional[Iterable[Any]] = None,
    env: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    """Build an import-light ``dcc-mcp-server sidecar`` launch contract.

    DCC startup hooks can call this helper before importing any native
    ``dcc_mcp_core`` module. The returned ``command`` is an argv list that can
    be passed to ``subprocess.Popen`` without shell quoting.
    """
    environment = dict(os.environ if env is None else env)
    dcc = str(dcc_type or "").strip()
    if not dcc:
        return _failed("invalid_dcc_type", "dcc_type is required.")

    endpoint = str(host_rpc or "").strip()
    if not endpoint:
        return _failed("invalid_host_rpc", "host_rpc is required.")

    pid = _parse_int(watch_pid)
    if pid is None:
        return _failed("invalid_watch_pid", "watch_pid must be a positive process id.")

    port = _parse_port(
        gateway_port if gateway_port is not None else environment.get("DCC_MCP_GATEWAY_PORT"),
        default=9765,
    )
    if port is None:
        return _failed("invalid_gateway_port", "gateway_port must be between 0 and 65535.")

    remote_port = None
    if gateway_remote_port is not None:
        remote_port = _parse_port(gateway_remote_port, default=None)
        if remote_port is None:
            return _failed(
                "invalid_gateway_remote_port",
                "gateway_remote_port must be between 0 and 65535.",
            )

    registry_path = _to_path(registry_dir) or Path(default_registry_dir()).expanduser()
    command = [
        _resolve_server_bin(server_bin, environment),
        "sidecar",
        "--dcc",
        dcc,
        "--host-rpc",
        endpoint,
        "--watch-pid",
        str(pid),
        "--registry-dir",
        str(registry_path),
        "--gateway-port",
        str(port),
    ]
    _append_flag_value(command, "--instance-id", instance_id)
    _append_flag_value(command, "--display-name", display_name)
    _append_flag_value(command, "--adapter-version", adapter_version)
    _append_flag_value(command, "--gateway-host", gateway_host or environment.get("DCC_MCP_GATEWAY_HOST"))
    _append_flag_value(command, "--gateway-name", gateway_name or environment.get("DCC_MCP_GATEWAY_NAME"))
    _append_flag_value(command, "--gateway-remote-host", gateway_remote_host)
    if remote_port is not None:
        command.extend(["--gateway-remote-port", str(remote_port)])
    if connect_timeout_secs is not None:
        timeout = _parse_int(connect_timeout_secs)
        if timeout is None:
            return _failed(
                "invalid_connect_timeout_secs",
                "connect_timeout_secs must be a positive integer.",
            )
        command.extend(["--connect-timeout-secs", str(timeout)])
    if no_ensure_gateway:
        command.append("--no-ensure-gateway")
    if legacy_gateway_election:
        command.append("--legacy-gateway-election")
    if extra_args:
        command.extend(str(arg) for arg in extra_args)

    env_set = {
        REGISTRY_ENV: str(registry_path),
        "DCC_MCP_GATEWAY_PORT": str(port),
    }
    if gateway_host:
        env_set["DCC_MCP_GATEWAY_HOST"] = str(gateway_host)
    if gateway_name:
        env_set["DCC_MCP_GATEWAY_NAME"] = str(gateway_name)

    return {
        "success": True,
        "role": ROLE_PER_DCC_SIDECAR,
        "dcc_type": dcc,
        "host_rpc": endpoint,
        "watch_pid": pid,
        "registry_dir": str(registry_path),
        "gateway_port": port,
        "command": command,
        "environment": {"set": env_set},
        "detached": True,
        "recommended_next_action": (
            "Spawn this command from the DCC startup hook and keep using the shared gateway URL."
        ),
    }


def launch_sidecar(
    *,
    dcc_type: str,
    host_rpc: str,
    watch_pid: int,
    registry_dir: Optional[Any] = None,
    server_bin: Optional[str] = None,
    instance_id: Optional[str] = None,
    display_name: Optional[str] = None,
    adapter_version: Optional[str] = None,
    gateway_port: Optional[int] = None,
    gateway_host: Optional[str] = None,
    gateway_name: Optional[str] = None,
    gateway_remote_host: Optional[str] = None,
    gateway_remote_port: Optional[int] = None,
    connect_timeout_secs: Optional[int] = None,
    no_ensure_gateway: bool = False,
    legacy_gateway_election: bool = False,
    detached: bool = True,
    cwd: Optional[Any] = None,
    env: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    """Start a per-DCC sidecar without importing native ``dcc_mcp_core``."""
    contract = build_sidecar_command(
        dcc_type=dcc_type,
        host_rpc=host_rpc,
        watch_pid=watch_pid,
        registry_dir=registry_dir,
        server_bin=server_bin,
        instance_id=instance_id,
        display_name=display_name,
        adapter_version=adapter_version,
        gateway_port=gateway_port,
        gateway_host=gateway_host,
        gateway_name=gateway_name,
        gateway_remote_host=gateway_remote_host,
        gateway_remote_port=gateway_remote_port,
        connect_timeout_secs=connect_timeout_secs,
        no_ensure_gateway=no_ensure_gateway,
        legacy_gateway_election=legacy_gateway_election,
        env=env,
    )
    if not contract.get("success"):
        return contract

    popen_env = dict(os.environ if env is None else env)
    popen_env.update(contract["environment"]["set"])
    kwargs: Dict[str, Any] = {
        "env": popen_env,
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "close_fds": os.name != "nt",
    }
    if cwd is not None:
        kwargs["cwd"] = str(_to_path(cwd) or cwd)
    if detached and os.name == "nt":
        flags = 0
        flags |= getattr(subprocess, "DETACHED_PROCESS", 0)
        flags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        flags |= getattr(subprocess, "CREATE_NO_WINDOW", 0)
        kwargs["creationflags"] = flags

    try:
        proc = subprocess.Popen(contract["command"], **kwargs)
    except Exception as exc:
        failed = _failed("spawn_failed", str(exc))
        failed["command"] = contract["command"]
        return failed

    return {
        **contract,
        "success": True,
        "status": "started",
        "pid": proc.pid,
        "detached": detached,
    }


def default_registry_dir() -> str:
    """Return the shared FileRegistry directory without importing ``_core``."""
    return os.environ.get(REGISTRY_ENV) or str(Path(tempfile.gettempdir()) / "dcc-mcp-registry")


def _to_path(path: Any) -> Optional[Path]:
    if path in (None, ""):
        return None
    try:
        return Path(str(path)).expanduser().resolve()
    except OSError:
        return Path(str(path)).expanduser().absolute()


def _parse_int(value: Any) -> Optional[int]:
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed > 0 else None


def _parse_port(value: Any, *, default: Optional[int]) -> Optional[int]:
    if value in (None, ""):
        return default
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if 0 <= parsed <= 65535 else None


def _resolve_server_bin(server_bin: Optional[str], env: Dict[str, str]) -> str:
    explicit = str(server_bin or env.get("DCC_MCP_SERVER_BIN") or "").strip()
    if explicit:
        return explicit
    return shutil.which("dcc-mcp-server") or "dcc-mcp-server"


def _append_flag_value(command: List[str], flag: str, value: Optional[Any]) -> None:
    if value in (None, ""):
        return
    command.extend([flag, str(value)])


def _failed(reason: str, message: str) -> Dict[str, Any]:
    return {
        "success": False,
        "status": "failed",
        "requires_restart": False,
        "path": None,
        "reason": reason,
        "message": message,
    }
