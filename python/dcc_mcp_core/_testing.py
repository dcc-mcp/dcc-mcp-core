"""Test-only helpers for dcc-mcp-core.

This module is intentionally separate from production code: it lets tests build
``DccServerBase`` instances without going through the real ``__init__`` (which
would require a running DCC and the compiled Rust core), while keeping the
production class free of test-aware fallback paths.

See issue #851 for the rationale.
"""

from __future__ import annotations

from typing import Any

from dcc_mcp_core._lifecycle_events import LifecycleEventDispatcher
from dcc_mcp_core._server import ServerLifecycleController
from dcc_mcp_core._server import ServerRuntimeController
from dcc_mcp_core._server import SkillQueryClient
from dcc_mcp_core._server import WindowResolver


def make_test_server(
    *,
    server: Any,
    dcc_name: str,
    dcc_pid: int = 0,
    dcc_window_handle: int | None = None,
    dcc_window_title: str | None = None,
    **extra_attrs: Any,
) -> Any:
    """Build a ``DccServerBase`` shell suitable for unit tests.

    Bypasses the real ``__init__`` (which needs a running DCC + the compiled
    ``_core`` extension) and pre-populates the collaborators that the rest of
    the class assumes exist.

    Any additional keyword arguments are written straight onto ``__dict__`` —
    handy for test cases that want to wire fakes for ``_config``,
    ``_hot_reloader``, etc.

    Parameters
    ----------
    server:
        The inner DCC server stub (typically a fake/mock).
    dcc_name:
        The DCC type name (e.g. ``"maya"``).
    dcc_pid:
        Optional process id, defaults to 0.
    dcc_window_handle:
        Optional native window handle.
    dcc_window_title:
        Optional native window title.
    **extra_attrs:
        Additional attributes to set on the instance ``__dict__``.

    Returns
    -------
    DccServerBase
        A bare instance with the standard collaborators wired up.

    """
    # Local import to avoid a circular import at module load time.
    from dcc_mcp_core.server_base import DccServerBase

    obj = DccServerBase.__new__(DccServerBase)
    obj.__dict__.update(
        {
            "_server": server,
            "_dcc_name": dcc_name,
            "_dcc_pid": dcc_pid,
            "_dcc_window_handle": dcc_window_handle,
            "_dcc_window_title": dcc_window_title,
            "_skill_client": SkillQueryClient(server, dcc_name),
            "_lifecycle_events": LifecycleEventDispatcher(
                dcc_name,
                lambda: getattr(obj, "_lifecycle_hooks", None),
            ),
            "_window_resolver": WindowResolver(
                dcc_name=dcc_name,
                dcc_pid=dcc_pid,
                dcc_window_handle=dcc_window_handle,
                dcc_window_title=dcc_window_title,
            ),
        }
    )
    if extra_attrs:
        obj.__dict__.update(extra_attrs)
    obj.__dict__.setdefault("_lifecycle", ServerLifecycleController(obj))
    obj.__dict__.setdefault("_runtime", ServerRuntimeController(obj))
    return obj


__all__ = ["make_test_server"]
