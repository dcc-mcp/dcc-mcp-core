"""Best-effort standalone gateway bootstrap for embedded Python adapters."""

from __future__ import annotations

import contextlib
import json
import logging
import os
from pathlib import Path
import shutil
import threading
import time
from typing import Any
from typing import Callable
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import urlopen
import uuid

from dcc_mcp_core.daemon_launch import launch_detached
from dcc_mcp_core.install_lifecycle import default_registry_dir

logger = logging.getLogger(__name__)

_LAUNCH_LOCK = "gateway-launch.lock"
_LAUNCH_LOCK_STALE_SECS_ENV = "DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS"
_LAUNCH_LOCK_STALE_SECS_DEFAULT = 30.0

# ── Version-aware takeover constants ────────────────────────────────────────
# Matches Rust dcc_mcp_transport::discovery::types::GATEWAY_SENTINEL_DCC_TYPE
_GATEWAY_SENTINEL_DCC_TYPE = "__gateway__"
_GATEWAY_SENTINEL_STALE_SECS = 30.0
_VERSION_TAKEOVER_WAIT_SECS = 20.0
_VERSION_TAKEOVER_POLL_INTERVAL = 0.5
_SERVICES_JSON = "services.json"


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
        stale_after = _launch_lock_stale_secs()
        for attempt in range(2):
            try:
                self._fd = os.open(str(self.path), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            except FileExistsError:
                if attempt == 0 and _remove_stale_launch_lock(self.path, stale_after):
                    continue
                return False
            else:
                with contextlib.suppress(OSError):
                    os.write(self._fd, f"pid={os.getpid()} ts={int(time.time())}\n".encode("ascii"))
                return True
        return False

    def release(self) -> None:
        if self._fd is not None:
            os.close(self._fd)
            self._fd = None
        with contextlib.suppress(FileNotFoundError):
            self.path.unlink()


def _launch_lock_stale_secs() -> float:
    return _float_env(_LAUNCH_LOCK_STALE_SECS_ENV, _LAUNCH_LOCK_STALE_SECS_DEFAULT)


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
        if _is_healthy(host, port, timeout=probe_timeout):
            return True
        time.sleep(0.1)
    return False


def _resolve_gateway_persist(gateway_persist: bool | None) -> bool:
    if gateway_persist is not None:
        return bool(gateway_persist)
    return (os.environ.get("DCC_MCP_GATEWAY_PERSIST") or "").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }


def _resolve_gateway_idle_timeout_secs(gateway_idle_timeout_secs: int | None) -> int | None:
    if gateway_idle_timeout_secs is not None:
        return max(int(gateway_idle_timeout_secs), 0)
    raw = (os.environ.get("DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS") or "").strip()
    if not raw:
        return None
    try:
        return max(int(raw), 0)
    except ValueError:
        return None


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
    if not env.get("DCC_MCP_GATEWAY_PORT"):
        env["DCC_MCP_GATEWAY_PORT"] = str(gateway_port)
    registry_path = _resolve_registry_dir(registry_dir)
    env["DCC_MCP_REGISTRY_DIR"] = str(registry_path)
    if dcc_type and not env.get("DCC_MCP_DCC_TYPE"):
        env["DCC_MCP_DCC_TYPE"] = dcc_type
    if persist:
        env["DCC_MCP_GATEWAY_PERSIST"] = "1"
    if idle_timeout is not None:
        env["DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS"] = str(idle_timeout)
    return cmd, env


