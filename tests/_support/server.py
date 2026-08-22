"""Construct test-only ``DccServerBase`` shells without loading a DCC."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core._lifecycle_events import LifecycleEventDispatcher
from dcc_mcp_core._server import ExecutionBridgeBinder
from dcc_mcp_core._server import LifecycleController
from dcc_mcp_core._server import ObservabilityFacade
from dcc_mcp_core._server import ServerLifecycleController
from dcc_mcp_core._server import ServerRuntimeController
from dcc_mcp_core._server import SkillDiscoveryController
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
    """Build a ``DccServerBase`` shell with its standard collaborators."""
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
            "_skill_discovery": SkillDiscoveryController(obj),
            "_execution": ExecutionBridgeBinder(obj),
            "_observability": ObservabilityFacade(obj),
        }
    )
    if extra_attrs:
        obj.__dict__.update(extra_attrs)
    obj.__dict__.setdefault("_lifecycle", ServerLifecycleController(obj))
    obj.__dict__.setdefault("_runtime", ServerRuntimeController(obj))
    obj.__dict__.setdefault("_lifecycle_ctrl", LifecycleController(obj))
    return obj


__all__ = ["make_test_server"]
