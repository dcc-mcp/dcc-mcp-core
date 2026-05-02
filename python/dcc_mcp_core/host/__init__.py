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

# Import local modules — Rust-backed primitives live in _core.
from dcc_mcp_core._core import BlockingDispatcher
from dcc_mcp_core._core import DispatchError
from dcc_mcp_core._core import PostHandle
from dcc_mcp_core._core import QueueDispatcher
from dcc_mcp_core._core import TickOutcome
from dcc_mcp_core.host._standalone import StandaloneHost

__all__ = [
    "BlockingDispatcher",
    "DispatchError",
    "PostHandle",
    "QueueDispatcher",
    "StandaloneHost",
    "TickOutcome",
]
