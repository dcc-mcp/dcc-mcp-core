"""Best-effort standalone gateway bootstrap for embedded Python adapters."""

from __future__ import annotations

import contextlib
from http.client import HTTPException
import json
import logging
import os
from pathlib import Path
import random  # noqa: F401  (tests patch ``gg.random.uniform``)
import time
from typing import Any
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import Request
from urllib.request import urlopen

from dcc_mcp_core._server._gateway_guardian_patrol import _ENSURE_TIMEOUT_DEFAULT

# Re-exported so ``dcc_mcp_core._server.gateway_guardian`` stays the public
# import location for the guardian after the patrol loop moved out.
from dcc_mcp_core._server._gateway_guardian_patrol import GatewayDaemonGuardian  # noqa: F401
from dcc_mcp_core._server._gateway_registry import _read_gateway_version_from_registry
from dcc_mcp_core._server._gateway_registry import _read_managed_gateway_version_from_registry
from dcc_mcp_core._server._gateway_registry import _resolve_registry_dir
from dcc_mcp_core._server._gateway_registry import _write_sentinel_entry
from dcc_mcp_core._server._gateway_server_bin import _get_core_version
from dcc_mcp_core._server._gateway_server_bin import _resolve_server_bin
from dcc_mcp_core._version_util import parse_semver as _parse_semver
from dcc_mcp_core.constants import ENV_DCC_TYPE
from dcc_mcp_core.constants import ENV_GATEWAY_ENSURE_TIMEOUT_SECS
from dcc_mcp_core.constants import ENV_GATEWAY_IDLE_TIMEOUT_SECS
from dcc_mcp_core.constants import ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS
from dcc_mcp_core.constants import ENV_GATEWAY_PERSIST
from dcc_mcp_core.constants import ENV_GATEWAY_PORT
from dcc_mcp_core.constants import ENV_REGISTRY_DIR
from dcc_mcp_core.daemon_launch import launch_detached
from dcc_mcp_core.env import env_float

logger = logging.getLogger(__name__)

_LAUNCH_LOCK = "gateway-launch.lock"
_LAUNCH_LOCK_STALE_SECS_DEFAULT = 30.0
_AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_DEFAULT = 300


def _is_healthy(host: str, port: int, timeout: float) -> bool:
    url = f"http://{host}:{port}/health"
    try:
        with urlopen(url, timeout=timeout) as resp:
            return int(getattr(resp, "status", 0)) == 200
    except HTTPError as err:
        return int(getattr(err, "code", 0)) == 200
    except (HTTPException, URLError, OSError, ValueError):
        return False


def _is_application_ready(host: str, port: int, timeout: float) -> bool:
    """Probe application readiness with a bounded legacy fallback."""
    url = f"http://{host}:{port}/v1/readyz"
    try:
        with urlopen(url, timeout=timeout) as resp:
            status = int(getattr(resp, "status", 0))
            if status == 404:
                # Pre-readiness gateways expose only /health.
                return _is_healthy(host, port, timeout)
            if status != 200:
                return False
            reader = getattr(resp, "read", None)
            if not callable(reader):
                return False
            raw = reader()
            if not raw:
                return False
            payload = json.loads(raw.decode("utf-8"))
            return isinstance(payload, dict) and payload.get("ok") is True
    except HTTPError as err:
        if int(getattr(err, "code", 0)) == 404:
            # Pre-readiness gateways expose only /health.
            return _is_healthy(host, port, timeout)
        return False
    except (HTTPException, URLError, OSError, ValueError, TypeError, UnicodeDecodeError, AttributeError):
        # A transport or malformed readiness response is not proof of liveness.
        return False


def _read_gateway_version_from_admin_health(
    gateway_host: str,
    gateway_port: int,
    *,
    timeout: float = 0.5,
) -> str | None:
    """Best-effort version probe from the running gateway's admin health API."""
    url = f"http://{gateway_host}:{gateway_port}/admin/api/health"
    try:
        with urlopen(url, timeout=timeout) as resp:
            if int(getattr(resp, "status", 0)) != 200:
                return None
            raw = resp.read()
        payload = json.loads(raw.decode("utf-8")) if raw else {}
    except (HTTPError, URLError, OSError, ValueError, AttributeError, UnicodeDecodeError):
        return None

    if not isinstance(payload, dict):
        return None
    version = payload.get("version")
    if isinstance(version, str) and version.strip():
        return version.strip()
    gateway = payload.get("gateway")
    if isinstance(gateway, dict):
        current = gateway.get("current")
        if isinstance(current, dict):
            version = current.get("version")
            if isinstance(version, str) and version.strip():
                return version.strip()
    return None


