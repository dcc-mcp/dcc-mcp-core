"""Agent feedback and rationale utilities for DCC-MCP servers.

This module provides two complementary features:

**Rationale capture** (issue #433):
    Agents can include a ``_meta.dcc.rationale`` field in ``tools/call``
    requests to explain why they are invoking a tool. Helper utilities here
    extract and store that signal server-side.

**Feedback tool** (issue #434):
    ``dcc_feedback__report`` — a built-in MCP tool that lets agents actively
    report when they are blocked, when a tool doesn't work as expected, or when
    they encounter a pattern that fails. Registered via
    :func:`register_feedback_tool`.

Rationale (proactive) + feedback (reactive) together give the server operator
a structured, agent-sourced signal of user intent and pain points — more
specific than human feedback, compatible with all MCP clients.

Example — rationale in a ``tools/call`` request::

    {
        "method": "tools/call",
        "params": {
            "name": "maya_geometry__create_sphere",
            "arguments": {"radius": 1.0},
            "_meta": {
                "dcc": {
                    "rationale": "User wants a reference sphere to compare scale."
                }
            }
        }
    }

Example — feedback tool call::

    # Agent reports it was blocked:
    {
        "method": "tools/call",
        "params": {
            "name": "dcc_feedback__report",
            "arguments": {
                "tool_name": "maya_geometry__create_sphere",
                "intent": "Create a 2 m radius sphere at the origin",
                "attempt": "Passed radius=2.0 but got a unit sphere",
                "blocker": "The radius parameter seems to be ignored",
                "severity": "blocked"
            }
        }
    }
"""

from __future__ import annotations

import logging
import os
import threading
import time
from typing import Any
from typing import Callable
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.request import Request
from urllib.request import urlopen
import uuid

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads
from dcc_mcp_core._tool_registration import ToolSpec
from dcc_mcp_core._tool_registration import register_tools
from dcc_mcp_core.constants import CATEGORY_FEEDBACK
from dcc_mcp_core.constants import ENV_GATEWAY_HOST
from dcc_mcp_core.constants import ENV_GATEWAY_PORT
from dcc_mcp_core.result_envelope import ToolResultEnvelope

logger = logging.getLogger(__name__)

_MAX_FEEDBACK_ENTRIES = 500
_DEFAULT_GATEWAY_HOST = "127.0.0.1"
_DEFAULT_GATEWAY_PORT = 9765
_GATEWAY_FEEDBACK_TIMEOUT_SECS = 5.0
_GATEWAY_EVENTS_URI = "resources://gateway/events"


class FeedbackStore:
    """Thread-safe bounded feedback state for one server instance."""

    def __init__(self, max_entries: int = _MAX_FEEDBACK_ENTRIES) -> None:
        self._lock = threading.Lock()
        self._entries: list[dict[str, Any]] = []
        self._max_entries = max(1, int(max_entries))

    def append(self, entry: dict[str, Any]) -> None:
        """Append one entry and evict the oldest overflow."""
        with self._lock:
            self._entries.append(dict(entry))
            if len(self._entries) > self._max_entries:
                del self._entries[: len(self._entries) - self._max_entries]

    def recent(
        self,
        *,
        tool_name: str | None = None,
        severity: str | None = None,
        limit: int = 50,
    ) -> list[dict[str, Any]]:
        """Return matching entries newest first."""
        with self._lock:
            entries = [dict(entry) for entry in reversed(self._entries)]
        if tool_name:
            entries = [entry for entry in entries if entry.get("tool_name") == tool_name]
        if severity:
            entries = [entry for entry in entries if entry.get("severity") == severity]
        return entries[: max(0, int(limit))]

    def clear(self) -> int:
        """Clear all entries and return the count removed."""
        with self._lock:
            count = len(self._entries)
            self._entries.clear()
        return count

    def reset_for_tests(self) -> None:
        """Clear mutable state between tests."""
        self.clear()


_DEFAULT_FEEDBACK_STORE = FeedbackStore()


def get_default_feedback_store() -> FeedbackStore:
    """Return the compatibility store used when no store is injected."""
    return _DEFAULT_FEEDBACK_STORE


def reset_default_feedback_store_for_tests() -> None:
    """Reset the compatibility store between tests."""
    _DEFAULT_FEEDBACK_STORE.reset_for_tests()


def _feedback_store(store: FeedbackStore | None) -> FeedbackStore:
    return store if store is not None else _DEFAULT_FEEDBACK_STORE


def _store_feedback(entry: dict[str, Any], *, store: FeedbackStore | None = None) -> None:
    """Append *entry* to an injected or compatibility feedback store."""
    _feedback_store(store).append(entry)


