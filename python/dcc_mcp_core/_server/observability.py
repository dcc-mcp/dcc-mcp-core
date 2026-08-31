"""Observability collaborators for :class:`DccServerBase` (#486).

Three small classes that own one observability responsibility each:

- :class:`FileLoggingManager` — rolling file logging via
  ``init_file_logging`` (``DCC_MCP_DISABLE_FILE_LOGGING`` env override).
- :class:`JobPersistenceManager` — SQLite job-history database wired into
  ``McpHttpConfig.job_storage_path``; probes for the ``job-persist-sqlite``
  feature so the server can fall back gracefully when the wheel was built
  without it (``DCC_MCP_DISABLE_JOB_PERSISTENCE`` env override).
- :class:`TelemetryManager` — in-process metrics initialisation via
  ``TelemetryConfig`` (``DCC_MCP_DISABLE_TELEMETRY`` env override).

File logging and telemetry remain best-effort. Job persistence records probe
failures explicitly and leaves the requested database path configured so a
subsequent server start fails closed instead of silently falling back to
in-memory history.
"""

from __future__ import annotations

from contextlib import contextmanager
import importlib
import logging
import os
from pathlib import Path
import re
import sqlite3
import stat
import tempfile
from typing import Any

from dcc_mcp_core.constants import ENV_DISABLE_FILE_LOGGING
from dcc_mcp_core.constants import ENV_DISABLE_JOB_PERSISTENCE
from dcc_mcp_core.constants import ENV_DISABLE_TELEMETRY
from dcc_mcp_core.constants import ENV_JOB_INSTANCE_KEY
from dcc_mcp_core.constants import ENV_JOB_STORAGE_PATH
from dcc_mcp_core.constants import ENV_LOG_DIR

logger = logging.getLogger(__name__)


class FileLoggingManager:
    """Owns rolling-file logging initialisation for one DCC server."""

    def __init__(self, dcc_name: str, *, enabled: bool = True) -> None:
        self._dcc_name = dcc_name
        self._enabled = enabled and os.environ.get(ENV_DISABLE_FILE_LOGGING, "0") != "1"
        self._log_dir: str = ""

    @property
    def enabled(self) -> bool:
        return self._enabled

    @property
    def log_dir(self) -> str:
        return self._log_dir

    def init(self) -> str:
        """Initialise rolling file logging; return the resolved log directory."""
        if not self._enabled:
            return ""
        try:
            from dcc_mcp_core import FileLoggingConfig
            from dcc_mcp_core import get_log_dir
            from dcc_mcp_core import init_file_logging

            log_dir = os.environ.get(ENV_LOG_DIR) or get_log_dir()
            pid = os.getpid()
            cfg = FileLoggingConfig(
                directory=log_dir,
                file_name_prefix=f"dcc-mcp-{self._dcc_name}.{pid}",
                max_files=14,
                max_size_bytes=20 * 1024 * 1024,
                rotation="both",
            )
            self._log_dir = init_file_logging(cfg)
            logger.info(
                "[%s] File logging enabled → %s/dcc-mcp-%s.%s.*.log",
                self._dcc_name,
                self._log_dir,
                self._dcc_name,
                pid,
            )
            return self._log_dir
        except Exception as exc:
            logger.warning("[%s] Could not enable file logging: %s", self._dcc_name, exc)
            return ""


