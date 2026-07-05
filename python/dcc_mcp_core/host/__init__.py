"""dcc_mcp_core.host: cross-DCC main-thread dispatcher primitives.

Re-exports the Rust-backed dispatcher classes from ``dcc_mcp_core._core`` and
provides the pure-Python :class:`StandaloneHost` driver for running a
dispatcher tick loop on a dedicated thread (useful in tests, CLI scripts, and
any environment without a real DCC main loop).

A typical integration looks like::

    from dcc_mcp_core.host import QueueDispatcher, StandaloneHost

    dispatcher = QueueDispatcher()
    with StandaloneHost(dispatcher):
        handle = dispatcher.post(lambda: some_work())
        result = handle.wait(timeout=5.0)

For real DCCs (Blender, Maya, Houdini, 3ds Max) the DCC adapter replaces
``StandaloneHost`` with a thin wrapper that calls ``dispatcher.tick(...)``
from the host's native idle primitive (``bpy.app.timers.register``,
``maya.utils.executeDeferred``, ``hou.ui.addEventLoopCallback``, etc.).
"""

# Import future modules
from __future__ import annotations

# Import local modules — Rust-backed primitives live in _core when available.
try:
    from dcc_mcp_core._core import BlockingDispatcher
    from dcc_mcp_core._core import DispatchError
    from dcc_mcp_core._core import PostHandle
    from dcc_mcp_core._core import QueueDispatcher
    from dcc_mcp_core._core import TickOutcome
except ImportError:
    BlockingDispatcher = None
    DispatchError = None
    PostHandle = None
    QueueDispatcher = None
    TickOutcome = None
from dcc_mcp_core.host._adapter import HostAdapter
from dcc_mcp_core.host._adapter import TickableDispatcher
from dcc_mcp_core.host._standalone import StandaloneHost
from dcc_mcp_core.host._wire import normalize_tool_arguments
from dcc_mcp_core.host._wire import normalize_tool_meta
from dcc_mcp_core.host.qt_dispatcher import QtCommandServer
from dcc_mcp_core.host.qt_dispatcher import ServerHandle
from dcc_mcp_core.host.qt_dispatcher import current_server
from dcc_mcp_core.host.qt_dispatcher import start_qt_server
from dcc_mcp_core.host.qt_dispatcher import stop_qt_server

__all__ = [
    "BlockingDispatcher",
    "DispatchError",
    "HostAdapter",
    "PostHandle",
    "QtCommandServer",
    "QueueDispatcher",
    "ServerHandle",
    "StandaloneHost",
    "TickOutcome",
    "TickableDispatcher",
    "current_server",
    "normalize_tool_arguments",
    "normalize_tool_meta",
    "start_qt_server",
    "stop_qt_server",
]
