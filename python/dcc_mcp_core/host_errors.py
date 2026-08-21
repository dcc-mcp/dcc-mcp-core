"""Cross-DCC startup and runtime error capture.

The bootstrap helpers in this module intentionally use only the Python
standard library.  DCC startup scripts can therefore import them before the
native ``dcc_mcp_core._core`` extension or their adapter package.
"""

from __future__ import annotations

import contextlib
from functools import partial
import json
import logging
from logging.handlers import RotatingFileHandler
import os
from pathlib import Path
import re
import sys
import threading
import time
import traceback
from typing import Any
from typing import Dict
from typing import Iterator
from typing import Optional

from ._version_util import package_version
from .constants import ENV_DISABLE_FILE_LOGGING
from .constants import ENV_LOG_DIR

_ERROR_LOG_LOCK = threading.Lock()
_MAX_MESSAGE_CHARS = 8192
_MAX_TRACEBACK_CHARS = 32768


_package_version = partial(package_version, fallback="unknown")


def _default_log_dir() -> Path:
    configured = os.environ.get(ENV_LOG_DIR)
    if configured:
        return Path(configured)
    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
    elif sys.platform == "darwin":
        base = str(Path.home() / "Library" / "Application Support")
    else:
        base = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(base) / "dcc-mcp" / "log"


def _safe_dcc_name(dcc_name: str) -> str:
    normalized = re.sub(r"[^A-Za-z0-9_.-]+", "_", str(dcc_name).strip())
    return normalized.strip("._-") or "dcc"


def _truncate(value: Optional[str], limit: int) -> Optional[str]:
    if value is None or len(value) <= limit:
        return value
    return value[:limit] + "...[truncated]"


def _json_safe(value: Any) -> Any:
    return json.loads(json.dumps(value, ensure_ascii=False, default=str))


def _write_host_error_event(
    event: Dict[str, Any],
    *,
    log_dir: Optional[str] = None,
    enabled: bool = True,
) -> Optional[str]:
    """Append one Admin-compatible structured line without masking the error."""
    if not enabled:
        return None
    try:
        directory = Path(log_dir) if log_dir else _default_log_dir()
        directory.mkdir(parents=True, exist_ok=True)
        dcc_name = _safe_dcc_name(str(event.get("dcc_type") or "dcc"))
        pid = int(event.get("dcc_pid") or os.getpid())
        path = directory / f"dcc-mcp-{dcc_name}.{pid}.host-errors.log"
        payload = json.dumps(_json_safe(event), ensure_ascii=False, separators=(",", ":"))
        record = logging.LogRecord(
            "dcc_mcp_core.host_errors",
            logging.ERROR,
            "",
            0,
            payload,
            (),
            None,
        )
        formatter = logging.Formatter(
            "%(asctime)sZ %(levelname)s dcc_mcp_core.host_errors: %(message)s",
            datefmt="%Y-%m-%dT%H:%M:%S",
        )
        formatter.converter = time.gmtime
        with _ERROR_LOG_LOCK:
            handler = RotatingFileHandler(
                str(path),
                maxBytes=5 * 1024 * 1024,
                backupCount=4,
                encoding="utf-8",
            )
            try:
                handler.setFormatter(formatter)
                handler.emit(record)
            finally:
                handler.close()
        return str(path)
    except Exception:
        return None


def _format_exception(
    exc_type: type,
    exc: BaseException,
    tb: Any,
) -> str:
    return "".join(traceback.format_exception(exc_type, exc, tb))


def record_bootstrap_error(
    dcc_name: str,
    exc: BaseException,
    *,
    adapter_version: Optional[str] = None,
    min_core_version: Optional[str] = None,
    phase: str = "bootstrap",
    log_dir: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
) -> Optional[str]:
    """Persist a startup exception before the MCP server is available.

    The exception is not swallowed.  Use :func:`capture_bootstrap_errors` in a
    DCC startup script when the original host error dialog must remain visible.
    """
    core_module = sys.modules.get("dcc_mcp_core")
    core_path = getattr(core_module, "__file__", None)
    event: Dict[str, Any] = {
        "event": "dcc_host_error",
        "message": _truncate(str(exc) or type(exc).__name__, _MAX_MESSAGE_CHARS),
        "dcc_type": str(dcc_name),
        "dcc_pid": os.getpid(),
        "phase": str(phase),
        "source": "adapter_bootstrap",
        "stream": "stderr",
        "level": "error",
        "exception_type": f"{type(exc).__module__}.{type(exc).__name__}",
        "traceback": _truncate(
            _format_exception(type(exc), exc, exc.__traceback__),
            _MAX_TRACEBACK_CHARS,
        ),
        "adapter_version": adapter_version,
        "min_core_version": min_core_version,
        "core_version": _package_version(),
        "core_path": str(core_path) if core_path else None,
        "python_version": ".".join(str(part) for part in sys.version_info[:3]),
        "python_executable": sys.executable,
        "metadata": metadata or {},
    }
    enabled = os.environ.get(ENV_DISABLE_FILE_LOGGING, "0") != "1"
    return _write_host_error_event(event, log_dir=log_dir, enabled=enabled)