class JobPersistenceManager:
    """Wires a per-DCC SQLite job-history database into ``McpHttpConfig``."""

    def __init__(
        self,
        dcc_name: str,
        *,
        enabled: bool = True,
        log_dir: str = "",
        instance_key: str | None = None,
    ) -> None:
        self._dcc_name = dcc_name
        self._enabled = enabled and os.environ.get(ENV_DISABLE_JOB_PERSISTENCE, "0") != "1"
        self._log_dir = log_dir
        self._instance_key = str(instance_key or os.environ.get(ENV_JOB_INSTANCE_KEY, "")).strip() or None
        self._state = "not_configured"
        self._last_error_kind: str | None = None

    @property
    def enabled(self) -> bool:
        return self._enabled

    @property
    def state(self) -> str:
        """Payload-safe persistence state for diagnostics/admin summaries."""
        return self._state

    @property
    def last_error_kind(self) -> str | None:
        return self._last_error_kind

    def init(self, config: Any) -> None:
        """Probe storage writability without starting a server or recovering rows."""
        if not self._enabled:
            self._state = "disabled"
            return
        configured_path = getattr(config, "job_storage_path", None)
        # Some source-only tests use permissive mock configs whose arbitrary
        # attributes are ``MagicMock`` instances. Treat those as unset rather
        # than passing them to ``Path``; real configured values remain strict
        # strings/path-like objects and are never rewritten.
        explicit_path = self._coerce_configured_path(configured_path)
        if explicit_path is None:
            explicit_path = os.environ.get(ENV_JOB_STORAGE_PATH)
        has_explicit_path = explicit_path is not None
        configured_db_path = str(Path(explicit_path).expanduser()) if explicit_path else None
        try:
            core = importlib.import_module("dcc_mcp_core._core")
            feature_enabled = getattr(core, "job_persistence_feature_enabled", lambda: False)()
        except ImportError:
            # A missing or unloadable native extension is the lite-wheel
            # boundary. Do not let a pure-Python SQLite probe claim that the
            # Rust persistence backend is available.
            feature_enabled = False
        except Exception:
            feature_enabled = False
        if not feature_enabled:
            self._state = "unavailable" if has_explicit_path else "disabled"
            self._last_error_kind = "feature_disabled"
            if has_explicit_path:
                config.job_storage_path = configured_db_path
            logger.warning(
                "[%s] job-persist-sqlite feature is unavailable; persistence remains %s",
                self._dcc_name,
                "unavailable" if has_explicit_path else "disabled",
            )
            return
        db_path = configured_db_path or self._resolve_db_path()
        if db_path is None:
            self._state = "unavailable"
            self._last_error_kind = "path"
            return
        try:
            self._probe_storage(db_path)
            config.job_storage_path = db_path
            self._state = "healthy"
            self._last_error_kind = None
            logger.info("[%s] Job persistence enabled → %s", self._dcc_name, db_path)
        except RuntimeError as exc:
            err_msg = str(exc)
            if "job-persist-sqlite" in err_msg and "job_storage_path" in err_msg:
                self._state = "disabled"
                self._last_error_kind = "feature_disabled"
                logger.warning(
                    "[%s] job-persist-sqlite feature is unavailable; persistence remains disabled",
                    self._dcc_name,
                )
            else:
                config.job_storage_path = db_path
                self._state = "unavailable"
                self._last_error_kind = self._classify_probe_error(exc)
                logger.warning("[%s] Job persistence unavailable; startup will fail closed: %s", self._dcc_name, exc)
        except Exception as exc:
            config.job_storage_path = db_path
            self._state = "unavailable"
            self._last_error_kind = self._classify_probe_error(exc)
            logger.warning("[%s] Job persistence unavailable; startup will fail closed: %s", self._dcc_name, exc)

    @staticmethod
    def _coerce_configured_path(value: Any) -> str | None:
        """Return a real configured path, ignoring permissive test doubles."""
        if isinstance(value, str):
            return value.strip() or None
        if not isinstance(value, os.PathLike):
            return None
        value_type = type(value)
        if value_type.__module__.startswith("unittest.mock") or "Mock" in value_type.__name__:
            return None
        try:
            path = os.fspath(value)
        except TypeError:
            return None
        return path.strip() or None if isinstance(path, str) else None

    @staticmethod
    def _probe_storage(db_path: str) -> None:
        """Exercise a transactional write using only a temporary table.

        ``BEGIN IMMEDIATE`` verifies that the database is writable and that
        SQLite can acquire its WAL/SHM locks. The temporary table and row are
        rolled back, so existing job rows (including Pending/Running rows) are
        never read, recovered, or modified by the startup probe.
        """
        with _storage_ownership_lock(db_path):
            connection = sqlite3.connect(db_path, timeout=1.0)
            try:
                connection.execute("BEGIN IMMEDIATE")
                connection.execute("CREATE TEMP TABLE dcc_mcp_job_storage_probe (value INTEGER)")
                connection.execute("INSERT INTO dcc_mcp_job_storage_probe(value) VALUES (1)")
                connection.rollback()
            finally:
                connection.close()

    @staticmethod
    def _classify_probe_error(error: Exception) -> str:
        message = str(error).lower()
        if "readonly" in message or "read-only" in message or "permission denied" in message:
            return "readonly"
        if "wal" in message or "shm" in message:
            return "wal"
        if "locked" in message or "busy" in message or "ownership" in message:
            return "ownership"
        return "backend"

    def _resolve_db_path(self, *, explicit_path: str | None = None) -> str | None:
        try:
            if explicit_path:
                return str(Path(explicit_path).expanduser())
            db_dir = self._log_dir or os.environ.get(ENV_LOG_DIR)
            if not db_dir:
                from dcc_mcp_core import get_log_dir

                db_dir = get_log_dir()
            # The native backend takes an exclusive physical-file lease. Keep
            # the default database per DCC process so two GUI instances can run
            # concurrently while a stop/start in the same process reuses its
            # history. Operators needing deliberate cross-process sharing must
            # provide an explicit ``job_storage_path`` and accept ownership
            # arbitration.
            key = self._instance_key or str(os.getpid())
            safe_key = re.sub(r"[^A-Za-z0-9_.-]+", "_", key)[:64] or "default"
            return str(Path(db_dir) / f"dcc-mcp-{self._dcc_name}-{safe_key}-jobs.db")
        except Exception as exc:
            logger.debug("[%s] Could not resolve job persistence path: %s", self._dcc_name, exc)
            return None


