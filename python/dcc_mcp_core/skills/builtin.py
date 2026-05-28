"""Built-in skill registration for DCC MCP servers.

This module centralises the registration of standard, host-local tools that
should be available on every DCC adapter by default (diagnostics, introspect,
feedback, recipes, and Qt introspection).
"""

from __future__ import annotations

import logging
from typing import Any
from typing import Callable

from dcc_mcp_core.dcc_server import register_diagnostic_mcp_tools
from dcc_mcp_core.feedback import register_feedback_tool
from dcc_mcp_core.introspect import register_introspect_tools
from dcc_mcp_core.recipes import register_recipes_tools
from dcc_mcp_core.script_materialization_tools import register_script_materialization_tools
from dcc_mcp_core.skills.qt_ui_inspector import register_qt_ui_inspector

logger = logging.getLogger(__name__)


def register_all_builtin_skills(
    server: Any,
    *,
    dcc_name: str = "dcc",
    dcc_pid: int | None = None,
    dcc_window_handle: int | None = None,
    dcc_window_title: str | None = None,
    gateway_failover_resolver: Callable[[], dict[str, Any]] | None = None,
) -> None:
    """Register all standard built-in tools on *server*.

    This includes:
    * ``dcc_diagnostics__*`` (screenshot, audit, metrics)
    * ``dcc_introspect__*`` (signature, search, eval)
    * ``dcc_feedback__*`` (report)
    * ``dcc_recipes__*`` (list, get)
    * ``qt_ui_inspector__*`` (lazy-loaded Qt tools)
    * ``materialize__*`` (script materialization)

    The call is idempotent; re-registering the same tools on the same
    server is safe.
    """
    logger.debug("[%s] Registering all built-in skills", dcc_name)

    # 1. Diagnostics (audit log, metrics, screenshot, gateway failover)
    register_diagnostic_mcp_tools(
        server,
        dcc_name=dcc_name,
        dcc_pid=dcc_pid,
        dcc_window_handle=dcc_window_handle,
        dcc_window_title=dcc_window_title,
        gateway_failover_resolver=gateway_failover_resolver,
    )

    # 2. Introspection (signature, search, eval)
    register_introspect_tools(server, dcc_name=dcc_name)

    # 3. Agent feedback
    register_feedback_tool(server, dcc_name=dcc_name)

    # 4. Recipes
    register_recipes_tools(server, dcc_name=dcc_name)

    # 5. Qt UI inspector (opt-in default capability)
    register_qt_ui_inspector(server, dcc_name=dcc_name)

    # 6. Script materialization
    register_script_materialization_tools(
        server,
        dcc_name=dcc_name,
        instance_id=str(dcc_pid or dcc_name),
    )
