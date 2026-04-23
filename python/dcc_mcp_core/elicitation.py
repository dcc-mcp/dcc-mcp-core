"""MCP Elicitation support for dcc-mcp-core (issue #407).

Elicitation lets a server pause mid-tool-call to ask the user for input:

- **Form mode**: send a JSON Schema; the MCP client renders a native form
  (confirm destructive action, select render layer, fill missing parameter).
- **URL mode**: hand the user to a browser (OAuth flow, payment, credential
  collection).

This module provides:

1. Declarative types — :class:`ElicitationRequest`, :class:`ElicitationMode`,
   :class:`FormElicitation`, :class:`UrlElicitation`.
2. Pure-Python async helpers — :func:`elicit_form`, :func:`elicit_url` — for
   use inside ``async def`` skill handlers.
3. A synchronous approval-gate helper — :func:`elicit_form_sync` — for
   DCC main-thread handlers that cannot be ``async``.

MCP spec reference: 2025-11-25 §Elicitation
<https://modelcontextprotocol.io/specification/2025-11-25/client/elicitation>

Note:
----
The Rust-level ``McpHttpServer`` support for forwarding elicitation requests
over the SSE session and wiring up the ``notifications/elicitation/response``
callback is planned as a follow-up Rust PR.  Until that lands, these helpers
are **stub implementations** that:

- Log a warning indicating elicitation is not yet wired to the wire protocol.
- Return a :class:`ElicitationResponse` with ``accepted=False`` and
  ``message="elicitation_not_supported"`` so callers can implement graceful
  fallback.

When the Rust layer is ready, replace the stub body with the actual coroutine
that sends ``notifications/elicitation/request`` and awaits the response.

Usage
-----
::

    from dcc_mcp_core.elicitation import elicit_form, elicit_url, ElicitationResponse

    async def delete_objects_handler(objects: list[str], **kwargs):
        resp: ElicitationResponse = await elicit_form(
            message=f"Delete {len(objects)} objects? This cannot be undone.",
            schema={
                "type": "object",
                "properties": {
                    "confirm": {"type": "boolean", "title": "Confirm deletion"},
                },
                "required": ["confirm"],
            },
        )
        if not resp.accepted or not resp.data.get("confirm"):
            return {"success": False, "message": "Cancelled by user"}
        # … proceed with deletion …

"""

from __future__ import annotations

import dataclasses
import enum
import logging
from typing import Any

logger = logging.getLogger(__name__)

__all__ = [
    "ElicitationMode",
    "ElicitationRequest",
    "ElicitationResponse",
    "FormElicitation",
    "UrlElicitation",
    "elicit_form",
    "elicit_form_sync",
    "elicit_url",
]


class ElicitationMode(str, enum.Enum):
    """Whether the server requests a form or a browser URL flow."""

    FORM = "form"
    URL = "url"


@dataclasses.dataclass
class FormElicitation:
    """Parameters for form-mode elicitation.

    Args:
        message: Human-readable description shown above the form.
        schema: JSON Schema (as a Python dict) the client uses to render
            the form.  Use ``type: object`` with ``properties`` and
            ``required`` to declare the expected fields.
        title: Optional short title for the form dialog.

    """

    message: str
    schema: dict[str, Any]
    title: str | None = None


@dataclasses.dataclass
class UrlElicitation:
    """Parameters for URL-mode elicitation.

    Args:
        message: Human-readable description shown to the user.
        url: Browser URL to open (OAuth, payment, credential flow…).
        description: Optional longer explanation of the URL action.

    """

    message: str
    url: str
    description: str | None = None


@dataclasses.dataclass
class ElicitationRequest:
    """A mid-tool-call elicitation request sent to the MCP client.

    Args:
        mode: ``ElicitationMode.FORM`` or ``ElicitationMode.URL``.
        params: A :class:`FormElicitation` or :class:`UrlElicitation` instance.

    """

    mode: ElicitationMode
    params: FormElicitation | UrlElicitation


@dataclasses.dataclass
class ElicitationResponse:
    """Result returned when the user completes or dismisses an elicitation.

    Args:
        accepted: ``True`` if the user submitted the form / completed the
            URL flow; ``False`` if they cancelled or the client does not
            support elicitation.
        data: Dict of user-supplied values (form mode only).
            ``None`` for URL mode or when ``accepted=False``.
        message: Optional status / error message.

    """

    accepted: bool
    data: dict[str, Any] | None = None
    message: str | None = None


