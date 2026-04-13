"""Standard diagnostic IPC action handlers for DCC MCP servers.

Any DCC adapter (Maya, Blender, Houdini, Unreal, ZBrush …) can call
:func:`register_diagnostic_handlers` from its server startup code to
expose three built-in IPC actions that the ``dcc-diagnostics`` and
``workflow`` skills use for live data retrieval.

Registered handlers
-------------------
``get_audit_log``
    Returns entries from the server-level :class:`SandboxContext` audit log.
    Supports ``filter`` (all/success/denied/error) and ``action_name`` filters.

``get_action_metrics``
    Returns per-action performance counters from the shared
    :class:`ActionRecorder`.  Optionally filtered to a single action name.

``dispatch_action``
    Relays a ``{"action": "...", "params": {...}}`` request through the
    server's internal dispatcher.  Used by ``workflow__run_chain`` to
    execute multi-step chains via IPC without spawning extra sub-processes.

IPC address convention
----------------------
:func:`register_diagnostic_handlers` also sets ``DCC_MCP_IPC_ADDRESS`` in
the process environment (unless already set externally) so that skill
subprocesses launched by the server can auto-discover the IPC endpoint via
``os.environ["DCC_MCP_IPC_ADDRESS"]``.

Usage example
-------------
In your DCC adapter's server startup code::

    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    class BlenderMcpServer:
        def __init__(self):
            from dcc_mcp_core import McpHttpConfig, create_skill_manager
            self._server = create_skill_manager("blender", McpHttpConfig())

        def start(self):
            register_diagnostic_handlers(self._server, dcc_name="blender")
            return self._server.start()

The ``dcc_name`` argument is used to derive the default IPC pipe name when
``DCC_MCP_IPC_ADDRESS`` is not already set.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Any

logger = logging.getLogger(__name__)

# ── module-level shared state (one per process) ────────────────────────────
# Populated by register_diagnostic_handlers().
_sandbox_context: Any = None  # SandboxContext | None
_action_recorder: Any = None  # ActionRecorder | None
_dispatcher_ref: Any = None  # ActionDispatcher | None


def _get_sandbox_context() -> Any:
    """Return the shared SandboxContext, creating one lazily if needed."""
    global _sandbox_context
    if _sandbox_context is None:
        try:
            from dcc_mcp_core._core import SandboxContext
            from dcc_mcp_core._core import SandboxPolicy

            policy = SandboxPolicy()
            _sandbox_context = SandboxContext(policy)
        except Exception as exc:
            logger.debug("Failed to create SandboxContext: %s", exc)
    return _sandbox_context


def _get_action_recorder(dcc_name: str = "dcc") -> Any:
    """Return the shared ActionRecorder, creating one lazily if needed."""
    global _action_recorder
    if _action_recorder is None:
        try:
            from dcc_mcp_core._core import ActionRecorder

            _action_recorder = ActionRecorder(f"dcc-mcp-{dcc_name}")
        except Exception as exc:
            logger.debug("Failed to create ActionRecorder: %s", exc)
    return _action_recorder


# ── handler implementations ────────────────────────────────────────────────


def _handle_get_audit_log(params_json: str) -> str:
    """Return audit log entries as a JSON string."""
    try:
        params = json.loads(params_json) if params_json else {}
    except json.JSONDecodeError:
        params = {}

    filter_ = params.get("filter", "all")
    action_name = params.get("action_name")
    limit = int(params.get("limit", 50))

    ctx = _get_sandbox_context()
    if ctx is None:
        return json.dumps({"success": False, "message": "SandboxContext not available."})

    try:
        audit = ctx.audit_log
        if action_name:
            entries = audit.entries_for_action(action_name)
        elif filter_ == "success":
            entries = audit.successes()
        elif filter_ == "denied":
            entries = audit.denials()
        else:
            entries = audit.entries()

        total = len(entries)
        serialized = []
        for entry in entries[:limit]:
            try:
                serialized.append(
                    {
                        "action": entry.action,
                        "outcome": entry.outcome,
                        "timestamp_ms": getattr(entry, "timestamp_ms", None),
                        "details": getattr(entry, "details", None),
                    }
                )
            except Exception:
                serialized.append(str(entry))

        return json.dumps(
            {
                "success": True,
                "total_entries": total,
                "entries": serialized,
                "source": "dcc-ipc",
            }
        )
    except Exception as exc:
        logger.warning("get_audit_log handler error: %s", exc)
        return json.dumps({"success": False, "message": str(exc)})


def _handle_get_action_metrics(params_json: str) -> str:
    """Return ActionRecorder metrics as a JSON string."""
    try:
        params = json.loads(params_json) if params_json else {}
    except json.JSONDecodeError:
        params = {}

    action_name = params.get("action_name")

    recorder = _get_action_recorder()
    if recorder is None:
        return json.dumps({"success": False, "message": "ActionRecorder not available."})

    try:
        if action_name:
            metric = recorder.metrics(action_name)
            metrics_list = [_metric_to_dict(metric)] if metric else []
        else:
            metrics_list = [_metric_to_dict(m) for m in recorder.all_metrics()]

        return json.dumps(
            {
                "success": True,
                "metrics": metrics_list,
                "source": "dcc-ipc",
            }
        )
    except Exception as exc:
        logger.warning("get_action_metrics handler error: %s", exc)
        return json.dumps({"success": False, "message": str(exc)})


def _handle_dispatch_action(params_json: str) -> str:
    """Relay a dispatch request through the server's ActionDispatcher."""
    try:
        params = json.loads(params_json) if params_json else {}
    except json.JSONDecodeError:
        return json.dumps({"success": False, "message": "Invalid JSON params."})

    action = params.get("action", "")
    action_params = params.get("params", {})

    if not action:
        return json.dumps({"success": False, "message": "Missing 'action' field."})

    dispatcher = _dispatcher_ref
    if dispatcher is None:
        return json.dumps({"success": False, "message": "Dispatcher not available."})

    try:
        result = dispatcher.dispatch(action, json.dumps(action_params))
        output = result.get("output", "{}")
        if isinstance(output, str):
            return output  # already JSON
        return json.dumps(output)
    except Exception as exc:
        logger.warning("dispatch_action handler error for '%s': %s", action, exc)
        return json.dumps({"success": False, "message": str(exc)})


