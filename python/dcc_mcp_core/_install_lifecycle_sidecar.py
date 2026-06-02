"""Import-light per-DCC sidecar launch helpers.

This module intentionally uses only the Python standard library and never
imports :mod:`dcc_mcp_core._core`.
"""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"
ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"
SUPPORTED_DISPATCH_HOST_RPC_SCHEMES = ("commandport", "qtserver", "ws", "wss")
TEST_ONLY_HOST_RPC_SCHEMES = ("stub",)


def sidecar_host_rpc_dispatch_contract(host_rpc: Any) -> Dict[str, Any]:
    """Classify whether a sidecar host RPC URI can prove tool dispatch.

    The generic sidecar may still start for unsupported schemes so operators
    get a diagnostic registry row. Adapter startup code that wants to claim
    "open the DCC and tools are usable" should require a dispatch-capable
    scheme and then run a readiness/probe check.
    """
    endpoint = str(host_rpc or "").strip()
    scheme = _uri_scheme(endpoint)
    base = {
        "host_rpc": endpoint,
        "scheme": scheme,
        "supported_schemes": list(SUPPORTED_DISPATCH_HOST_RPC_SCHEMES),
        "test_only_schemes": list(TEST_ONLY_HOST_RPC_SCHEMES),
    }
    if not endpoint:
        return {
            **base,
            "status": "invalid",
            "dispatch_ready_capable": False,
            "test_only": False,
            "reason": "missing_host_rpc",
            "message": "host_rpc is required before sidecar dispatch can be proven.",
        }
    if scheme is None:
        return {
            **base,
            "status": "invalid",
            "dispatch_ready_capable": False,
            "test_only": False,
            "reason": "missing_scheme",
            "message": "host_rpc must include a URI scheme such as commandport://, qtserver://, ws://, or wss://.",
        }
    if scheme in SUPPORTED_DISPATCH_HOST_RPC_SCHEMES:
        return {
            **base,
            "status": "dispatch_capable",
            "dispatch_ready_capable": True,
            "test_only": False,
            "reason": None,
            "message": "The sidecar can become dispatch-ready once the DCC host RPC bridge accepts a connection.",
        }
    if scheme in TEST_ONLY_HOST_RPC_SCHEMES:
        return {
            **base,
            "status": "test_only",
            "dispatch_ready_capable": False,
            "test_only": True,
            "reason": "test_only_host_rpc",
            "message": "stub:// is test-only and must not be used as adapter startup proof.",
        }
    return {
        **base,
        "status": "unsupported",
        "dispatch_ready_capable": False,
        "test_only": False,
        "reason": "unsupported_host_rpc_scheme",
        "message": (
            "No generic sidecar HostRpcClient is registered for this scheme; "
            "the sidecar can register for diagnostics but cannot prove tool dispatch."
        ),
    }


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
    require_dispatch_capable: bool = False,
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
    dispatch_contract = sidecar_host_rpc_dispatch_contract(endpoint)
    if require_dispatch_capable and not dispatch_contract["dispatch_ready_capable"]:
        failed = _failed(
            "dispatch_not_capable",
            (
                "host_rpc is not dispatch-capable for a production sidecar launch. "
                "Use commandport://, qtserver://, ws://, or wss://, or disable "
                "require_dispatch_capable for diagnostics-only launches."
            ),
        )
        failed["dispatch_contract"] = dispatch_contract
        return failed

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
        "readiness_selector": {
            "dcc_type": dcc,
            "instance_id": instance_id,
            "host_rpc": endpoint,
        },
        "readiness_argv": _build_readiness_argv(
            dcc_type=dcc,
            host_rpc=endpoint,
            registry_path=registry_path,
            instance_id=instance_id,
        ),
        "readiness_command": _build_readiness_command(
            environment,
            dcc_type=dcc,
            host_rpc=endpoint,
            registry_path=registry_path,
            instance_id=instance_id,
        ),
        "dispatch_contract": dispatch_contract,
        "detached": True,
        "recommended_next_action": _sidecar_launch_next_action(dispatch_contract),
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
    require_dispatch_capable: bool = False,
    extra_args: Optional[Iterable[Any]] = None,
    detached: bool = True,
    cwd: Optional[Any] = None,
    env: Optional[Dict[str, str]] = None,
    wait_ready_timeout_secs: Optional[float] = None,
    poll_interval_secs: float = 0.25,
    probe_tool: Optional[str] = None,
    probe_arguments: Optional[Dict[str, Any]] = None,
    probe_timeout_secs: float = 3.0,
) -> Dict[str, Any]:
    """Start a per-DCC sidecar without importing native ``dcc_mcp_core``.

    By default the helper returns as soon as ``subprocess.Popen`` succeeds so
    DCC startup hooks do not block their host UI. Pass
    ``wait_ready_timeout_secs`` from a background startup task or installer when
    the caller wants a bounded dispatch-readiness verdict in the same result.
    """
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
        require_dispatch_capable=require_dispatch_capable,
        extra_args=extra_args,
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

    result = {
        **contract,
        "success": True,
        "status": "started",
        "pid": proc.pid,
        "detached": detached,
    }
    if wait_ready_timeout_secs is not None:
        result["readiness"] = _check_launch_readiness(
            registry_dir=contract["registry_dir"],
            dcc_type=contract["dcc_type"],
            instance_id=contract.get("readiness_selector", {}).get("instance_id"),
            host_rpc=contract["host_rpc"],
            timeout_secs=wait_ready_timeout_secs,
            poll_interval_secs=poll_interval_secs,
            probe_tool=probe_tool,
            probe_arguments=probe_arguments,
            probe_timeout_secs=probe_timeout_secs,
        )
        result["ready"] = bool(result["readiness"].get("ready"))
    return result


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


