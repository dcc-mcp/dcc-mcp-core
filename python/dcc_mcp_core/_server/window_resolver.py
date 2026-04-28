"""DCC window-handle resolver collaborator for :class:`DccServerBase` (#486).

Looks up the native window handle (HWND on Windows, XID on X11) for the
DCC application that hosts this MCP server, in priority order:

1. Explicit ``dcc_window_handle`` provided at construction time.
2. Cached lookup from a previous successful resolution.
3. PID-based lookup via :class:`WindowFinder` + ``CaptureTarget.process_id``.
4. Window-title substring lookup via ``CaptureTarget.window_title``.
5. ``None`` if everything fails.

Any exception during lookup is swallowed and a DEBUG log is emitted: the
diagnostics tools that consume the handle (screenshot capture, audit log)
all gracefully fall back to a process-wide capture when the handle is
``None``.
"""

from __future__ import annotations

import logging

logger = logging.getLogger(__name__)


class WindowResolver:
    """Resolve and cache the DCC window handle on demand."""

    def __init__(
        self,
        *,
        dcc_name: str,
        dcc_pid: int,
        dcc_window_handle: int | None = None,
        dcc_window_title: str | None = None,
    ) -> None:
        self._dcc_name = dcc_name
        self._dcc_pid = dcc_pid
        self._dcc_window_handle = dcc_window_handle
        self._dcc_window_title = dcc_window_title
        self._cached_hwnd: int | None = None

    @property
    def dcc_pid(self) -> int:
        return self._dcc_pid

    @property
    def dcc_window_title(self) -> str | None:
        return self._dcc_window_title

    @property
    def dcc_window_handle(self) -> int | None:
        return self._dcc_window_handle

    def resolve(self) -> int | None:
        """Return the resolved DCC window handle, or ``None`` if unavailable."""
        if self._dcc_window_handle is not None:
            return self._dcc_window_handle
        if self._cached_hwnd is not None:
            return self._cached_hwnd
        try:
            from dcc_mcp_core import CaptureTarget
            from dcc_mcp_core import WindowFinder

            finder = WindowFinder()
            info = None
            if self._dcc_pid:
                info = finder.find(CaptureTarget.process_id(self._dcc_pid))
            if info is None and self._dcc_window_title:
                info = finder.find(CaptureTarget.window_title(self._dcc_window_title))
            if info is not None:
                self._cached_hwnd = int(info.handle)
            return self._cached_hwnd
        except Exception as exc:
            logger.debug("[%s] _resolve_window_handle failed: %s", self._dcc_name, exc)
            return None
