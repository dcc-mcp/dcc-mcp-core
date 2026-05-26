"""Public host namespace for the universal Qt dispatcher.

The implementation lives in :mod:`dcc_mcp_core.qt_dispatcher` so the Python
package and the Rust ``qtserver://`` bootstrap include the same source file.
"""

from __future__ import annotations

from dcc_mcp_core.qt_dispatcher import DISPATCHER_VERSION
from dcc_mcp_core.qt_dispatcher import QtCommandServer
from dcc_mcp_core.qt_dispatcher import ServerHandle
from dcc_mcp_core.qt_dispatcher import current_server
from dcc_mcp_core.qt_dispatcher import start_qt_server
from dcc_mcp_core.qt_dispatcher import stop_qt_server

__all__ = [
    "DISPATCHER_VERSION",
    "QtCommandServer",
    "ServerHandle",
    "current_server",
    "start_qt_server",
    "stop_qt_server",
]
