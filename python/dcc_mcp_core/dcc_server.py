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
    :class:`ToolRecorder`.  Optionally filtered to a single action name.

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
            from dcc_mcp_core import McpHttpConfig, create_skill_server
            self._server = create_skill_server("blender", McpHttpConfig())

        def start(self):
            register_diagnostic_handlers(self._server, dcc_name="blender")
            return self._server.start()

The ``dcc_name`` argument is used to derive the default IPC pipe name when
``DCC_MCP_IPC_ADDRESS`` is not already set.
"""

from __future__ import annotations

import base64
import json
import logging
import os
import time
from typing import Any
from typing import Callable

logger = logging.getLogger(__name__)

# ── module-level shared state (one per process) ────────────────────────────
# Populated by register_diagnostic_handlers().
_sandbox_context: Any = None  # SandboxContext | None
_action_recorder: Any = None  # ToolRecorder | None
_dispatcher_ref: Any = None  # ToolDispatcher | None

# Diagnostic instance context (DCC PID / window / resolver callback).
_instance_context: dict[str, Any] = {
    "dcc_name": None,
    "dcc_pid": None,
    "dcc_window_handle": None,
    "dcc_window_title": None,
    "resolver": None,
}

# Lazily-constructed capturers (one per kind, reused across screenshots).
_capturer_window: Any = None
_capturer_full: Any = None


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
    """Return the shared ToolRecorder, creating one lazily if needed."""
    global _action_recorder
    if _action_recorder is None:
        try:
            from dcc_mcp_core._core import ToolRecorder

            _action_recorder = ToolRecorder(f"dcc-mcp-{dcc_name}")
        except Exception as exc:
            logger.debug("Failed to create ToolRecorder: %s", exc)
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
    """Return ToolRecorder metrics as a JSON string."""
    try:
        params = json.loads(params_json) if params_json else {}
    except json.JSONDecodeError:
        params = {}

    action_name = params.get("action_name")

    recorder = _get_action_recorder()
    if recorder is None:
        return json.dumps({"success": False, "message": "ToolRecorder not available."})

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


def _get_window_capturer() -> Any:
    """Return (and cache) a window-target ``Capturer`` instance."""
    global _capturer_window
    if _capturer_window is None:
        from dcc_mcp_core import Capturer

        _capturer_window = Capturer.new_window_auto()
    return _capturer_window


def _get_full_capturer() -> Any:
    """Return (and cache) a full-screen ``Capturer`` instance."""
    global _capturer_full
    if _capturer_full is None:
        from dcc_mcp_core import Capturer

        _capturer_full = Capturer.new_auto()
    return _capturer_full


def _handle_take_screenshot(params_json: str) -> str:
    """Capture a screenshot of the owning DCC window (or the full screen).

    Params (all optional):
        format (str): ``"png"`` (default), ``"jpeg"``, or ``"raw_bgra"``.
        jpeg_quality (int): 0-100, default 85.
        scale (float): 0.0-1.0, default 1.0.
        timeout_ms (int): default 5000.
        full_screen (bool): capture the whole desktop instead of the window.
        process_id (int): override the DCC PID for this call.
        window_handle (int): override the native window handle.
        window_title (str): override the window title substring.
    """
    try:
        params = json.loads(params_json) if params_json else {}
    except json.JSONDecodeError:
        params = {}

    fmt = params.get("format", "png")
    quality = int(params.get("jpeg_quality", 85))
    scale = float(params.get("scale", 1.0))
    timeout_ms = int(params.get("timeout_ms", 5000))
    full_screen = bool(params.get("full_screen", False))
    hwnd = params.get("window_handle") or _instance_context.get("dcc_window_handle")
    pid = params.get("process_id") or _instance_context.get("dcc_pid")
    title = params.get("window_title") or _instance_context.get("dcc_window_title")

    try:
        if full_screen:
            cap = _get_full_capturer()
            frame = cap.capture(
                format=fmt,
                jpeg_quality=quality,
                scale=scale,
                timeout_ms=timeout_ms,
            )
        else:
            if not hwnd:
                resolver = _instance_context.get("resolver")
                if callable(resolver):
                    try:
                        hwnd = resolver()
                    except Exception as exc:
                        logger.debug("take_screenshot: resolver failed: %s", exc)
            cap = _get_window_capturer()
            frame = cap.capture_window(
                process_id=pid,
                window_handle=hwnd,
                window_title=title,
                format=fmt,
                jpeg_quality=quality,
                scale=scale,
                timeout_ms=timeout_ms,
            )
        payload = {
            "success": True,
            "message": f"Captured {frame.width}x{frame.height} {fmt}",
            "format": frame.format,
            "width": frame.width,
            "height": frame.height,
            "mime_type": frame.mime_type,
            "byte_len": frame.byte_len(),
            "image_base64": base64.b64encode(frame.data).decode("ascii"),
            "window_rect": list(frame.window_rect) if frame.window_rect else None,
            "window_title": frame.window_title,
            "timestamp_ms": int(time.time() * 1000),
            "source": "dcc-ipc",
        }
    except Exception as exc:
        logger.warning("take_screenshot handler error: %s", exc)
        payload = {
            "success": False,
            "message": str(exc),
            "error": type(exc).__name__,
            "source": "dcc-ipc",
        }
    return json.dumps(payload)


def _handle_process_status(params_json: str) -> str:
    """Report adapter process health and DCC instance context.

    Params (all optional) are ignored; the handler reads state from
    ``_instance_context`` and the current Python process.
    """
    _ = params_json
    ctx = _instance_context
    dcc_pid = ctx.get("dcc_pid")
    dcc_name = ctx.get("dcc_name") or "dcc"

    alive = True
    try:
        if dcc_pid:
            if os.name == "nt":
                import ctypes

                PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
                handle = ctypes.windll.kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, int(dcc_pid))
                if handle:
                    ctypes.windll.kernel32.CloseHandle(handle)
                    alive = True
                else:
                    alive = False
            else:
                os.kill(int(dcc_pid), 0)
    except OSError:
        alive = False
    except Exception as exc:
        logger.debug("process_status alive-check failed: %s", exc)

    payload = {
        "success": True,
        "dcc_name": dcc_name,
        "dcc_pid": dcc_pid,
        "dcc_window_handle": ctx.get("dcc_window_handle"),
        "dcc_window_title": ctx.get("dcc_window_title"),
        "adapter_pid": os.getpid(),
        "dcc_alive": alive,
        "timestamp_ms": int(time.time() * 1000),
    }
    return json.dumps(payload)


def _handle_dispatch_action(params_json: str) -> str:
    """Relay a dispatch request through the server's ToolDispatcher."""
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
    dcc_pid: int | None = None,
    dcc_window_handle: int | None = None,
    dcc_window_title: str | None = None,
    resolver: Callable[[], int | None] | None = None,
) -> None:
    """Register the standard diagnostic IPC action handlers on *server*.

    Also sets ``DCC_MCP_IPC_ADDRESS`` in the process environment (unless it
    is already set externally) so that skill subprocesses inherit the IPC
    address and can call back into this server.

    This function is idempotent: calling it multiple times on the same server
    overwrites previously registered handlers with the same names.

    Args:
        server: A :class:`dcc_mcp_core.McpHttpServer` / skill-manager object
            that exposes a ``register_handler(name, callable)`` method.
        dispatcher: Optional :class:`dcc_mcp_core.ToolDispatcher` used for
            the ``dispatch_action`` relay handler.  When ``None``, dispatch
            relay calls return an error response.
        dcc_name: Short DCC identifier used for the IPC pipe name derivation
            and ``ToolRecorder`` label (e.g. ``"maya"``, ``"blender"``).
        dcc_pid: PID of the DCC application process. Used by
            ``take_screenshot`` to resolve the window target.
        dcc_window_handle: Pre-resolved native window handle (HWND / XID).
        dcc_window_title: Substring of the DCC window title for title lookup.
        resolver: Optional callback returning the current native window handle
            when neither ``dcc_window_handle`` nor a cache hit is available.

    Example::

        from dcc_mcp_core.dcc_server import register_diagnostic_handlers

        # In your DCC adapter server startup:
        register_diagnostic_handlers(my_server, dispatcher=my_dispatcher, dcc_name="blender")

    Registered actions
    ------------------
    - ``get_audit_log`` — sandbox audit log entries
    - ``get_action_metrics`` — ToolRecorder performance counters
    - ``dispatch_action`` — relay through the server's ToolDispatcher
    - ``take_screenshot`` — capture the DCC window (or full screen)

    """
    global _dispatcher_ref
    if dispatcher is not None:
        _dispatcher_ref = dispatcher

    _instance_context.update(
        {
            "dcc_name": dcc_name,
            "dcc_pid": dcc_pid,
            "dcc_window_handle": dcc_window_handle,
            "dcc_window_title": dcc_window_title,
            "resolver": resolver,
        }
    )

    # Ensure action recorder uses the correct DCC name
    _get_action_recorder(dcc_name)

    try:
        server.register_handler("get_audit_log", _handle_get_audit_log)
        server.register_handler("get_action_metrics", _handle_get_action_metrics)
        server.register_handler("dispatch_action", _handle_dispatch_action)
        server.register_handler("take_screenshot", _handle_take_screenshot)
        logger.debug(
            "Registered diagnostic IPC handlers for dcc=%r: "
            "get_audit_log, get_action_metrics, dispatch_action, take_screenshot",
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


# ── diagnostic MCP tool specs (exposed via tools/list & tools/call) ───────


_SCREENSHOT_SCHEMA: dict = {
    "type": "object",
    "properties": {
        "format": {"type": "string", "enum": ["png", "jpeg", "raw_bgra"], "default": "png"},
        "jpeg_quality": {"type": "integer", "minimum": 0, "maximum": 100, "default": 85},
        "scale": {"type": "number", "minimum": 0.0, "maximum": 1.0, "default": 1.0},
        "timeout_ms": {"type": "integer", "minimum": 100, "default": 5000},
        "full_screen": {"type": "boolean", "default": False},
        "window_handle": {"type": "integer"},
        "window_title": {"type": "string"},
        "process_id": {"type": "integer"},
    },
    "additionalProperties": False,
}

_AUDIT_SCHEMA: dict = {
    "type": "object",
    "properties": {
        "filter": {"type": "string", "enum": ["all", "allowed", "denied"], "default": "all"},
        "limit": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 100},
    },
    "additionalProperties": False,
}

