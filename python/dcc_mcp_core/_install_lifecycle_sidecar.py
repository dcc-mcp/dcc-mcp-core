"""Import-light per-DCC sidecar launch helpers.

This module intentionally uses only the Python standard library and never
imports :mod:`dcc_mcp_core._core`.
"""

from __future__ import annotations

import contextlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Tuple
from urllib.parse import urlsplit
from uuid import UUID

REGISTRY_ENV = "DCC_MCP_REGISTRY_DIR"
ROLE_PER_DCC_SIDECAR = "per-dcc-sidecar"
SUPPORTED_DISPATCH_HOST_RPC_SCHEMES = ("commandport", "qtserver", "ws", "wss")
TEST_ONLY_HOST_RPC_SCHEMES = ("stub",)
SERVER_BINARY_VERSION_TIMEOUT_SECS = 1.0


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
        "uri_valid": False,
        "validation_error": None,
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
        message = "host_rpc must include a URI scheme such as commandport://, qtserver://, ws://, or wss://."
        return {
            **base,
            "status": "invalid",
            "dispatch_ready_capable": False,
            "test_only": False,
            "reason": "missing_scheme",
            "validation_error": message,
            "message": message,
        }
    if scheme in SUPPORTED_DISPATCH_HOST_RPC_SCHEMES:
        validation_error = _dispatch_uri_validation_error(endpoint, scheme)
        if validation_error is not None:
            return {
                **base,
                "status": "invalid",
                "dispatch_ready_capable": False,
                "test_only": False,
                "reason": "invalid_host_rpc_uri",
                "validation_error": validation_error,
                "message": validation_error,
            }
        return {
            **base,
            "status": "dispatch_capable",
            "dispatch_ready_capable": True,
            "test_only": False,
            "uri_valid": True,
            "reason": None,
            "message": "The sidecar can become dispatch-ready once the DCC host RPC bridge accepts a connection.",
        }
    if scheme in TEST_ONLY_HOST_RPC_SCHEMES:
        return {
            **base,
            "status": "test_only",
            "dispatch_ready_capable": False,
            "test_only": True,
            "uri_valid": True,
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
    watch_pid: Optional[int] = None,
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
    be passed to ``subprocess.Popen`` without shell quoting. When called from an
    in-process DCC plugin, omit ``watch_pid`` to bind the sidecar to the current
    DCC process. CLI callers must pass ``--watch-pid`` explicitly because the
    CLI process is not the DCC host.
    """
    environment = _merged_env(env)
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

    pid = _parse_int(os.getpid() if watch_pid is None else watch_pid)
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

    normalized_instance_id = _normalize_instance_id(instance_id)
    if instance_id not in (None, "") and normalized_instance_id is None:
        return _failed(
            "invalid_instance_id",
            "instance_id must be a UUID accepted by dcc-mcp-server sidecar.",
        )

    registry_path = _to_path(registry_dir) or Path(default_registry_dir()).expanduser()
    server_binary = _server_binary_diagnostic(server_bin, environment)
    command = [
        server_binary["command"],
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
    _append_flag_value(command, "--instance-id", normalized_instance_id)
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
        "server_binary": server_binary,
        "environment": {"set": env_set},
        "readiness_selector": {
            "dcc_type": dcc,
            "instance_id": normalized_instance_id,
            "host_rpc": endpoint,
        },
        "readiness_argv": _build_readiness_argv(
            dcc_type=dcc,
            host_rpc=endpoint,
            registry_path=registry_path,
            instance_id=normalized_instance_id,
        ),
        "readiness_command": _build_readiness_command(
            environment,
            dcc_type=dcc,
            host_rpc=endpoint,
            registry_path=registry_path,
            instance_id=normalized_instance_id,
        ),
        "dispatch_contract": dispatch_contract,
        "readiness_contract": _sidecar_readiness_contract(dispatch_contract),
        "detached": True,
        "recommended_next_action": _sidecar_launch_next_action(dispatch_contract),
    }


def launch_sidecar(
    *,
    dcc_type: str,
    host_rpc: str,
    watch_pid: Optional[int] = None,
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
    stdio_log_dir: Optional[Any] = None,
    capture_stdio: bool = True,
    liveness_check_secs: float = 0.0,
    return_process: bool = False,
) -> Dict[str, Any]:
    """Start a per-DCC sidecar without importing native ``dcc_mcp_core``.

    By default the helper returns as soon as ``subprocess.Popen`` succeeds so
    DCC startup hooks do not block their host UI, while stdout/stderr are
    written to sidecar log files under the registry directory. Pass
    ``wait_ready_timeout_secs`` from a background startup task or installer when
    the caller wants a bounded dispatch-readiness verdict in the same result;
    pass ``liveness_check_secs`` when a supervisor needs immediate-exit
    detection after spawning. Pass ``return_process=True`` only from in-process
    supervisors that need to terminate the child later; CLI/JSON callers should
    keep the default because ``subprocess.Popen`` is not serializable.
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

    popen_env = _merged_env(env)
    popen_env.update(contract["environment"]["set"])
    try:
        stdio = _build_stdio_contract(
            contract,
            stdio_log_dir=stdio_log_dir,
            capture_stdio=capture_stdio,
        )
    except OSError as exc:
        failed = _failed("stdio_log_failed", str(exc))
        failed["command"] = contract["command"]
        failed["registry_dir"] = contract["registry_dir"]
        failed["server_binary"] = contract.get("server_binary")
        return failed

    kwargs: Dict[str, Any] = {
        "env": popen_env,
        "stdin": subprocess.DEVNULL,
        "stdout": stdio["stdout_handle"],
        "stderr": stdio["stderr_handle"],
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
        failed["registry_dir"] = contract["registry_dir"]
        failed["server_binary"] = contract.get("server_binary")
        failed["stdio"] = stdio["result"]
        return failed
    finally:
        for handle in stdio["close_after_spawn"]:
            with contextlib.suppress(OSError):
                handle.close()

    liveness = _check_early_liveness(proc, liveness_check_secs)

    result = {
        **contract,
        "success": liveness.get("alive") is not False,
        "status": "started" if liveness.get("alive") is not False else "exited",
        "pid": proc.pid,
        "detached": detached,
        "ready": False,
        "readiness_checked": False,
        "readiness": _unchecked_launch_readiness(contract),
        "stdio": stdio["result"],
        "liveness": liveness,
    }
    if return_process:
        result["process"] = proc
    if liveness.get("alive") is False:
        result["reason"] = "sidecar_exited_during_startup"
        result["message"] = "Sidecar process exited before the startup liveness check completed."
        return result
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
        result["readiness_checked"] = True
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


def _merged_env(env: Optional[Dict[str, str]]) -> Dict[str, str]:
    environment = dict(os.environ)
    if env:
        environment.update(env)
    return environment


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


def _normalize_instance_id(value: Optional[Any]) -> Optional[str]:
    if value in (None, ""):
        return None
    text = str(value).strip()
    if not text:
        return None
    try:
        return str(UUID(text))
    except (TypeError, ValueError):
        return None


def _resolve_server_bin(server_bin: Optional[str], env: Dict[str, str]) -> str:
    configured = _configured_server_bin(server_bin, env)
    if configured:
        return configured
    return shutil.which("dcc-mcp-server") or "dcc-mcp-server"


def _server_binary_diagnostic(server_bin: Optional[str], env: Dict[str, str]) -> Dict[str, Any]:
    explicit = str(server_bin or "").strip()
    env_configured = str(env.get("DCC_MCP_SERVER_BIN") or "").strip()
    configured = explicit or env_configured
    command = _resolve_server_bin(server_bin, env)
    resolved_path = shutil.which(command, path=env.get("PATH"))
    if resolved_path is None and Path(command).is_file():
        resolved_path = str(_to_path(command) or command)
    version, version_error = _probe_server_binary_version(command, env)
    source = "explicit" if explicit else ("env" if env_configured else "path")
    return {
        "command": command,
        "source": source,
        "configured": configured or None,
        "path": resolved_path,
        "version": version,
        "version_error": version_error,
    }


def _configured_server_bin(server_bin: Optional[str], env: Dict[str, str]) -> str:
    return str(server_bin or "").strip() or str(env.get("DCC_MCP_SERVER_BIN") or "").strip()


def _probe_server_binary_version(
    command: str,
    env: Dict[str, str],
) -> Tuple[Optional[str], Optional[str]]:
    try:
        result = subprocess.run(
            [command, "--version"],
            env=env,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=SERVER_BINARY_VERSION_TIMEOUT_SECS,
        )
    except Exception as exc:
        return None, f"running --version failed: {exc}"

    stdout = result.stdout.strip()
    stderr = result.stderr.strip()
    if result.returncode == 0:
        output = stdout or stderr
        return output or None, None if output else "--version produced no output"
    detail = stderr or stdout or f"exit code {result.returncode}"
    return None, detail


def _append_flag_value(command: List[str], flag: str, value: Optional[Any]) -> None:
    if value in (None, ""):
        return
    command.extend([flag, str(value)])


def _build_stdio_contract(
    contract: Dict[str, Any],
    *,
    stdio_log_dir: Optional[Any],
    capture_stdio: bool,
) -> Dict[str, Any]:
    if not capture_stdio:
        return {
            "stdout_handle": subprocess.DEVNULL,
            "stderr_handle": subprocess.DEVNULL,
            "close_after_spawn": [],
            "result": {
                "captured": False,
                "stdout_path": None,
                "stderr_path": None,
                "message": "Sidecar stdout/stderr are discarded.",
            },
        }

    log_dir = _to_path(stdio_log_dir) if stdio_log_dir is not None else None
    if log_dir is None:
        log_dir = Path(str(contract["registry_dir"])) / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    stem = _sidecar_log_stem(contract)
    stdout_path = log_dir / f"{stem}.stdout.log"
    stderr_path = log_dir / f"{stem}.stderr.log"
    stdout_handle = stdout_path.open("ab")
    try:
        stderr_handle = stderr_path.open("ab")
    except OSError:
        stdout_handle.close()
        raise
    return {
        "stdout_handle": stdout_handle,
        "stderr_handle": stderr_handle,
        "close_after_spawn": [stdout_handle, stderr_handle],
        "result": {
            "captured": True,
            "log_dir": str(log_dir),
            "stdout_path": str(stdout_path),
            "stderr_path": str(stderr_path),
            "message": "Sidecar stdout/stderr are written to log files.",
        },
    }


def _sidecar_log_stem(contract: Dict[str, Any]) -> str:
    selector = contract.get("readiness_selector") if isinstance(contract.get("readiness_selector"), dict) else {}
    identity = selector.get("instance_id") or contract.get("watch_pid") or "unknown"
    return "sidecar-{}-{}".format(_safe_log_token(contract.get("dcc_type")), _safe_log_token(identity))


def _safe_log_token(value: Any) -> str:
    text = str(value or "unknown").strip().lower()
    cleaned = []
    for char in text:
        if char.isalnum() or char in ("-", "_", "."):
            cleaned.append(char)
        else:
            cleaned.append("-")
    token = "".join(cleaned).strip("-._")
    return token or "unknown"


def _check_early_liveness(proc: Any, timeout_secs: float) -> Dict[str, Any]:
    timeout = max(0.0, float(timeout_secs or 0.0))
    if timeout <= 0:
        return {
            "checked": False,
            "alive": None,
            "timeout_secs": timeout,
            "exit_code": None,
            "message": "Startup liveness check was not requested.",
        }
    poll = getattr(proc, "poll", None)
    if not callable(poll):
        return {
            "checked": False,
            "alive": None,
            "timeout_secs": timeout,
            "exit_code": None,
            "message": "Process object does not expose poll(); liveness could not be checked.",
        }
    started = time.monotonic()
    deadline = started + timeout
    exit_code = poll()
    while exit_code is None and time.monotonic() < deadline:
        time.sleep(min(0.05, max(0.0, deadline - time.monotonic())))
        exit_code = poll()
    elapsed = round(time.monotonic() - started, 3)
    return {
        "checked": True,
        "alive": exit_code is None,
        "timeout_secs": timeout,
        "elapsed_secs": elapsed,
        "exit_code": exit_code,
        "message": (
            "Sidecar process remained alive through the startup liveness window."
            if exit_code is None
            else "Sidecar process exited during the startup liveness window."
        ),
    }


def _uri_scheme(value: Any) -> Optional[str]:
    text = str(value or "").strip()
    if "://" not in text:
        return None
    return text.split("://", 1)[0].lower()


def _dispatch_uri_validation_error(endpoint: str, scheme: str) -> Optional[str]:
    if scheme in ("commandport", "qtserver"):
        return _tcp_dispatch_uri_validation_error(endpoint, scheme)
    if scheme in ("ws", "wss"):
        return _websocket_dispatch_uri_validation_error(endpoint, scheme)
    return None


def _tcp_dispatch_uri_validation_error(endpoint: str, scheme: str) -> Optional[str]:
    try:
        parsed = urlsplit(endpoint)
    except ValueError as exc:
        return f"{scheme} host_rpc URI is invalid: {exc}."
    if parsed.scheme.lower() != scheme:
        return f"host_rpc must start with {scheme}://."
    if not parsed.hostname:
        return f"{scheme} host_rpc URI must include a host."
    try:
        port = parsed.port
    except ValueError as exc:
        return f"{scheme} host_rpc URI has an invalid port: {exc}."
    if port is None:
        return f"{scheme} host_rpc URI must include a non-zero port."
    if port <= 0:
        return f"{scheme} host_rpc URI port must be non-zero."
    if parsed.path or parsed.query or parsed.fragment:
        return f"{scheme} host_rpc URI must be host:port only; path, query, and fragment are not supported."
    return None


def _websocket_dispatch_uri_validation_error(endpoint: str, scheme: str) -> Optional[str]:
    try:
        parsed = urlsplit(endpoint)
    except ValueError as exc:
        return f"{scheme} host_rpc URI is invalid: {exc}."
    if parsed.scheme.lower() != scheme:
        return f"host_rpc must start with {scheme}://."
    if not parsed.hostname:
        return f"{scheme} host_rpc URI must include a host."
    try:
        port = parsed.port
    except ValueError as exc:
        return f"{scheme} host_rpc URI has an invalid port: {exc}."
    if port == 0:
        return f"{scheme} host_rpc URI port must be non-zero."
    return None


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


def _sidecar_readiness_contract(dispatch_contract: Dict[str, Any]) -> Dict[str, Any]:
    dispatch_capable = bool(dispatch_contract.get("dispatch_ready_capable"))
    direct_use_status = "requires_ready_verdict" if dispatch_capable else "diagnostics_only"
    message = (
        "Launching the sidecar only proves that a helper process was requested; "
        "tool calls are directly usable only after sidecar readiness reports ready."
        if dispatch_capable
        else (
            "Launching the sidecar with this host_rpc can publish diagnostics, but it cannot prove DCC tool dispatch."
        )
    )
    return {
        "ready_on_launch": False,
        "requires_readiness_check": True,
        "requires_dispatch_capable_host_rpc": True,
        "dispatch_ready_capable": dispatch_capable,
        "direct_use_status": direct_use_status,
        "ready_verdict": "sidecar_readiness_status(...).ready == true" if dispatch_capable else None,
        "message": message,
    }


def _unchecked_launch_readiness(contract: Dict[str, Any]) -> Dict[str, Any]:
    readiness_contract = contract.get("readiness_contract")
    if not isinstance(readiness_contract, dict):
        readiness_contract = _sidecar_readiness_contract(contract.get("dispatch_contract", {}))
    status = "not_checked" if readiness_contract.get("dispatch_ready_capable") else "dispatch_not_capable"
    return {
        "success": False,
        "status": status,
        "ready": False,
        "checked": False,
        "selector": contract.get("readiness_selector"),
        "message": readiness_contract.get("message"),
        "recommended_next_action": contract.get("recommended_next_action"),
    }


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