def get_feedback_entries(
    *,
    tool_name: str | None = None,
    severity: str | None = None,
    limit: int = 50,
    store: FeedbackStore | None = None,
) -> list[dict[str, Any]]:
    """Return recent feedback entries, newest first.

    Parameters
    ----------
    tool_name:
        If given, filter to entries for this tool.
    severity:
        If given, filter by severity (``"blocked"``, ``"workaround_found"``,
        ``"suggestion"``).
    limit:
        Maximum number of entries to return (default 50).
    store:
        Instance-owned store; defaults to the compatibility store.

    Returns
    -------
    list[dict]
        Each entry has keys: ``id``, ``timestamp``, ``tool_name``, ``intent``,
        ``attempt``, ``blocker``, ``severity``.

    """
    return _feedback_store(store).recent(tool_name=tool_name, severity=severity, limit=limit)


def clear_feedback(*, store: FeedbackStore | None = None) -> int:
    """Clear all in-memory feedback entries. Returns the count cleared."""
    return _feedback_store(store).clear()


# ── Rationale helpers ──────────────────────────────────────────────────────


def extract_rationale(params: dict[str, Any] | str) -> str | None:
    """Extract ``_meta.dcc.rationale`` from a ``tools/call`` params dict.

    Parameters
    ----------
    params:
        The ``params`` dict from a ``tools/call`` request, or a JSON string
        of the same.

    Returns
    -------
    str | None
        The rationale string, or ``None`` if not present.

    Example
    -------
    .. code-block:: python

        params = {
            "name": "create_sphere",
            "arguments": {"radius": 1.0},
            "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
        }
        rationale = extract_rationale(params)
        # "User wants a reference sphere."

    """
    if isinstance(params, str):
        try:
            params = json_loads(params)
        except (TypeError, ValueError):
            return None
    if not isinstance(params, dict):
        return None
    meta = params.get("_meta", {}) or {}
    dcc_meta = meta.get("dcc", {}) or {}
    return dcc_meta.get("rationale") or None


def make_rationale_meta(rationale: str) -> dict[str, Any]:
    """Build the ``_meta`` fragment for a ``tools/call`` request with a rationale.

    Parameters
    ----------
    rationale:
        A concise explanation of *why* the tool is being called — from the
        agent's perspective.  Examples: ``"User asked to create a reference
        sphere for scale comparison."``

    Returns
    -------
    dict
        ``{"_meta": {"dcc": {"rationale": "..."}}}``

    Example
    -------
    .. code-block:: python

        import httpx

        meta = make_rationale_meta("User wants a reference sphere for scale.")
        body = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "create_sphere",
                "arguments": {"radius": 1.0},
                **meta,
            },
        }
        response = httpx.post("http://127.0.0.1:8765/mcp", json=body)

    """
    return {"_meta": {"dcc": {"rationale": rationale}}}


# ── Feedback tool schema ───────────────────────────────────────────────────

_FEEDBACK_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "tool_name": {
            "type": "string",
            "description": "Name of the tool that blocked or failed.",
        },
        "intent": {
            "type": "string",
            "description": "What the agent was trying to accomplish.",
        },
        "attempt": {
            "type": "string",
            "description": "Parameters or approach the agent tried.",
        },
        "blocker": {
            "type": "string",
            "description": "Where it got stuck or what didn't work.",
        },
        "severity": {
            "type": "string",
            "enum": ["blocked", "workaround_found", "suggestion"],
            "description": "blocked | workaround_found | suggestion",
        },
        "request_id": {
            "type": "string",
            "description": "Request id of the failed call, when known.",
        },
        "job_id": {
            "type": "string",
            "description": "Job id of the failed asynchronous operation, when known.",
        },
    },
    "required": ["tool_name", "intent", "blocker", "severity"],
    "additionalProperties": False,
}

_FEEDBACK_TOOL_DESCRIPTION = (
    "Report when blocked, when a tool doesn't work as expected, or when a call "
    "pattern fails. "
    "When to use: call this after trying a tool and getting stuck so maintainers "
    "can improve the skill. "
    "How to use: set tool_name to the tool that failed, intent to your goal, "
    "attempt to what you tried, blocker to where you got stuck, severity to "
    "'blocked' / 'workaround_found' / 'suggestion'. The shared Core handler "
    "forwards the report to the gateway; request_id and job_id may correlate "
    "the failed operation."
)


def _build_gateway_feedback_endpoint(
    *,
    gateway_host: str | None = None,
    gateway_port: int | None = None,
) -> str | None:
    """Build the configured gateway feedback URL, or ``None`` when disabled."""
    host = gateway_host
    if host is None:
        host = os.environ.get(ENV_GATEWAY_HOST, _DEFAULT_GATEWAY_HOST)
    host = str(host).strip()

    port = gateway_port
    if port is None:
        raw_port = os.environ.get(ENV_GATEWAY_PORT, str(_DEFAULT_GATEWAY_PORT)).strip()
        try:
            port = int(raw_port)
        except ValueError:
            return None
    if not host or not 0 < int(port) <= 65535:
        return None
    if any(marker in host for marker in ("://", "/", "?", "#", "@")):
        return None
    if host == "0.0.0.0":
        host = "127.0.0.1"
    elif host in {"::", "[::]"}:
        host = "[::1]"
    if ":" in host and not host.startswith("["):
        host = f"[{host}]"
    return f"http://{host}:{int(port)}/v1/feedback"


