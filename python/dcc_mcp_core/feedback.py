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
import threading
import time
from typing import Any
import uuid

from dcc_mcp_core import json_dumps
from dcc_mcp_core import json_loads

logger = logging.getLogger(__name__)

# ── In-memory feedback store ───────────────────────────────────────────────

_FEEDBACK_LOCK = threading.Lock()
_FEEDBACK_STORE: list[dict[str, Any]] = []
_MAX_FEEDBACK_ENTRIES = 500


def _store_feedback(entry: dict[str, Any]) -> None:
    """Append *entry* to the in-memory feedback store (thread-safe, capped)."""
    with _FEEDBACK_LOCK:
        _FEEDBACK_STORE.append(entry)
        if len(_FEEDBACK_STORE) > _MAX_FEEDBACK_ENTRIES:
            del _FEEDBACK_STORE[: len(_FEEDBACK_STORE) - _MAX_FEEDBACK_ENTRIES]


def get_feedback_entries(
    *,
    tool_name: str | None = None,
    severity: str | None = None,
    limit: int = 50,
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

    Returns
    -------
    list[dict]
        Each entry has keys: ``id``, ``timestamp``, ``tool_name``, ``intent``,
        ``attempt``, ``blocker``, ``severity``.

    """
    with _FEEDBACK_LOCK:
        entries = list(reversed(_FEEDBACK_STORE))
    if tool_name:
        entries = [e for e in entries if e.get("tool_name") == tool_name]
    if severity:
        entries = [e for e in entries if e.get("severity") == severity]
    return entries[:limit]


def clear_feedback() -> int:
    """Clear all in-memory feedback entries. Returns the count cleared."""
    with _FEEDBACK_LOCK:
        count = len(_FEEDBACK_STORE)
        _FEEDBACK_STORE.clear()
    return count


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
    "'blocked' / 'workaround_found' / 'suggestion'."
)


def _handle_feedback_report(params: str) -> str:
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
    _store_feedback(entry)
    logger.info(
        "dcc_feedback__report: id=%s tool=%s severity=%s",
        entry["id"],
        entry["tool_name"],
        entry["severity"],
    )
    return json_dumps(
        {
            "success": True,
            "message": "Feedback recorded.",
            "context": {"feedback_id": entry["id"]},
        }
    )


# ── Registration helper ────────────────────────────────────────────────────


def register_feedback_tool(
    server: Any,
    *,
    dcc_name: str = "dcc",
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

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core import create_skill_server, McpHttpConfig
        from dcc_mcp_core.feedback import register_feedback_tool

        server = create_skill_server("maya", McpHttpConfig(port=8765))
        register_feedback_tool(server, dcc_name="maya")
        handle = server.start()

    """
    try:
        registry = server.registry
    except Exception as exc:
        logger.warning("register_feedback_tool: server.registry unavailable: %s", exc)
        return

    try:
        registry.register(
            name="dcc_feedback__report",
            description=_FEEDBACK_TOOL_DESCRIPTION,
            input_schema=json_dumps(_FEEDBACK_SCHEMA),
            dcc=dcc_name,
            category="feedback",
            version="1.0.0",
        )
    except Exception as exc:
        logger.warning("register_feedback_tool: register failed: %s", exc)
        return

    def _mcp_handler(params: Any) -> Any:
        params_str = json_dumps(params) if not isinstance(params, str) else params
        result_str = _handle_feedback_report(params_str)
        try:
            return json_loads(result_str)
        except (TypeError, ValueError):
            return {"success": False, "message": "Invalid handler output"}

    try:
        server.register_handler("dcc_feedback__report", _mcp_handler)
    except Exception as exc:
        logger.warning("register_feedback_tool: register_handler failed: %s", exc)


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "clear_feedback",
    "extract_rationale",
    "get_feedback_entries",
    "make_rationale_meta",
    "register_feedback_tool",
]