# ── public API ────────────────────────────────────────────────────────────


def register_diagnostic_handlers(
    server: Any,
    *,
    dispatcher: Any = None,
    dcc_name: str = "dcc",
) -> None:
    """Register the three standard diagnostic IPC action handlers on *server*.

    Also sets ``DCC_MCP_IPC_ADDRESS`` in the process environment (unless it
    is already set externally) so that skill subprocesses inherit the IPC
    address and can call back into this server.

    This function is idempotent: calling it multiple times on the same server
    overwrites previously registered handlers with the same names.

    Args:
        server: A :class:`dcc_mcp_core.McpHttpServer` / skill-manager object
            that exposes a ``register_handler(name, callable)`` method.
        dispatcher: Optional :class:`dcc_mcp_core.ActionDispatcher` used for
            the ``dispatch_action`` relay handler.  When ``None``, dispatch
            relay calls return an error response.
        dcc_name: Short DCC identifier used for the IPC pipe name derivation
            and ``ActionRecorder`` label (e.g. ``"maya"``, ``"blender"``).

    Example::

        from dcc_mcp_core.dcc_server import register_diagnostic_handlers

        # In your DCC adapter server startup:
        register_diagnostic_handlers(my_server, dispatcher=my_dispatcher, dcc_name="blender")

    Registered actions
    ------------------
    - ``get_audit_log`` — sandbox audit log entries
    - ``get_action_metrics`` — ActionRecorder performance counters
    - ``dispatch_action`` — relay through the server's ActionDispatcher

    """
    global _dispatcher_ref
    if dispatcher is not None:
        _dispatcher_ref = dispatcher

    # Ensure action recorder uses the correct DCC name
    _get_action_recorder(dcc_name)

    try:
        server.register_handler("get_audit_log", _handle_get_audit_log)
        server.register_handler("get_action_metrics", _handle_get_action_metrics)
        server.register_handler("dispatch_action", _handle_dispatch_action)
        logger.debug(
            "Registered diagnostic IPC handlers for dcc=%r: get_audit_log, get_action_metrics, dispatch_action",
            dcc_name,
        )
    except Exception as exc:
        logger.warning("Failed to register diagnostic handlers: %s", exc)
        return

    _set_ipc_address_env(dcc_name)


def _set_ipc_address_env(dcc_name: str = "dcc") -> None:
    """Derive and export ``DCC_MCP_IPC_ADDRESS`` for skill subprocesses."""
    if os.environ.get("DCC_MCP_IPC_ADDRESS"):
        return  # respect any externally-configured override

    try:
        from dcc_mcp_core._core import TransportAddress

        addr = TransportAddress.default_local(dcc_name, os.getpid())
        addr_str = str(addr)
        os.environ["DCC_MCP_IPC_ADDRESS"] = addr_str
        logger.debug("Set DCC_MCP_IPC_ADDRESS=%s (dcc=%r)", addr_str, dcc_name)
    except Exception as exc:
        logger.debug("Could not derive default IPC address for dcc=%r: %s", dcc_name, exc)


# ── internal helpers ──────────────────────────────────────────────────────


def _metric_to_dict(metric: Any) -> dict:
    return {
        "action_name": metric.action_name,
        "invocation_count": metric.invocation_count,
        "success_count": metric.success_count,
        "failure_count": metric.failure_count,
        "success_rate": round(metric.success_rate(), 4),
        "avg_duration_ms": round(metric.avg_duration_ms, 2),
        "p95_duration_ms": round(metric.p95_duration_ms, 2),
        "p99_duration_ms": round(metric.p99_duration_ms, 2),
    }
