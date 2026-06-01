"""Best-effort standalone gateway bootstrap for embedded Python adapters."""

from __future__ import annotations

import contextlib
import os
from pathlib import Path
import shutil
import subprocess
import threading
import time
from typing import Any
from typing import Callable
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import urlopen

from dcc_mcp_core.install_lifecycle import default_registry_dir

_LAUNCH_LOCK = "gateway-launch.lock"


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


def _resolve_registry_dir(registry_dir: str | None) -> Path:
    if registry_dir:
        return Path(registry_dir).expanduser()
    return Path(default_registry_dir()).expanduser()


class _LaunchLock:
    def __init__(self, path: Path) -> None:
        self.path = path
        self._fd: int | None = None

    def acquire(self) -> bool:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        try:
            self._fd = os.open(str(self.path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            return False
        return True

    def release(self) -> None:
        if self._fd is not None:
            os.close(self._fd)
            self._fd = None
        with contextlib.suppress(FileNotFoundError):
            self.path.unlink()


def _wait_gateway_ready(host: str, port: int, *, timeout_secs: float, probe_timeout: float = 0.5) -> bool:
    deadline = time.time() + max(timeout_secs, 0.2)
    while time.time() < deadline:
        if _is_healthy(host, port, timeout=probe_timeout):
            return True
        time.sleep(0.1)
    return False


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

    registry_path = _resolve_registry_dir(registry_dir)
    launch_lock = _LaunchLock(registry_path / _LAUNCH_LOCK)
    try:
        acquired = launch_lock.acquire()
    except OSError as exc:
        return {"ok": False, "reason": "launch_lock_failed", "error": str(exc)}

    if not acquired:
        if _wait_gateway_ready(gateway_host, gateway_port, timeout_secs=timeout_secs):
            return {"ok": True, "reason": "launch_in_progress", "registry_dir": str(registry_path)}
        return {
            "ok": False,
            "reason": "launch_in_progress_timeout",
            "registry_dir": str(registry_path),
        }

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
    env["DCC_MCP_REGISTRY_DIR"] = str(registry_path)
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
        try:
            if _is_healthy(gateway_host, gateway_port, timeout=0.5):
                return {"ok": True, "reason": "already_healthy", "registry_dir": str(registry_path)}
            subprocess.Popen(cmd, **kwargs)
        except Exception as exc:
            return {"ok": False, "reason": "spawn_failed", "error": str(exc), "command": cmd}

        if _wait_gateway_ready(gateway_host, gateway_port, timeout_secs=timeout_secs):
            return {"ok": True, "reason": "spawned", "command": cmd, "registry_dir": str(registry_path)}

        return {"ok": False, "reason": "spawn_timeout", "command": cmd, "registry_dir": str(registry_path)}
    finally:
        launch_lock.release()


class GatewayDaemonGuardian:
    """Background guardian that re-ensures the standalone gateway after crashes."""

    def __init__(
        self,
        *,
        gateway_host: str,
        gateway_port: int,
        registry_dir: str | None,
        dcc_type: str,
        probe_interval_secs: float | None = None,
        probe_timeout_secs: float | None = None,
        restart_timeout_secs: float | None = None,
        failure_threshold: int | None = None,
        status_callback: Callable[[dict[str, Any]], None] | None = None,
    ) -> None:
        self.gateway_host = gateway_host
        self.gateway_port = gateway_port
        self.registry_dir = registry_dir
        self.dcc_type = dcc_type
        self.probe_interval_secs = probe_interval_secs or _float_env(
            "DCC_MCP_GATEWAY_GUARDIAN_INTERVAL",
            5.0,
        )
        self.probe_timeout_secs = probe_timeout_secs or _float_env(
            "DCC_MCP_GATEWAY_GUARDIAN_TIMEOUT",
            0.5,
        )
        self.restart_timeout_secs = restart_timeout_secs or _float_env(
            "DCC_MCP_GATEWAY_GUARDIAN_RESTART_TIMEOUT",
            5.0,
        )
        self.failure_threshold = max(
            1,
            failure_threshold or _int_env("DCC_MCP_GATEWAY_GUARDIAN_FAILURES", 2),
        )
        self.status_callback = status_callback
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._lock = threading.Lock()
        self._consecutive_failures = 0
        self._restart_attempts = 0
        self._last_status: dict[str, Any] = {
            "ok": False,
            "reason": "not_started",
            "guardian_running": False,
            "consecutive_failures": 0,
            "restart_attempts": 0,
            "gateway_host": gateway_host,
            "gateway_port": gateway_port,
        }

    def start(self) -> bool:
        if self.gateway_port <= 0:
            self._publish({"ok": False, "reason": "gateway_port_not_configured"})
            return False
        if self._thread is not None and self._thread.is_alive():
            return True
        self._stop.clear()
        self._thread = threading.Thread(
            target=self._run,
            name=f"dcc-mcp-gateway-guardian-{self.dcc_type}",
            daemon=True,
        )
        self._thread.start()
        self._publish({"ok": True, "reason": "guardian_started", "guardian_running": True})
        return True

    def stop(self, timeout: float = 1.0) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=max(timeout, 0.0))
            self._thread = None
        self._publish({"ok": True, "reason": "guardian_stopped", "guardian_running": False})

    def status(self) -> dict[str, Any]:
        with self._lock:
            status = dict(self._last_status)
        status["guardian_running"] = bool(self._thread is not None and self._thread.is_alive())
        return status

    def probe_once(self) -> dict[str, Any]:
        if self.gateway_port <= 0:
            return self._publish({"ok": False, "reason": "gateway_port_not_configured"})

        if _is_healthy(self.gateway_host, self.gateway_port, timeout=self.probe_timeout_secs):
            self._consecutive_failures = 0
            return self._publish({"ok": True, "reason": "healthy", "consecutive_failures": 0})

        self._consecutive_failures += 1
        if self._consecutive_failures < self.failure_threshold:
            return self._publish(
                {
                    "ok": False,
                    "reason": "probe_failed",
                    "consecutive_failures": self._consecutive_failures,
                }
            )

        self._restart_attempts += 1
        result = ensure_gateway_daemon(
            gateway_host=self.gateway_host,
            gateway_port=self.gateway_port,
            registry_dir=self.registry_dir,
            dcc_type=self.dcc_type,
            timeout_secs=self.restart_timeout_secs,
        )
        if result.get("ok"):
            self._consecutive_failures = 0
        return self._publish(result)

    def _run(self) -> None:
        while not self._stop.wait(max(self.probe_interval_secs, 0.1)):
            self.probe_once()

    def _publish(self, update: dict[str, Any]) -> dict[str, Any]:
        payload = {
            "gateway_host": self.gateway_host,
            "gateway_port": self.gateway_port,
            "guardian_running": bool(self._thread is not None and self._thread.is_alive()),
            "consecutive_failures": self._consecutive_failures,
            "restart_attempts": self._restart_attempts,
            "timestamp_ms": int(time.time() * 1000),
            **update,
        }
        with self._lock:
            self._last_status = payload
        if self.status_callback is not None:
            with contextlib.suppress(Exception):
                self.status_callback(dict(payload))
        return payload


def _float_env(name: str, default: float) -> float:
    try:
        return max(float(os.environ.get(name, "") or default), 0.1)
    except ValueError:
        return default


def _int_env(name: str, default: int) -> int:
    try:
        return int(os.environ.get(name, "") or default)
    except ValueError:
        return default