def _feedback_error(message: str, error: str, **context: Any) -> str:
    return ToolResultEnvelope.fail(message, error=error, **context).to_json()


def _decode_gateway_response(raw: bytes) -> dict[str, Any] | None:
    try:
        payload = json_loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, TypeError, ValueError):
        return None
    return payload if isinstance(payload, dict) else None


def _gateway_error_message(payload: dict[str, Any] | None) -> str:
    if not payload:
        return "Gateway rejected the feedback report."
    error = payload.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        return error["message"]
    if isinstance(payload.get("message"), str):
        return payload["message"]
    return "Gateway rejected the feedback report."


def _handle_gateway_feedback_report(
    params: str | dict[str, Any],
    *,
    dcc_name: str,
    gateway_endpoint: str | None,
    instance_id_provider: Callable[[], str | None] | None = None,
    store: FeedbackStore | None = None,
) -> str:
    """Forward one compatibility-tool report to the gateway authority."""
    try:
        args = json_loads(params) if isinstance(params, str) else params
    except (TypeError, ValueError) as exc:
        return _feedback_error(f"Invalid params: {exc}", "invalid_input")
    if not isinstance(args, dict):
        return _feedback_error("Feedback params must be an object.", "invalid_input")
    if not gateway_endpoint:
        return _feedback_error(
            "Gateway feedback is disabled or not configured.",
            "gateway_feedback_unavailable",
        )

    report = {
        name: args[name]
        for name in ("tool_name", "intent", "attempt", "blocker", "severity", "request_id", "job_id")
        if name in args and args[name] is not None
    }
    report["dcc_type"] = dcc_name
    if instance_id_provider is not None:
        try:
            instance_id = instance_id_provider()
        except Exception as exc:
            logger.warning("Could not resolve feedback instance id: %s", exc)
            return _feedback_error(
                "The adapter instance id is unavailable.",
                "feedback_instance_unavailable",
            )
        if not instance_id:
            return _feedback_error(
                "The adapter instance id is unavailable.",
                "feedback_instance_unavailable",
            )
        report["instance_id"] = str(instance_id)

    transport_request_id = str(uuid.uuid4())
    try:
        request = Request(
            gateway_endpoint,
            data=json_dumps(report).encode("utf-8"),
            headers={
                "Accept": "application/json",
                "Content-Type": "application/json",
                "X-Request-ID": transport_request_id,
            },
            method="POST",
        )
        with urlopen(request, timeout=_GATEWAY_FEEDBACK_TIMEOUT_SECS) as response:
            status = int(getattr(response, "status", 0) or 0)
            echoed_request_id = response.headers.get("X-Request-ID")
            payload = _decode_gateway_response(response.read())
    except HTTPError as exc:
        echoed_request_id = exc.headers.get("X-Request-ID") if exc.headers is not None else None
        if echoed_request_id != transport_request_id:
            return _feedback_error(
                "Gateway feedback response did not match the request.",
                "transport_desync",
            )
        payload = _decode_gateway_response(exc.read())
        return _feedback_error(
            _gateway_error_message(payload),
            "gateway_feedback_rejected",
            status_code=int(exc.code),
        )
    except (URLError, OSError, TimeoutError, ValueError) as exc:
        logger.warning("Gateway feedback forwarding failed: %s", exc)
        return _feedback_error(
            "Gateway feedback endpoint is unavailable.",
            "gateway_feedback_unavailable",
        )

    if echoed_request_id != transport_request_id:
        return _feedback_error(
            "Gateway feedback response did not match the request.",
            "transport_desync",
        )
    if status != 201 or payload is None or payload.get("ok") is not True or payload.get("success") is not True:
        return _feedback_error(
            _gateway_error_message(payload),
            "gateway_feedback_invalid_receipt",
            status_code=status,
        )

    feedback_id = payload.get("feedback_id")
    event_resource_uri = payload.get("event_resource_uri")
    recorded_at = payload.get("recorded_at")
    try:
        parsed_feedback_id = uuid.UUID(feedback_id) if isinstance(feedback_id, str) else None
    except (ValueError, AttributeError):
        parsed_feedback_id = None
    if (
        parsed_feedback_id is None
        or event_resource_uri != _GATEWAY_EVENTS_URI
        or not isinstance(recorded_at, str)
        or not recorded_at
    ):
        return _feedback_error(
            "Gateway returned an invalid feedback receipt.",
            "gateway_feedback_invalid_receipt",
        )

    _store_feedback(
        {
            "id": feedback_id,
            "timestamp": time.time(),
            **report,
        },
        store=store,
    )
    logger.info(
        "dcc_feedback__report forwarded: id=%s tool=%s severity=%s",
        feedback_id,
        report.get("tool_name", ""),
        report.get("severity", ""),
    )
    return ToolResultEnvelope.ok(
        "Feedback recorded at the gateway.",
        feedback_id=feedback_id,
        recorded_at=recorded_at,
        event_resource_uri=event_resource_uri,
    ).to_json()


