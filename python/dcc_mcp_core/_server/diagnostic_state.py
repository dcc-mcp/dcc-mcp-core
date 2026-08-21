"""Instance-owned mutable state for DCC diagnostic handlers."""

from __future__ import annotations

import logging
import threading
from typing import Any

logger = logging.getLogger(__name__)


def _empty_instance_context(dcc_name: str | None = None) -> dict[str, Any]:
    return {
        "dcc_name": dcc_name,
        "dcc_pid": None,
        "dcc_window_handle": None,
        "dcc_window_title": None,
        "resolver": None,
        "gateway_failover_resolver": None,
        "dcc_version": None,
    }


class DiagnosticRuntimeState:
    """Own lazy diagnostics collaborators for one DCC server instance."""

    def __init__(self, dcc_name: str = "dcc") -> None:
        self._lock = threading.RLock()
        self.instance_context = _empty_instance_context(dcc_name)
        self.sandbox_context: Any = None
        self.action_recorder: Any = None
        self.action_recorder_dcc_name: str | None = None
        self.dispatcher: Any = None
        self.server: Any = None
        self.window_capturer: Any = None
        self.full_capturer: Any = None

    def configure_instance(self, **values: Any) -> None:
        """Update this instance's diagnostic context atomically."""
        with self._lock:
            self.instance_context.update(values)

    def snapshot_instance_context(self) -> dict[str, Any]:
        """Return a consistent shallow snapshot for one handler invocation."""
        with self._lock:
            return dict(self.instance_context)

    def get_sandbox_context(self) -> Any:
        """Return this instance's lazily-created sandbox context."""
        with self._lock:
            if self.sandbox_context is None:
                try:
                    from dcc_mcp_core._core import SandboxContext
                    from dcc_mcp_core._core import SandboxPolicy

                    self.sandbox_context = SandboxContext(SandboxPolicy())
                except Exception as exc:
                    logger.debug("Failed to create SandboxContext: %s", exc)
            return self.sandbox_context

    def get_action_recorder(self, dcc_name: str | None = None) -> Any:
        """Return a recorder whose namespace matches the requested DCC."""
        with self._lock:
            requested = dcc_name or self.instance_context.get("dcc_name") or "dcc"
            if self.action_recorder is None or self.action_recorder_dcc_name != requested:
                self.action_recorder = None
                self.action_recorder_dcc_name = None
                try:
                    from dcc_mcp_core._core import ToolRecorder

                    self.action_recorder = ToolRecorder(f"dcc-mcp-{requested}")
                    self.action_recorder_dcc_name = requested
                except Exception as exc:
                    logger.debug("Failed to create ToolRecorder: %s", exc)
            return self.action_recorder

    def get_window_capturer(self) -> Any:
        """Return this instance's cached window capturer."""
        with self._lock:
            if self.window_capturer is None:
                from dcc_mcp_core import Capturer

                self.window_capturer = Capturer.new_window_auto()
            return self.window_capturer

    def get_full_capturer(self) -> Any:
        """Return this instance's cached full-screen capturer."""
        with self._lock:
            if self.full_capturer is None:
                from dcc_mcp_core import Capturer

                self.full_capturer = Capturer.new_auto()
            return self.full_capturer

    def reset_for_tests(self, dcc_name: str = "dcc") -> None:
        """Clear all mutable collaborators without replacing this object."""
        with self._lock:
            self.instance_context.clear()
            self.instance_context.update(_empty_instance_context(dcc_name))
            self.sandbox_context = None
            self.action_recorder = None
            self.action_recorder_dcc_name = None
            self.dispatcher = None
            self.server = None
            self.window_capturer = None
            self.full_capturer = None


_DEFAULT_DIAGNOSTIC_STATE = DiagnosticRuntimeState()


def get_default_diagnostic_state() -> DiagnosticRuntimeState:
    """Return the compatibility state used by standalone registration APIs."""
    return _DEFAULT_DIAGNOSTIC_STATE


def reset_default_diagnostic_state_for_tests() -> None:
    """Reset standalone compatibility state between tests."""
    _DEFAULT_DIAGNOSTIC_STATE.reset_for_tests()


__all__ = [
    "DiagnosticRuntimeState",
    "get_default_diagnostic_state",
    "reset_default_diagnostic_state_for_tests",
]