_METRICS_SCHEMA: dict = {
    "type": "object",
    "properties": {
        "action_name": {"type": "string"},
    },
    "additionalProperties": False,
}

_PROC_SCHEMA: dict = {
    "type": "object",
    "properties": {},
    "additionalProperties": False,
}


def register_diagnostic_mcp_tools(
    server: Any,
    *,
    dcc_name: str = "dcc",
    dcc_pid: int | None = None,
    dcc_window_handle: int | None = None,
    dcc_window_title: str | None = None,
    resolver: Callable[[], int | None] | None = None,
) -> None:
    """Register the four ``diagnostics__*`` MCP tools on *server*.

    Tools are registered as regular :class:`ToolRegistry` entries so they
    appear in ``tools/list`` and can be invoked via ``tools/call``. Must be
    called **before** :meth:`McpHttpServer.start` — per the AGENTS.md rule
    "register all actions before start".

    Instance context kwargs match :func:`register_diagnostic_handlers` and
    are propagated to the same ``_instance_context`` global so the IPC path
    and the MCP-tool path share state.

    Registered tools
    ----------------
    - ``diagnostics__screenshot`` — capture the DCC window (or full screen)
    - ``diagnostics__audit_log`` — recent sandbox audit events
    - ``diagnostics__action_metrics`` — tool dispatch metrics
    - ``diagnostics__process_status`` — adapter process / DCC alive check

    Args:
        server: An :class:`McpHttpServer` instance (must expose a
            ``.registry`` getter and ``.register_handler()``).
        dcc_name: Logical DCC name (used as audit log / metrics namespace).
        dcc_pid: Optional PID of the DCC process — when set, the screenshot
            and process-status tools resolve the target window from this PID.
        dcc_window_handle: Optional HWND / X11 window ID to capture directly.
        dcc_window_title: Optional window title substring used as a fallback
            when neither ``dcc_pid`` nor ``dcc_window_handle`` resolves.
        resolver: Optional callable returning the current DCC PID on demand.
            Evaluated lazily per request so long-lived servers can track DCC
            process restarts.

    """
    _instance_context.update(
        {
            "dcc_name": dcc_name,
            "dcc_pid": dcc_pid,
            "dcc_window_handle": dcc_window_handle,
            "dcc_window_title": dcc_window_title,
            "resolver": resolver,
        }
    )
    _get_action_recorder(dcc_name)

    try:
        registry = server.registry
    except Exception as exc:
        logger.warning("register_diagnostic_mcp_tools: server.registry unavailable: %s", exc)
        return

    specs = [
        (
            "diagnostics__screenshot",
            "Capture the DCC window (or full screen).",
            _SCREENSHOT_SCHEMA,
            _handle_take_screenshot,
        ),
        ("diagnostics__audit_log", "Recent sandbox audit events.", _AUDIT_SCHEMA, _handle_get_audit_log),
        (
            "diagnostics__action_metrics",
            "Tool dispatch telemetry metrics.",
            _METRICS_SCHEMA,
            _handle_get_action_metrics,
        ),
        ("diagnostics__process_status", "Adapter process and DCC alive status.", _PROC_SCHEMA, _handle_process_status),
    ]

    for name, desc, schema, handler in specs:
        try:
            registry.register(
                name=name,
                description=desc,
                input_schema=json.dumps(schema),
                dcc=dcc_name,
                category="diagnostics",
                version="1.0.0",
            )
        except Exception as exc:
            logger.warning("register_diagnostic_mcp_tools: register(%s) failed: %s", name, exc)
            continue
        try:
            server.register_handler(name, lambda params, h=handler: _adapt_mcp_handler(h, params))
        except Exception as exc:
            logger.warning("register_diagnostic_mcp_tools: register_handler(%s) failed: %s", name, exc)


def _adapt_mcp_handler(handler: Callable[[str], str], params: Any) -> Any:
    """Adapt an IPC-style ``(json_str) -> json_str`` handler to MCP's dict IO."""
    params_str = json.dumps(params) if not isinstance(params, str) else params
    result_str = handler(params_str)
    try:
        return json.loads(result_str)
    except (TypeError, json.JSONDecodeError):
        return {"success": False, "message": "Invalid handler output"}