@contextlib.contextmanager
def capture_bootstrap_errors(
    dcc_name: str,
    *,
    adapter_version: Optional[str] = None,
    min_core_version: Optional[str] = None,
    phase: str = "bootstrap",
    log_dir: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
) -> Iterator[None]:
    """Record and re-raise any DCC adapter bootstrap failure."""
    try:
        yield
    except BaseException as exc:
        record_bootstrap_error(
            dcc_name,
            exc,
            adapter_version=adapter_version,
            min_core_version=min_core_version,
            phase=phase,
            log_dir=log_dir,
            metadata=metadata,
        )
        raise


class _ErrorLoggingHandler(logging.Handler):
    def __init__(self, capture: _HostErrorCapture) -> None:
        super().__init__(level=logging.ERROR)
        self._capture = capture

    def emit(self, record: logging.LogRecord) -> None:
        try:
            traceback_text = None
            exception_type = None
            if record.exc_info:
                exc_type, exc, tb = record.exc_info
                if exc_type is not None and exc is not None:
                    traceback_text = _format_exception(exc_type, exc, tb)
                    exception_type = f"{exc_type.__module__}.{exc_type.__name__}"
            self._capture.report(
                record.getMessage(),
                source=record.name,
                stream="logging",
                phase="runtime",
                exception_type=exception_type,
                traceback_text=traceback_text,
                metadata={"logger": record.name},
            )
        except Exception:
            self.handleError(record)


