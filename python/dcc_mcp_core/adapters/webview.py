"""WebView-host adapter base (#211).

``WebViewAdapter`` is a slim, standalone Pure-Python template for hosts whose
capability profile is narrower than a full DCC (Maya, Blender, …) — most
commonly AuroraView-style browser / tool panels that do not have a scene
graph, timeline, or selection.

Unlike :class:`dcc_mcp_core.DccServerBase`, this class is **not** intended to
be wired up to the Gateway directly; it is a stub that integration projects
(AuroraView, an Electron bridge, an ImGui tool panel, …) subclass to advertise
*what the host can actually do*.  The pre-declared
:attr:`WebViewAdapter.capabilities` map (all false by default) signals to the
registry that scene / timeline / selection tools should be hidden for sessions
routed to this adapter — see ``ToolRegistry.register(required_capabilities=…)``
for the advertising side.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

# Import built-in modules
from typing import Any
from typing import ClassVar

if TYPE_CHECKING:
    # Import built-in modules
    from collections.abc import Iterable
    from collections.abc import Mapping


#: The closed set of capability keys recognised by dcc-mcp-core.
#:
#: Registry consumers that implement capability-based filtering should treat
#: any key **not** in this set as host-specific and leave filtering to the
#: adapter.  The set is intentionally small — extensions live in
#: :attr:`dcc_mcp_core.DccCapabilities.extensions` rather than here.
CAPABILITY_KEYS: frozenset[str] = frozenset(
    {
        "scene",
        "timeline",
        "selection",
        "undo",
        "render",
    }
)


#: Default capability map for :class:`WebViewAdapter` subclasses.
#:
#: All five built-in capabilities are set to ``False`` — WebView hosts do not
#: own a scene, timeline, or selection model.  Subclasses may flip an entry
#: when their web app genuinely exposes the capability (e.g. an in-browser
#: timeline editor).
WEBVIEW_DEFAULT_CAPABILITIES: dict[str, bool] = {k: False for k in CAPABILITY_KEYS}


class WebViewContext(dict):  # type: ignore[type-arg]
    """Typed-dict convenience for the payload returned by ``get_context()``.

    Stored as a ``dict`` subclass to stay JSON-serialisable without extra
    conversion.  Recognised keys (all optional):

    - ``window_title`` : ``str``
    - ``url``          : ``str``
    - ``pid``          : ``int``
    - ``cdp_port``     : ``int``  (Chrome DevTools Protocol debug port)
    - ``host_dcc``     : ``str``  (the DCC embedding this WebView, if any)
    """


class WebViewAdapter:
    """Standalone WebView-host adapter template.

    This class is intentionally **not** a subclass of
    :class:`dcc_mcp_core.DccServerBase`; it is a pure-Python stub whose
    subclasses are registered with the Gateway as a DCC type (``"webview"``,
    ``"auroraview"``, …) but advertise a narrower capability surface.

    Subclasses must override:

    - :meth:`get_context`   — return descriptor dict (window / url / pid / …)
    - :meth:`list_tools`    — list tool metadata dicts exposed by the host
    - :meth:`execute`       — dispatch a tool invocation to the host's bridge
    - :meth:`get_audit_log` — return recent audit entries (may return ``[]``)

    The default :attr:`capabilities` map (all ``False``) signals to the
    registry that scene / timeline / selection / undo / render tools should
    be hidden for sessions routed to this adapter.
    """

    #: Capability descriptor advertised to the Gateway / registry.
    #:
    #: Override in subclasses to flip individual entries when the host
    #: genuinely supports that capability (e.g. a Chromium-based editor with
    #: an in-browser undo stack → ``{"undo": True, ...}``).
    capabilities: ClassVar[dict[str, bool]] = dict(WEBVIEW_DEFAULT_CAPABILITIES)

    #: DCC short-name this adapter registers as.  Override in subclasses.
    dcc_name: str = "webview"

    def get_context(self) -> WebViewContext:
        """Return descriptor dict for the current WebView host session.

        Recommended keys (any may be absent): ``window_title``, ``url``,
        ``pid``, ``cdp_port``, ``host_dcc``.
        """
        raise NotImplementedError

    def list_tools(self) -> list[dict[str, Any]]:
        """Return tool declaration dicts exposed by the host's JS bridge."""
        raise NotImplementedError

    def execute(self, tool: str, params: Mapping[str, Any] | None = None) -> dict[str, Any]:
        """Invoke ``tool`` on the host with ``params`` and return the result."""
        raise NotImplementedError

    def get_audit_log(self, limit: int = 100) -> list[dict[str, Any]]:
        """Return up to ``limit`` recent audit-log entries (default ``[]``).

        Concrete subclasses may forward to ``SandboxPolicy.audit_log`` or
        return an empty list when auditing is not wired up.
        """
        return []

    @classmethod
    def advertised_capabilities(cls) -> dict[str, bool]:
        """Return a **fresh copy** of :attr:`capabilities` (safe to mutate)."""
        return dict(cls.capabilities)

    @classmethod
    def supports(cls, capability: str) -> bool:
        """Return ``True`` when the adapter advertises ``capability`` as enabled."""
        return bool(cls.capabilities.get(capability, False))

    @classmethod
    def matches_requirements(cls, required: Iterable[str]) -> bool:
        """Return ``True`` when every required capability is advertised.

        Intended for a Gateway-side filter that hides tools registered with
        ``ToolRegistry.register(required_capabilities=[…])`` from WebView
        sessions that don't advertise the needed key.
        """
        return all(cls.supports(key) for key in required)