def ensure_gateway_daemon(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str | None,
    dcc_type: str,
    timeout_secs: float = 5.0,
    gateway_persist: bool | None = None,
    gateway_idle_timeout_secs: int | None = None,
    server_bin: str | None = None,
    adapter_version: str | None = None,
    enable_version_takeover: bool = True,
) -> dict[str, Any]:
    """Ensure a machine-wide gateway daemon is healthy on ``gateway_port``.

    When spawning a new daemon, lifecycle options are forwarded to
    ``dcc-mcp-server gateway``. Unset values fall back to
    ``DCC_MCP_GATEWAY_PERSIST`` / ``DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS``.

    When *enable_version_takeover* is ``True`` (the default) and the gateway
    port is already occupied by a healthy gateway, this function checks whether
    the current ``dcc_mcp_core`` version is newer than the running gateway.  If
    it is, a ``__gateway__`` sentinel entry is written to ``services.json`` so
    the running gateway's 15-second cleanup loop detects a newer challenger and
    voluntarily yields.  This function then waits for the old gateway to exit
    and spawns the replacement.
    """
    if gateway_port <= 0:
        return {"ok": False, "reason": "gateway_port_not_configured"}

    registry_path = _resolve_registry_dir(registry_dir)

    if _is_healthy(gateway_host, gateway_port, timeout=0.5):
        if enable_version_takeover:
            takeover = _try_version_takeover(
                gateway_host=gateway_host,
                gateway_port=gateway_port,
                registry_dir=str(registry_path),
                dcc_type=dcc_type,
                timeout_secs=timeout_secs,
                server_bin=server_bin,
                adapter_version=adapter_version,
            )
            if takeover.get("reason") != "already_healthy":
                return takeover
        return {"ok": True, "reason": "already_healthy"}

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
            if _is_healthy(gateway_host, gateway_port, timeout=0.5):
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
        self._crash_count = 0
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
            try:
                self.probe_once()
            except Exception:
                self._crash_count += 1
                logger.exception(
                    "[gateway_guardian:%s] probe_once crashed (crash #%d)",
                    self.dcc_type,
                    self._crash_count,
                )
                self._publish(
                    {
                        "ok": False,
                        "reason": "guardian_crash",
                        "crash_count": self._crash_count,
                    }
                )

    def _publish(self, update: dict[str, Any]) -> dict[str, Any]:
        payload = {
            "gateway_host": self.gateway_host,
            "gateway_port": self.gateway_port,
            "guardian_running": bool(self._thread is not None and self._thread.is_alive()),
            "consecutive_failures": self._consecutive_failures,
            "restart_attempts": self._restart_attempts,
            "crash_count": self._crash_count,
            "timestamp_ms": int(time.time() * 1000),
            **update,
        }
        with self._lock:
            self._last_status = payload
        if self.status_callback is not None:
            with contextlib.suppress(Exception):
                self.status_callback(dict(payload))
        return payload


# ── Version-aware takeover helpers ──────────────────────────────────────────


def _get_core_version() -> str | None:
    """Resolve ``dcc_mcp_core`` version without importing the native extension.

    Resolution order:
    1. ``DCC_MCP_CORE_VERSION`` environment variable
    2. ``importlib.metadata.version("dcc-mcp-core")``
    3. ``dcc_mcp_core.__version__`` attribute
    """
    env_version = (os.environ.get("DCC_MCP_CORE_VERSION") or "").strip()
    if env_version:
        return env_version

    try:
        from importlib.metadata import version as pkg_version

        return pkg_version("dcc-mcp-core")
    except Exception:
        pass

    try:
        import dcc_mcp_core

        return getattr(dcc_mcp_core, "__version__", None) or None
    except Exception:
        pass

    return None


def _parse_semver(v: str) -> tuple[int, int, int]:
    """Parse a semver string to a numeric triple, matching Rust :func:`parse_semver`.

    Leading ``v`` / ``V`` stripped; pre-release suffixes (``-rc1``) ignored;
    missing components default to ``0``.
    """
    text = str(v).strip().lstrip("vV")
    text = text.split("-", 1)[0]
    parts = text.split(".")
    nums: list[int] = []
    for part in parts[:3]:
        try:
            nums.append(int(part))
        except ValueError:
            nums.append(0)
    while len(nums) < 3:
        nums.append(0)
    return (nums[0], nums[1], nums[2])


def _is_newer_version(candidate: str, current: str) -> bool:
    """Return ``True`` when *candidate* is strictly newer than *current*."""
    return _parse_semver(candidate) > _parse_semver(current)


def _system_time_now_json() -> dict[str, int]:
    """Return the current time in serde ``SystemTime`` JSON format.

    serde serializes ``std::time::SystemTime`` as a struct with two fields:
    ``secs_since_epoch`` (u64) and ``nanos_since_epoch`` (u32).
    """
    now = time.time()
    secs = int(now)
    nanos = int((now - secs) * 1_000_000_000)
    return {"secs_since_epoch": secs, "nanos_since_epoch": nanos}