def _handle_feedback_report(params: str, *, store: FeedbackStore | None = None) -> str:
    """IPC-style handler for ``dcc_feedback__report``."""
    try:
        args: dict[str, Any] = json_loads(params) if isinstance(params, str) else params
    except (TypeError, ValueError) as exc:
        return json_dumps({"success": False, "message": f"Invalid params: {exc}"})

    entry: dict[str, Any] = {
        "id": str(uuid.uuid4()),
        "timestamp": time.time(),
        "tool_name": args.get("tool_name", ""),
        "intent": args.get("intent", ""),
        "attempt": args.get("attempt", ""),
        "blocker": args.get("blocker", ""),
        "severity": args.get("severity", "blocked"),
    }
    _store_feedback(entry, store=store)
    logger.info(
        "dcc_feedback__report: id=%s tool=%s severity=%s",
        entry["id"],
        entry["tool_name"],
        entry["severity"],
    )
    return ToolResultEnvelope.ok("Feedback recorded.", feedback_id=entry["id"]).to_json()


# ── Registration helper ────────────────────────────────────────────────────


def register_feedback_tool(
    server: Any,
    *,
    dcc_name: str = "dcc",
    store: FeedbackStore | None = None,
    gateway_endpoint: str | None = None,
    gateway_host: str | None = None,
    gateway_port: int | None = None,
    instance_id_provider: Callable[[], str | None] | None = None,
) -> None:
    """Register the ``dcc_feedback__report`` MCP tool on *server*.

    Call this **before** ``server.start()``, alongside
    :func:`~dcc_mcp_core.dcc_server.register_diagnostic_mcp_tools`.

    Parameters
    ----------
    server:
        An ``McpHttpServer`` or compatible object exposing ``server.registry``
        (:class:`~dcc_mcp_core.ToolRegistry`) and
        ``server.register_handler(name, handler)``.
    dcc_name:
        DCC name string used in the tool's ``dcc`` metadata field.
    store:
        Optional compatibility mirror updated only after gateway acceptance.
    gateway_endpoint:
        Explicit gateway ``/v1/feedback`` URL. When omitted, it is built from
        ``gateway_host`` / ``gateway_port`` or their environment defaults.
    gateway_host:
        Optional configured gateway host used when no endpoint is supplied.
    gateway_port:
        Optional configured gateway port; ``0`` disables the forwarder.
    instance_id_provider:
        Optional late-bound provider for the live adapter instance id.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import create_skill_server, McpHttpConfig
        from dcc_mcp_core.feedback import FeedbackStore, register_feedback_tool

        server = create_skill_server("maya", McpHttpConfig(port=8765))
        feedback_store = FeedbackStore()
        register_feedback_tool(
            server, dcc_name="maya", store=feedback_store
        )
        handle = server.start()

    """
    resolved_endpoint = gateway_endpoint or _build_gateway_feedback_endpoint(
        gateway_host=gateway_host,
        gateway_port=gateway_port,
    )

    def _mcp_handler(params: Any) -> Any:
        params_str = json_dumps(params) if not isinstance(params, str) else params
        result_str = _handle_gateway_feedback_report(
            params_str,
            dcc_name=dcc_name,
            gateway_endpoint=resolved_endpoint,
            instance_id_provider=instance_id_provider,
            store=store,
        )
        try:
            return json_loads(result_str)
        except (TypeError, ValueError):
            return {"success": False, "message": "Invalid handler output"}

    register_tools(
        server,
        [
            ToolSpec(
                name="dcc_feedback__report",
                description=_FEEDBACK_TOOL_DESCRIPTION,
                input_schema=_FEEDBACK_SCHEMA,
                handler=_mcp_handler,
                category=CATEGORY_FEEDBACK,
            ),
        ],
        dcc_name=dcc_name,
        log_prefix="register_feedback_tool",
        logger=logger,
    )


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "FeedbackStore",
    "clear_feedback",
    "extract_rationale",
    "get_default_feedback_store",
    "get_feedback_entries",
    "make_rationale_meta",
    "register_feedback_tool",
    "reset_default_feedback_store_for_tests",
]