def _request_gateway_yield(
    gateway_host: str,
    gateway_port: int,
    *,
    challenger_version: str,
    reason: str,
    timeout: float = 1.0,
) -> bool:
    """Ask a running gateway to voluntarily release its port."""
    url = f"http://{gateway_host}:{gateway_port}/gateway/yield"
    body = json.dumps(
        {
            "challenger_version": challenger_version,
            "reason": reason,
            "suggested_successor": "python-gateway-guardian",
        }
    ).encode("utf-8")
    req = Request(
        url,
        data=body,
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    try:
        with urlopen(req, timeout=timeout) as resp:
            status = int(getattr(resp, "status", 0))
            return 200 <= status < 300
    except HTTPError as err:
        return 200 <= int(getattr(err, "code", 0)) < 300
    except (URLError, OSError, ValueError):
        return False


class _LaunchLock:
    """Cross-process launch lock for the standalone gateway.

    The ``acquire`` method consolidates stale check + delete + retry-create
    into a single flat attempt (aligned with the Rust sidecar
    ``acquire_launch_lock_with_stale``), minimising the TOCTOU windows that
    exist between check-and-delete and delete-and-retry-create.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        self._fd: int | None = None

    def acquire(self) -> bool:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        stale_after = _launch_lock_stale_secs()
        try:
            self._fd = os.open(str(self.path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        except FileExistsError:
            if not _remove_stale_launch_lock(self.path, stale_after):
                return False
            # Immediately retry after stale lock removal to minimise the
            # delete-and-retry-create TOCTOU window.
            try:
                self._fd = os.open(str(self.path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            except FileExistsError:
                return False
        # Write metadata into the lock file.
        with contextlib.suppress(OSError):
            os.write(self._fd, f"pid={os.getpid()} ts={int(time.time())}\n".encode("ascii"))
        return True

    def release(self) -> None:
        if self._fd is not None:
            os.close(self._fd)
            self._fd = None
        with contextlib.suppress(FileNotFoundError):
            self.path.unlink()


def _launch_lock_stale_secs() -> float:
    return env_float(ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS, _LAUNCH_LOCK_STALE_SECS_DEFAULT, minimum=0.1)


def _remove_stale_launch_lock(path: Path, stale_after_secs: float) -> bool:
    try:
        stat = path.stat()
    except FileNotFoundError:
        return True
    except OSError:
        return False

    age_secs = time.time() - stat.st_mtime
    if age_secs < stale_after_secs:
        return False

    # Re-check immediately before unlinking so a newly recreated fresh lock is
    # less likely to be removed after another process wins the launch race.
    try:
        stat = path.stat()
    except FileNotFoundError:
        return True
    except OSError:
        return False

    age_secs = time.time() - stat.st_mtime
    if age_secs < stale_after_secs:
        return False

    try:
        path.unlink()
    except FileNotFoundError:
        return True
    except OSError:
        return False
    return True


def _wait_gateway_ready(host: str, port: int, *, timeout_secs: float, probe_timeout: float = 0.5) -> bool:
    deadline = time.time() + max(timeout_secs, 0.2)
    while time.time() < deadline:
        if _is_application_ready(host, port, timeout=probe_timeout):
            return True
        time.sleep(0.1)
    return False


def _wait_managed_gateway_ready(
    host: str,
    port: int,
    *,
    registry_dir: str | None,
    minimum_version: str,
    timeout_secs: float,
    probe_timeout: float = 0.5,
) -> bool:
    """Wait for a healthy gateway with a formal ownership sentinel.

    A healthy port alone is insufficient during takeover: an older guardian
    can win the bind race and make the endpoint look ready. The Rust gateway
    sentinel includes process ownership fields that the temporary Python
    challenger row deliberately lacks.
    """
    deadline = time.time() + max(timeout_secs, 0.2)
    while time.time() < deadline:
        managed_version = _read_managed_gateway_version_from_registry(
            registry_dir,
            gateway_host=host,
            gateway_port=port,
        )
        version_is_acceptable = managed_version is not None and not _is_newer_version(
            minimum_version,
            managed_version,
        )
        if version_is_acceptable and _is_application_ready(host, port, timeout=probe_timeout):
            return True
        time.sleep(0.1)
    return False


def _resolve_gateway_persist(gateway_persist: bool | None) -> bool:
    if gateway_persist is not None:
        return bool(gateway_persist)
    return (os.environ.get(ENV_GATEWAY_PERSIST) or "").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }


def _resolve_gateway_idle_timeout_secs(gateway_idle_timeout_secs: int | None) -> int | None:
    if gateway_idle_timeout_secs is not None:
        return max(int(gateway_idle_timeout_secs), 0)
    raw = (os.environ.get(ENV_GATEWAY_IDLE_TIMEOUT_SECS) or "").strip()
    if not raw:
        return _AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_DEFAULT
    try:
        return max(int(raw), 0)
    except ValueError:
        return _AUTO_ENSURE_GATEWAY_IDLE_TIMEOUT_DEFAULT


def build_gateway_daemon_command(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str | None,
    dcc_type: str,
    gateway_persist: bool | None = None,
    gateway_idle_timeout_secs: int | None = None,
    server_bin: str | None = None,
) -> tuple[list[str], dict[str, str]]:
    """Build argv and env for ``dcc-mcp-server gateway``."""
    exe = (server_bin or "").strip() or _resolve_server_bin()
    cmd = [
        exe,
        "gateway",
        "--host",
        gateway_host,
        "--port",
        str(gateway_port),
    ]
    persist = _resolve_gateway_persist(gateway_persist)
    idle_timeout = _resolve_gateway_idle_timeout_secs(gateway_idle_timeout_secs)
    if persist:
        cmd.append("--gateway-persist")
    if idle_timeout is not None:
        cmd.extend(["--gateway-idle-timeout-secs", str(idle_timeout)])

    env = os.environ.copy()
    if not env.get(ENV_GATEWAY_PORT):
        env[ENV_GATEWAY_PORT] = str(gateway_port)
    registry_path = _resolve_registry_dir(registry_dir)
    env[ENV_REGISTRY_DIR] = str(registry_path)
    if dcc_type and not env.get(ENV_DCC_TYPE):
        env[ENV_DCC_TYPE] = dcc_type
    if persist:
        env[ENV_GATEWAY_PERSIST] = "1"
    if idle_timeout is not None:
        env[ENV_GATEWAY_IDLE_TIMEOUT_SECS] = str(idle_timeout)
    return cmd, env


def _try_version_takeover(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str | None,
    dcc_type: str,
    timeout_secs: float,
    gateway_persist: bool | None,
    gateway_idle_timeout_secs: int | None,
    server_bin: str | None,
) -> dict[str, Any] | None:
    """Attempt version-aware gateway takeover when a running gateway is older.

    Returns a result dict if takeover was attempted (success or failure),
    or None if the running gateway is sufficiently new.
    """
    our_version = _get_core_version()
    # Skip takeover when version is a dev placeholder.
    if not our_version or our_version == "0.0.0-dev":
        return None

    gateway_version = _read_gateway_version_from_registry(
        registry_dir,
        gateway_host=gateway_host,
        gateway_port=gateway_port,
    )
    if gateway_version is None:
        gateway_version = _read_gateway_version_from_admin_health(
            gateway_host,
            gateway_port,
        )
    if gateway_version is None or not _is_newer_version(our_version, gateway_version):
        # Running gateway is same or newer — no takeover needed.
        return None

    logger.info(
        "version takeover: our version %s is newer than running gateway %s — triggering takeover",
        our_version,
        gateway_version,
    )

    cooperative_yield_requested = _request_gateway_yield(
        gateway_host,
        gateway_port,
        challenger_version=our_version,
        reason="python_gateway_guardian_version_takeover",
    )

    # Gateways without /gateway/yield still need the registry fallback.
    sentinel_ok = cooperative_yield_requested or _write_sentinel_entry(
        registry_dir,
        gateway_host=gateway_host,
        gateway_port=gateway_port,
        crate_version=our_version,
        adapter_dcc=dcc_type if dcc_type else None,
    )
    if not cooperative_yield_requested and not sentinel_ok:
        logger.warning("version takeover: failed to request yield or write sentinel; skipping takeover")
        return None

    # Wait for the old gateway to yield (up to ~20 s for the 15 s cleanup interval + grace).
    deadline = time.time() + 20.0
    while time.time() < deadline:
        if not _is_application_ready(gateway_host, gateway_port, timeout=0.5):
            logger.info("version takeover: old gateway yielded — spawning new version")
            break
        time.sleep(0.5)
    else:
        logger.warning("version takeover: old gateway did not yield within 20 s; continuing with existing gateway")
        return None

    # Old gateway yielded — spawn new version.
    registry_path = _resolve_registry_dir(registry_dir)
    launch_lock = _LaunchLock(registry_path / _LAUNCH_LOCK)
    try:
        acquired = launch_lock.acquire()
    except OSError as exc:
        return {"ok": False, "reason": "takeover_launch_lock_failed", "error": str(exc)}

    if not acquired:
        # Another process is spawning — wait for it.
        if _wait_managed_gateway_ready(
            gateway_host,
            gateway_port,
            registry_dir=str(registry_path),
            minimum_version=our_version,
            timeout_secs=timeout_secs,
        ):
            return {"ok": True, "reason": "takeover_spawned_by_peer"}
        return {"ok": False, "reason": "takeover_lock_in_progress_timeout"}

    try:
        cmd, env = build_gateway_daemon_command(
            gateway_host=gateway_host,
            gateway_port=gateway_port,
            registry_dir=str(registry_path),
            dcc_type=dcc_type,
            gateway_persist=gateway_persist,
            gateway_idle_timeout_secs=gateway_idle_timeout_secs,
            server_bin=server_bin,
        )
        try:
            spawn = launch_detached(cmd, env=env, cwd=Path.cwd())
            if not spawn.get("ok"):
                return {
                    "ok": False,
                    "reason": "takeover_spawn_failed",
                    "error": spawn.get("error"),
                    "command": cmd,
                }
        except Exception as exc:
            return {"ok": False, "reason": "takeover_spawn_failed", "error": str(exc), "command": cmd}

        if _wait_managed_gateway_ready(
            gateway_host,
            gateway_port,
            registry_dir=str(registry_path),
            minimum_version=our_version,
            timeout_secs=timeout_secs,
        ):
            return {
                "ok": True,
                "reason": "version_takeover_spawned",
                "command": cmd,
                "registry_dir": str(registry_path),
                "pid": spawn.get("pid"),
                "old_version": gateway_version,
                "new_version": our_version,
            }

        return {"ok": False, "reason": "takeover_managed_ready_timeout", "command": cmd}
    finally:
        launch_lock.release()


def _resolve_ensure_timeout(timeout_secs: float | None) -> float:
    """Resolve the ensure timeout: explicit arg > env var > default (15s)."""
    if timeout_secs is not None:
        return max(float(timeout_secs), 0.1)
    return env_float(ENV_GATEWAY_ENSURE_TIMEOUT_SECS, _ENSURE_TIMEOUT_DEFAULT, minimum=0.1)


def ensure_gateway_daemon(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str | None,
    dcc_type: str,
    timeout_secs: float | None = None,
    gateway_persist: bool | None = None,
    gateway_idle_timeout_secs: int | None = None,
    server_bin: str | None = None,
) -> dict[str, Any]:
    """Ensure a machine-wide gateway daemon is healthy on ``gateway_port``.

    When spawning a new daemon, lifecycle options are forwarded to
    ``dcc-mcp-server gateway``. Unset values fall back to
    ``DCC_MCP_GATEWAY_PERSIST`` / ``DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS``.
    """
    timeout_secs = _resolve_ensure_timeout(timeout_secs)
    if gateway_port <= 0:
        return {"ok": False, "reason": "gateway_port_not_configured"}
    if _is_application_ready(gateway_host, gateway_port, timeout=0.5):
        takeover_result = _try_version_takeover(
            gateway_host=gateway_host,
            gateway_port=gateway_port,
            registry_dir=registry_dir,
            dcc_type=dcc_type,
            timeout_secs=timeout_secs,
            gateway_persist=gateway_persist,
            gateway_idle_timeout_secs=gateway_idle_timeout_secs,
            server_bin=server_bin,
        )
        if takeover_result is not None:
            return takeover_result
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

    cmd, env = build_gateway_daemon_command(
        gateway_host=gateway_host,
        gateway_port=gateway_port,
        registry_dir=str(registry_path),
        dcc_type=dcc_type,
        gateway_persist=gateway_persist,
        gateway_idle_timeout_secs=gateway_idle_timeout_secs,
        server_bin=server_bin,
    )

    try:
        try:
            if _is_application_ready(gateway_host, gateway_port, timeout=0.5):
                return {"ok": True, "reason": "already_healthy", "registry_dir": str(registry_path)}
            spawn = launch_detached(cmd, env=env, cwd=Path.cwd())
            if not spawn.get("ok"):
                return {
                    "ok": False,
                    "reason": spawn.get("reason", "spawn_failed"),
                    "error": spawn.get("error"),
                    "command": cmd,
                    "registry_dir": str(registry_path),
                }
        except Exception as exc:
            return {"ok": False, "reason": "spawn_failed", "error": str(exc), "command": cmd}

        if _wait_gateway_ready(gateway_host, gateway_port, timeout_secs=timeout_secs):
            return {
                "ok": True,
                "reason": "spawned",
                "command": cmd,
                "registry_dir": str(registry_path),
                "pid": spawn.get("pid"),
            }

        return {"ok": False, "reason": "spawn_timeout", "command": cmd, "registry_dir": str(registry_path)}
    finally:
        launch_lock.release()


def launch_gateway_daemon(**kwargs: Any) -> dict[str, Any]:
    """Alias for :func:`ensure_gateway_daemon` with explicit daemon naming."""
    return ensure_gateway_daemon(**kwargs)


def _is_newer_version(candidate: str, current: str) -> bool:
    """Return True when *candidate* is strictly newer than *current*."""
    candidate_semver = _parse_semver(candidate)
    current_semver = _parse_semver(current)
    return candidate_semver is not None and current_semver is not None and candidate_semver > current_semver