def _read_services_json(registry_dir: Path) -> list[dict[str, Any]]:
    """Read ``services.json`` entries from *registry_dir*."""
    path = registry_dir / _SERVICES_JSON
    if not path.exists():
        return []
    try:
        with path.open("r", encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        return []
    if isinstance(data, list):
        return [item for item in data if isinstance(item, dict)]
    return []


def _write_services_json(registry_dir: Path, entries: list[dict[str, Any]]) -> bool:
    """Atomically write *entries* to ``services.json``."""
    path = registry_dir / _SERVICES_JSON
    tmp = path.with_suffix(".tmp")
    try:
        registry_dir.mkdir(parents=True, exist_ok=True)
        with tmp.open("w", encoding="utf-8") as fh:
            json.dump(entries, fh, indent=2, ensure_ascii=False)
        tmp.replace(path)
        return True
    except OSError:
        return False


def _write_takeover_sentinel(
    registry_dir: Path,
    host: str,
    port: int,
    version: str,
    adapter_version: str | None = None,
    adapter_dcc: str | None = None,
) -> bool:
    """Write a ``__gateway__`` sentinel entry to trigger the running gateway to yield.

    Removes any existing sentinel for the same (host, port) first, then appends
    the new entry.  Uses the serde ``SystemTime`` JSON format for
    ``last_heartbeat`` and ``registered_at`` so the Rust ``FileRegistry`` can
    deserialise the entry correctly.
    """
    entries = _read_services_json(registry_dir)

    # Remove existing sentinel entries for this (host, port)
    filtered: list[dict[str, Any]] = []
    for entry in entries:
        if (
            entry.get("dcc_type") == _GATEWAY_SENTINEL_DCC_TYPE
            and entry.get("host") == host
            and entry.get("port") == port
        ):
            continue
        filtered.append(entry)

    now_ts = _system_time_now_json()
    sentinel: dict[str, Any] = {
        "dcc_type": _GATEWAY_SENTINEL_DCC_TYPE,
        "instance_id": str(uuid.uuid4()),
        "host": host,
        "port": port,
        "version": version,
        "status": "available",
        "last_heartbeat": dict(now_ts),
        "registered_at": dict(now_ts),
    }
    if adapter_version:
        sentinel["adapter_version"] = adapter_version
    if adapter_dcc:
        sentinel["adapter_dcc"] = adapter_dcc

    filtered.append(sentinel)
    return _write_services_json(registry_dir, filtered)


def _cleanup_takeover_sentinel(
    registry_dir: Path,
    host: str,
    port: int,
) -> bool:
    """Remove our ``__gateway__`` sentinel from ``services.json``."""
    entries = _read_services_json(registry_dir)
    filtered: list[dict[str, Any]] = []
    removed = False
    for entry in entries:
        if (
            entry.get("dcc_type") == _GATEWAY_SENTINEL_DCC_TYPE
            and entry.get("host") == host
            and entry.get("port") == port
        ):
            removed = True
            continue
        filtered.append(entry)
    if not removed:
        return True
    return _write_services_json(registry_dir, filtered)


def _sentry_stale(entry: dict[str, Any], stale_timeout: float) -> bool:
    """Check if a services.json entry is stale based on ``last_heartbeat``.

    The Rust ``ServiceEntry::is_stale`` compares ``SystemTime::elapsed()``
    against a timeout.  We match this by reading the serde-serialised
    ``last_heartbeat`` field.
    """
    hb = entry.get("last_heartbeat")
    if not isinstance(hb, dict):
        return True
    secs = hb.get("secs_since_epoch")
    if not isinstance(secs, (int, float)):
        return True
    nanos = hb.get("nanos_since_epoch", 0)
    if not isinstance(nanos, (int, float)):
        nanos = 0
    ts = float(secs) + float(nanos) / 1_000_000_000
    return (time.time() - ts) > stale_timeout


def _try_version_takeover(
    *,
    gateway_host: str,
    gateway_port: int,
    registry_dir: str,
    dcc_type: str,
    timeout_secs: float,
    server_bin: str | None = None,
    adapter_version: str | None = None,
) -> dict[str, Any]:
    """Check if we carry a newer version and trigger a version-aware gateway takeover.

    Returns a result dict; ``{"ok": True, "reason": "already_healthy"}`` when
    no takeover was needed, ``{"ok": True, "reason": "version_takeover_spawned"}``
    on successful takeover, or ``{"ok": False, ...}`` on failure.
    """
    our_version = _get_core_version()
    if not our_version:
        logger.debug("[gateway_guardian] no core version resolved; skipping version takeover")
        return {"ok": True, "reason": "already_healthy"}

    registry_path = Path(registry_dir).expanduser()
    entries = _read_services_json(registry_path)

    # Check if any sentinel entry is already newer than us (another sidecar
    # already requested a newer takeover).
    for entry in entries:
        if entry.get("dcc_type") != _GATEWAY_SENTINEL_DCC_TYPE:
            continue
        if _sentry_stale(entry, _GATEWAY_SENTINEL_STALE_SECS):
            continue
        their_version = entry.get("version")
        if not their_version:
            continue
        if _is_newer_version(their_version, our_version):
            logger.debug(
                "[gateway_guardian] running gateway sentinel (%s) is newer than us (%s); no takeover",
                their_version,
                our_version,
            )
            return {"ok": True, "reason": "already_healthy"}

    # Determine if we are newer than any existing sentinel.
    should_takeover = False
    for entry in entries:
        if entry.get("dcc_type") != _GATEWAY_SENTINEL_DCC_TYPE:
            continue
        if _sentry_stale(entry, _GATEWAY_SENTINEL_STALE_SECS):
            continue
        their_version = entry.get("version")
        if not their_version:
            continue
        if _is_newer_version(our_version, their_version):
            should_takeover = True
            break

    if not should_takeover:
        # No sentinel exists or we're not newer — no action.
        return {"ok": True, "reason": "already_healthy"}

    logger.info(
        "[gateway_guardian] version %s > running gateway; triggering takeover for %s:%d",
        our_version,
        gateway_host,
        gateway_port,
    )

    # Write our sentinel so the running gateway detects a newer challenger
    # in its 15 s cleanup loop and voluntarily yields.
    if not _write_takeover_sentinel(
        registry_path,
        gateway_host,
        gateway_port,
        our_version,
        adapter_version=adapter_version,
        adapter_dcc=dcc_type,
    ):
        return {
            "ok": False,
            "reason": "version_takeover_sentinel_write_failed",
            "registry_dir": str(registry_path),
        }

    # Wait for old gateway to yield (polling /health).
    deadline = time.time() + _VERSION_TAKEOVER_WAIT_SECS
    old_gateway_yielded = False
    while time.time() < deadline:
        if not _is_healthy(gateway_host, gateway_port, timeout=_VERSION_TAKEOVER_POLL_INTERVAL):
            old_gateway_yielded = True
            break
        time.sleep(_VERSION_TAKEOVER_POLL_INTERVAL)

    if not old_gateway_yielded:
        logger.warning("[gateway_guardian] old gateway did not yield within %s s", _VERSION_TAKEOVER_WAIT_SECS)
        _cleanup_takeover_sentinel(registry_path, gateway_host, gateway_port)
        return {"ok": True, "reason": "already_healthy", "version_takeover_timeout": True}

    # Spawn new gateway (reuse build_gateway_daemon_command + launch_detached).
    cmd, env = build_gateway_daemon_command(
        gateway_host=gateway_host,
        gateway_port=gateway_port,
        registry_dir=registry_dir,
        dcc_type=dcc_type,
        server_bin=server_bin,
    )
    try:
        spawn = launch_detached(cmd, env=env, cwd=Path.cwd())
    except Exception as exc:
        _cleanup_takeover_sentinel(registry_path, gateway_host, gateway_port)
        return {"ok": False, "reason": "spawn_failed", "error": str(exc), "command": cmd}

    if not spawn.get("ok"):
        _cleanup_takeover_sentinel(registry_path, gateway_host, gateway_port)
        return {
            "ok": False,
            "reason": spawn.get("reason", "spawn_failed"),
            "error": spawn.get("error"),
            "command": cmd,
            "registry_dir": str(registry_path),
        }

    if _wait_gateway_ready(gateway_host, gateway_port, timeout_secs=timeout_secs):
        _cleanup_takeover_sentinel(registry_path, gateway_host, gateway_port)
        logger.info("[gateway_guardian] version takeover successful — new gateway %s running", our_version)
        return {
            "ok": True,
            "reason": "version_takeover_spawned",
            "version": our_version,
            "command": cmd,
            "registry_dir": str(registry_path),
            "pid": spawn.get("pid"),
        }

    _cleanup_takeover_sentinel(registry_path, gateway_host, gateway_port)
    return {"ok": False, "reason": "spawn_timeout", "command": cmd, "registry_dir": str(registry_path)}


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