@contextmanager
def _storage_ownership_lock(db_path: str):
    """Acquire the native Rust SQLite ownership sidecar used by ``fs4``."""
    db_file = Path(db_path)
    try:
        file_stat = os.lstat(db_file)
    except FileNotFoundError:
        file_stat = None
    if file_stat is not None and (
        stat.S_ISLNK(file_stat.st_mode) or (os.name == "nt" and getattr(file_stat, "st_file_attributes", 0) & 0x400)
    ):
        raise OSError("job storage path must not be a symlink or reparse point")
    try:
        db_file.parent.mkdir(parents=True, exist_ok=True)
        # SqliteStorage creates the database before deriving its physical
        # identity. Do the same before asking the native helper for a lease;
        # otherwise first-use probes could briefly select a pathname lock.
        db_file.touch(exist_ok=True)
    except OSError:
        # Let SQLite report the definitive read-only/permission error below.
        pass
    lock_path = None
    try:
        core = importlib.import_module("dcc_mcp_core._core")
        native_path = getattr(core, "job_persistence_ownership_lock_path", None)
        if callable(native_path):
            lock_path = native_path(db_path)
    except Exception:
        lock_path = None
    if not lock_path:
        # Compatibility fallback for source-only environments. Native wheels
        # always expose the helper above, keeping Python and Rust leases in
        # the same physical-file namespace.
        try:
            db_stat = db_file.stat()
            lock_path = str(Path(tempfile.gettempdir()) / f"dcc-mcp-job-u{db_stat.st_dev:x}-{db_stat.st_ino:x}.lock")
        except OSError:
            lock_path = f"{db_path}.lock"
    lock_file = Path(lock_path)
    handle = lock_file.open("a+b")
    acquired = False
    windows_overlapped = None
    try:
        if os.name == "nt":
            import ctypes
            from ctypes import wintypes
            import msvcrt

            class _Overlapped(ctypes.Structure):
                _fields_ = [
                    ("Internal", wintypes.LPVOID),
                    ("InternalHigh", wintypes.LPVOID),
                    ("Offset", wintypes.DWORD),
                    ("OffsetHigh", wintypes.DWORD),
                    ("hEvent", wintypes.HANDLE),
                ]

            windows_overlapped = _Overlapped()
            kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
            lock_file_ex = kernel32.LockFileEx
            lock_file_ex.argtypes = [
                wintypes.HANDLE,
                wintypes.DWORD,
                wintypes.DWORD,
                wintypes.DWORD,
                wintypes.DWORD,
                ctypes.POINTER(_Overlapped),
            ]
            lock_file_ex.restype = wintypes.BOOL
            flags = 0x00000002 | 0x00000001  # LOCKFILE_EXCLUSIVE_LOCK | FAIL_IMMEDIATELY
            if not lock_file_ex(
                wintypes.HANDLE(msvcrt.get_osfhandle(handle.fileno())),
                flags,
                0,
                1,
                0,
                ctypes.byref(windows_overlapped),
            ):
                raise OSError("job storage ownership unavailable: database lock is held")
        else:
            import fcntl

            try:
                fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError as exc:
                raise OSError("job storage ownership unavailable: database lock is held") from exc
        acquired = True
        yield handle
    finally:
        try:
            if acquired and os.name == "nt":
                import ctypes
                from ctypes import wintypes
                import msvcrt

                kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
                unlock_file_ex = kernel32.UnlockFileEx
                unlock_file_ex.argtypes = [
                    wintypes.HANDLE,
                    wintypes.DWORD,
                    wintypes.DWORD,
                    wintypes.DWORD,
                    ctypes.c_void_p,
                ]
                unlock_file_ex.restype = wintypes.BOOL
                unlock_file_ex(
                    wintypes.HANDLE(msvcrt.get_osfhandle(handle.fileno())),
                    0,
                    1,
                    0,
                    ctypes.byref(windows_overlapped),
                )
            elif acquired:
                import fcntl

                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        finally:
            handle.close()


class TelemetryManager:
    """Owns in-process metrics initialisation via ``TelemetryConfig``."""

    def __init__(self, dcc_name: str, dcc_pid: int, *, enabled: bool = True) -> None:
        self._dcc_name = dcc_name
        self._dcc_pid = dcc_pid
        self._enabled = enabled and os.environ.get(ENV_DISABLE_TELEMETRY, "0") != "1"

    @property
    def enabled(self) -> bool:
        return self._enabled

    def init(self) -> None:
        """Initialise telemetry once; safe to call multiple times."""
        if not self._enabled:
            return
        try:
            from dcc_mcp_core import TelemetryConfig
            from dcc_mcp_core import is_telemetry_initialized

            if is_telemetry_initialized():
                return
            (
                TelemetryConfig(f"dcc-mcp-{self._dcc_name}")
                .with_noop_exporter()
                .set_enable_metrics(True)
                .set_enable_tracing(False)
                .with_attribute("dcc.name", self._dcc_name)
                .with_attribute("dcc.pid", str(self._dcc_pid))
                .init()
            )
            logger.info("[%s] In-process telemetry (metrics) enabled", self._dcc_name)
        except Exception as exc:
            logger.debug("[%s] Could not enable telemetry: %s", self._dcc_name, exc)