class _HostErrorCapture:
    """Own process hooks and feed existing output/session event buffers."""

    def __init__(
        self,
        dcc_name: str,
        dcc_pid: int,
        *,
        instance_id: str,
        core_version: str,
        adapter_version: Optional[str] = None,
        log_dir: Optional[str] = None,
        persist_to_file: bool = True,
        output_capture: Any = None,
        session_events: Any = None,
        notify_updated: Any = None,
    ) -> None:
        self.dcc_name = str(dcc_name)
        self.dcc_pid = int(dcc_pid)
        self.instance_id = str(instance_id)
        self.core_version = str(core_version)
        self.adapter_version = adapter_version
        self.log_dir = log_dir
        self.persist_to_file = bool(persist_to_file)
        self.output_capture = output_capture
        self.session_events = session_events
        self._notify_updated = notify_updated
        self._installed = False
        self._logging_handler = _ErrorLoggingHandler(self)
        self._previous_sys_hook: Any = None
        self._previous_thread_hook: Any = None
        self._previous_unraisable_hook: Any = None
        self._sys_hook: Any = None
        self._thread_hook: Any = None
        self._unraisable_hook: Any = None

    @property
    def output_resource_uri(self) -> Optional[str]:
        return getattr(self.output_capture, "resource_uri", None)

    @property
    def events_resource_uri(self) -> Optional[str]:
        return getattr(self.session_events, "resource_uri", None)

    def install(self) -> None:
        if self._installed:
            return
        self._installed = True
        logging.getLogger().addHandler(self._logging_handler)

        self._previous_sys_hook = sys.excepthook

        def _sys_hook(exc_type: type, exc: BaseException, tb: Any) -> None:
            if self._installed:
                self.report_exception(exc_type, exc, tb, source="sys.excepthook")
            self._previous_sys_hook(exc_type, exc, tb)

        self._sys_hook = _sys_hook
        sys.excepthook = _sys_hook

        previous_thread_hook = getattr(threading, "excepthook", None)
        if previous_thread_hook is not None:
            self._previous_thread_hook = previous_thread_hook

            def _thread_hook(args: Any) -> None:
                if self._installed:
                    self.report_exception(
                        args.exc_type,
                        args.exc_value,
                        args.exc_traceback,
                        source="threading.excepthook",
                        metadata={"thread": getattr(args.thread, "name", None)},
                    )
                self._previous_thread_hook(args)

            self._thread_hook = _thread_hook
            threading.excepthook = _thread_hook  # type: ignore[attr-defined]

        previous_unraisable_hook = getattr(sys, "unraisablehook", None)
        if previous_unraisable_hook is not None:
            self._previous_unraisable_hook = previous_unraisable_hook

            def _unraisable_hook(args: Any) -> None:
                if self._installed:
                    self.report_exception(
                        args.exc_type,
                        args.exc_value,
                        args.exc_traceback,
                        source="sys.unraisablehook",
                        metadata={"error_message": getattr(args, "err_msg", None)},
                    )
                self._previous_unraisable_hook(args)

            self._unraisable_hook = _unraisable_hook
            sys.unraisablehook = _unraisable_hook  # type: ignore[attr-defined]

    def close(self) -> None:
        if not self._installed:
            return
        self._installed = False
        logging.getLogger().removeHandler(self._logging_handler)
        if sys.excepthook is self._sys_hook:
            sys.excepthook = self._previous_sys_hook
        if getattr(threading, "excepthook", None) is self._thread_hook:
            threading.excepthook = self._previous_thread_hook  # type: ignore[attr-defined]
        if getattr(sys, "unraisablehook", None) is self._unraisable_hook:
            sys.unraisablehook = self._previous_unraisable_hook  # type: ignore[attr-defined]

    def report_exception(
        self,
        exc_type: type,
        exc: BaseException,
        tb: Any,
        *,
        source: str,
        phase: str = "runtime",
        message: Optional[str] = None,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        return self.report(
            message or str(exc) or exc_type.__name__,
            source=source,
            stream="stderr",
            phase=phase,
            exception_type=f"{exc_type.__module__}.{exc_type.__name__}",
            traceback_text=_format_exception(exc_type, exc, tb),
            metadata=metadata,
        )

    def report(
        self,
        message: str,
        *,
        source: str = "host",
        stream: str = "stderr",
        phase: str = "runtime",
        exception_type: Optional[str] = None,
        traceback_text: Optional[str] = None,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        message = _truncate(str(message), _MAX_MESSAGE_CHARS) or "DCC host error"
        traceback_text = _truncate(traceback_text, _MAX_TRACEBACK_CHARS)
        event: Dict[str, Any] = {
            "event": "dcc_host_error",
            "message": message,
            "dcc_type": self.dcc_name,
            "dcc_pid": self.dcc_pid,
            "instance_id": self.instance_id,
            "phase": str(phase),
            "source": str(source),
            "stream": str(stream),
            "level": "error",
            "exception_type": exception_type,
            "traceback": traceback_text,
            "adapter_version": self.adapter_version,
            "core_version": self.core_version,
            "python_version": ".".join(str(part) for part in sys.version_info[:3]),
            "metadata": metadata or {},
        }
        _write_host_error_event(
            event,
            log_dir=self.log_dir,
            enabled=self.persist_to_file,
        )
        output_stream = stream if stream in {"stdout", "stderr", "script_editor"} else "stderr"
        output_text = traceback_text or message
        if self.output_capture is not None:
            with contextlib.suppress(Exception):
                self.output_capture.push(output_stream, output_text)
        if self.session_events is not None:
            event_metadata = {
                "phase": event["phase"],
                "dcc_pid": self.dcc_pid,
                "exception_type": exception_type,
                "traceback": traceback_text,
                "adapter_version": self.adapter_version,
                "core_version": self.core_version,
            }
            if metadata:
                event_metadata["details"] = metadata
            with contextlib.suppress(Exception):
                self.session_events.append(
                    source=str(source),
                    stream=str(stream),
                    message=message,
                    level="error",
                    metadata=_json_safe(event_metadata),
                )
        if callable(self._notify_updated):
            for uri in (self.output_resource_uri, self.events_resource_uri):
                if uri:
                    with contextlib.suppress(Exception):
                        self._notify_updated(uri)
        return event


__all__ = ["capture_bootstrap_errors", "record_bootstrap_error"]