# ---------------------------------------------------------------------------
# Async helpers
# ---------------------------------------------------------------------------


async def elicit_form(
    message: str,
    schema: dict[str, Any],
    *,
    title: str | None = None,
) -> ElicitationResponse:
    """Pause a tool call and ask the user for form input (async).

    Sends a form-mode elicitation request to the MCP client and awaits the
    user's response.

    Args:
        message: Human-readable prompt shown above the form.
        schema: JSON Schema dict describing the expected input.
        title: Optional dialog title.

    Returns:
        :class:`ElicitationResponse` with ``accepted=True`` and ``data``
        populated when the user submits, or ``accepted=False`` when the
        user cancels or the client does not support elicitation.

    Note:
        The Rust MCP HTTP layer support for elicitation is planned (issue
        #407).  Until then this is a stub that returns
        ``accepted=False, message="elicitation_not_supported"``.

    Example::

        resp = await elicit_form(
            message="Select render settings",
            schema={
                "type": "object",
                "properties": {
                    "quality": {"type": "string", "enum": ["low", "medium", "high"]},
                    "samples": {"type": "integer", "minimum": 1, "maximum": 4096},
                },
                "required": ["quality"],
            },
        )
        if resp.accepted:
            quality = resp.data["quality"]

    """
    logger.warning(
        "elicit_form: MCP Elicitation is not yet wired to the HTTP transport "
        "(issue #407). Returning accepted=False as graceful fallback. "
        "message=%r",
        message,
    )
    return ElicitationResponse(accepted=False, message="elicitation_not_supported")


async def elicit_url(
    message: str,
    url: str,
    *,
    description: str | None = None,
) -> ElicitationResponse:
    """Pause a tool call and hand the user to a browser URL (async).

    Opens a browser-based flow (OAuth, payment, credential collection) and
    waits for the client to signal completion.

    Args:
        message: Human-readable description of the URL action.
        url: Browser URL to open.
        description: Optional longer explanation.

    Returns:
        :class:`ElicitationResponse` with ``accepted=True`` when the URL
        flow completes, or ``accepted=False`` when the client does not
        support URL-mode elicitation.

    Note:
        The Rust MCP HTTP layer support for elicitation is planned (issue
        #407).  Until then this is a stub.

    Example::

        resp = await elicit_url(
            message="Authorize access to Shotgrid",
            url="https://shotgrid.example.com/oauth/authorize?client_id=...",
        )

    """
    logger.warning(
        "elicit_url: MCP Elicitation (URL mode) is not yet wired to the HTTP "
        "transport (issue #407). Returning accepted=False. url=%r",
        url,
    )
    return ElicitationResponse(accepted=False, message="elicitation_not_supported")


# ---------------------------------------------------------------------------
# Synchronous fallback (for DCC main-thread handlers)
# ---------------------------------------------------------------------------


def elicit_form_sync(
    message: str,
    schema: dict[str, Any],
    *,
    title: str | None = None,
    fallback_values: dict[str, Any] | None = None,
) -> ElicitationResponse:
    """Block a DCC main-thread handler until elicitation completes (sync wrapper).

    Since DCC main-thread handlers cannot be ``async``, this helper provides
    a blocking alternative.  When the Rust transport supports elicitation, this
    will block the calling thread until the user responds.

    Args:
        message: Human-readable prompt.
        schema: JSON Schema dict.
        title: Optional dialog title.
        fallback_values: Values to return as ``data`` when elicitation is
            not supported.  If ``None``, returns ``accepted=False``.

    Returns:
        :class:`ElicitationResponse`.

    """
    logger.warning(
        "elicit_form_sync: MCP Elicitation is not yet wired to the HTTP transport (issue #407). %s",
        f"Using fallback_values={fallback_values!r}" if fallback_values else "Returning accepted=False.",
    )
    if fallback_values is not None:
        return ElicitationResponse(accepted=True, data=fallback_values, message="fallback_values_used")
    return ElicitationResponse(accepted=False, message="elicitation_not_supported")
