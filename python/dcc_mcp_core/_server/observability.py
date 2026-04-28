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

All three are intentionally non-fatal: any internal failure is logged and
swallowed so the server can still come up without observability.
"""

from __future__ import annotations

import logging
import os
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class FileLoggingManager:
    """Owns rolling-file logging initialisation for one DCC server."""

    def __init__(self, dcc_name: str, *, enabled: bool = True) -> None:
        self._dcc_name = dcc_name
        self._enabled = enabled and os.environ.get("DCC_MCP_DISABLE_FILE_LOGGING", "0") != "1"
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

            log_dir = os.environ.get("DCC_MCP_LOG_DIR") or get_log_dir()
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

    def __init__(self, dcc_name: str, *, enabled: bool = True, log_dir: str = "") -> None:
        self._dcc_name = dcc_name
        self._enabled = enabled and os.environ.get("DCC_MCP_DISABLE_JOB_PERSISTENCE", "0") != "1"
        self._log_dir = log_dir

    @property
    def enabled(self) -> bool:
        return self._enabled

    def init(self, config: Any) -> None:
        """Probe for the ``job-persist-sqlite`` feature and wire ``config``."""
        if not self._enabled:
            return
        db_path = self._resolve_db_path()
        if db_path is None:
            return
        try:
            from dcc_mcp_core import McpHttpConfig
            from dcc_mcp_core import create_skill_server

            probe_cfg = McpHttpConfig(port=0, server_name="probe")
            probe_cfg.job_storage_path = db_path
            probe_srv = create_skill_server(self._dcc_name, probe_cfg)
            probe_handle = probe_srv.start()
            probe_handle.shutdown()
            config.job_storage_path = db_path
            logger.info("[%s] Job persistence enabled → %s", self._dcc_name, db_path)
        except RuntimeError as exc:
            err_msg = str(exc)
            if "job-persist-sqlite" in err_msg and "job_storage_path" in err_msg:
                logger.warning(
                    "[%s] job-persist-sqlite feature not compiled in; job persistence disabled (in-memory fallback)",
                    self._dcc_name,
                )
            else:
                logger.debug("[%s] Job persistence probe failed: %s", self._dcc_name, exc)
        except Exception as exc:
            logger.debug("[%s] Job persistence probe failed: %s", self._dcc_name, exc)

    def _resolve_db_path(self) -> str | None:
        try:
            from dcc_mcp_core import get_log_dir

            db_dir = self._log_dir or os.environ.get("DCC_MCP_LOG_DIR") or get_log_dir()
            return str(Path(db_dir) / f"dcc-mcp-{self._dcc_name}-jobs.db")
        except Exception as exc:
            logger.debug("[%s] Could not resolve job persistence path: %s", self._dcc_name, exc)
            return None


class TelemetryManager:
    """Owns in-process metrics initialisation via ``TelemetryConfig``."""

    def __init__(self, dcc_name: str, dcc_pid: int, *, enabled: bool = True) -> None:
        self._dcc_name = dcc_name
        self._dcc_pid = dcc_pid
        self._enabled = enabled and os.environ.get("DCC_MCP_DISABLE_TELEMETRY", "0") != "1"

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