def _uri_scheme(value: Any) -> Optional[str]:
    text = str(value or "").strip()
    if "://" not in text:
        return None
    return text.split("://", 1)[0].lower()


def _sidecar_launch_next_action(dispatch_contract: Dict[str, Any]) -> str:
    if dispatch_contract.get("dispatch_ready_capable"):
        return (
            "Spawn this command from the DCC startup hook; use readiness_command "
            "or wait_for_sidecar_ready() before claiming tools are callable."
        )
    return (
        "This sidecar launch can register a diagnostic row, but it cannot prove "
        "DCC tool dispatch with the configured host_rpc. Use a supported real "
        "host RPC scheme before claiming the plugin is directly usable."
    )


def _build_readiness_argv(
    *,
    dcc_type: str,
    host_rpc: str,
    registry_path: Path,
    instance_id: Optional[str],
) -> List[str]:
    command = [
        "sidecar-ready",
        "--dcc",
        dcc_type,
        "--host-rpc",
        host_rpc,
        "--registry-dir",
        str(registry_path),
    ]
    _append_flag_value(command, "--instance-id", instance_id)
    return command


def _build_readiness_command(
    env: Dict[str, str],
    *,
    dcc_type: str,
    host_rpc: str,
    registry_path: Path,
    instance_id: Optional[str],
) -> List[str]:
    python_bin = str(env.get("DCC_MCP_PYTHON_EXECUTABLE") or sys.executable)
    return [
        python_bin,
        "-m",
        "dcc_mcp_core.install_lifecycle",
        *_build_readiness_argv(
            dcc_type=dcc_type,
            host_rpc=host_rpc,
            registry_path=registry_path,
            instance_id=instance_id,
        ),
    ]


def _check_launch_readiness(
    *,
    registry_dir: str,
    dcc_type: str,
    instance_id: Optional[str],
    host_rpc: str,
    timeout_secs: float,
    poll_interval_secs: float,
    probe_tool: Optional[str],
    probe_arguments: Optional[Dict[str, Any]],
    probe_timeout_secs: float,
) -> Dict[str, Any]:
    from ._install_lifecycle_readiness import sidecar_readiness_status
    from ._install_lifecycle_readiness import wait_for_sidecar_ready

    timeout = max(0.0, float(timeout_secs))
    if timeout > 0:
        return wait_for_sidecar_ready(
            registry_dir,
            dcc_type=dcc_type,
            instance_id=instance_id,
            host_rpc=host_rpc,
            timeout_secs=timeout,
            poll_interval_secs=poll_interval_secs,
            probe_tool=probe_tool,
            probe_arguments=probe_arguments,
            probe_timeout_secs=probe_timeout_secs,
        )
    return sidecar_readiness_status(
        registry_dir,
        dcc_type=dcc_type,
        instance_id=instance_id,
        host_rpc=host_rpc,
        probe_tool=probe_tool,
        probe_arguments=probe_arguments,
        probe_timeout_secs=probe_timeout_secs,
    )


def _failed(reason: str, message: str) -> Dict[str, Any]:
    return {
        "success": False,
        "status": "failed",
        "requires_restart": False,
        "path": None,
        "reason": reason,
        "message": message,
    }
