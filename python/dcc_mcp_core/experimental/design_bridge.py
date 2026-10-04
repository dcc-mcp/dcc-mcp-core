"""Bridge already-authorized design sessions without owning transport or login.

Official MCP sessions are duck typed: ``await list_tools(cursor=...)`` and
``await call_tool(name, arguments=...)``. Framer instead requires an explicit
API facade; this module does not describe that API as an official MCP server.
The caller owns initialization, authentication, transport cleanup and native
readback. Catalog discovery and a received response never certify a host task.
"""

from __future__ import annotations

# Import standard library modules.
import asyncio
from copy import deepcopy
from dataclasses import dataclass
import sys
from typing import Any
from typing import Callable
from typing import Mapping

# Import local modules.
from dcc_mcp_core.errors import DccMcpError
from dcc_mcp_core.result_envelope import ToolResultEnvelope

__all__ = [
    "DesignBridgeCallResult",
    "DesignBridgeError",
    "DesignProviderProfile",
    "DesignSessionBridge",
    "FramerApiFacade",
    "get_design_provider_profile",
]

_ERROR_MESSAGES = {
    "auth_required": "The upstream session requires authentication by its owner.",
    "permission_denied": "The upstream session denied this operation.",
    "unsupported_host": "The selected provider does not support this upstream host platform.",
    "unavailable": "The upstream session or advertised tool is unavailable.",
    "upstream_error": "The upstream session returned an error or an invalid response.",
}


class DesignBridgeError(DccMcpError):
    """Expose only stable error codes and fixed, credential-free messages.

    Args:
        code: A supported bridge error code, never an upstream message.
        dispatched: Whether the operation was submitted to the upstream.
        reason: One of the fixed bridge diagnostic reasons.

    """

    def __init__(self, code: str, *, dispatched: bool = False, reason: str = "") -> None:
        if code not in _ERROR_MESSAGES:
            raise ValueError("Unknown design bridge error code")
        if reason not in {
            "",
            "stale_session",
            "tool_not_advertised",
            "invalid_response",
            "invalid_session",
            "invalid_arguments",
        }:
            raise ValueError("Unknown design bridge error reason")
        self.code = code
        self.dispatched = dispatched
        self.reason = reason
        super().__init__(_ERROR_MESSAGES[code])

    def to_dict(self) -> dict[str, Any]:
        """Return a Core error envelope without the upstream exception payload."""
        return ToolResultEnvelope.fail(
            str(self),
            error=self.code,
            dispatched=self.dispatched,
            reason=self.reason,
            outcome="indeterminate" if self.dispatched else "not_dispatched",
            native_acceptance="unverified",
        ).to_dict()


@dataclass(frozen=True)
class DesignProviderProfile:
    """Identify an integration boundary without promising specific tools.

    ``supported_host_platforms`` constrains the upstream application host,
    rather than the machine where this bridge happens to run. An empty tuple
    means the bridge imposes no platform constraint, not universal support.
    """

    provider: str
    label: str
    protocol: str
    official_docs: str
    supported_host_platforms: tuple[str, ...] = ()

    def to_dict(self) -> dict[str, Any]:
        """Return provider metadata independently of observed availability."""
        return {
            "provider": self.provider,
            "label": self.label,
            "protocol": self.protocol,
            "official_docs": self.official_docs,
            "supported_host_platforms": list(self.supported_host_platforms),
        }


_PROFILES = {
    "figma": DesignProviderProfile("figma", "Figma", "mcp", "https://developers.figma.com/docs/figma-mcp-server/"),
    "penpot": DesignProviderProfile("penpot", "Penpot", "mcp", "https://penpot.app/ai/mcp-server"),
    "pencil": DesignProviderProfile("pencil", "Pencil", "mcp", "https://docs.pencil.dev/core-concepts/ai-agents"),
    "sketch": DesignProviderProfile("sketch", "Sketch", "mcp", "https://www.sketch.com/docs/mcp-server/", ("darwin",)),
    "framer": DesignProviderProfile(
        "framer", "Framer", "api", "https://www.framer.com/developers/server-api-introduction"
    ),
}


def get_design_provider_profile(provider: str) -> DesignProviderProfile:
    """Return one of the five supported integration profiles.

    Args:
        provider: Canonical provider identifier, case insensitive.

    Raises:
        ValueError: If the provider is unknown.

    """
    if not isinstance(provider, str) or provider.strip().lower() not in _PROFILES:
        raise ValueError("Unknown design provider")
    return _PROFILES[provider.strip().lower()]


class FramerApiFacade:
    """Adapt caller-owned Framer API operations to the bridge's discovery seam.

    The caller explicitly supplies async callbacks. ``list_tools`` returns
    descriptors with names and input schemas; ``call_tool`` returns an
    MCP-shaped result mapping for the bridge envelope. This normalization is
    an adapter contract, not a claim that Framer serves MCP. Neither callback
    may create authentication authority on behalf of this bridge.

    Args:
        list_tools: Async callback accepting the keyword ``cursor``.
        call_tool: Async callback accepting a name and keyword ``arguments``.

    """

    def __init__(self, *, list_tools: Callable[..., Any], call_tool: Callable[..., Any]) -> None:
        if not callable(list_tools) or not callable(call_tool):
            raise ValueError("Framer facade requires callable API adapters")
        self._list_tools = list_tools
        self._call_tool = call_tool

    async def list_tools(self, cursor: str | None = None) -> Any:
        """Return the caller's explicit API operation catalog page."""
        return await self._list_tools(cursor=cursor)

    async def call_tool(self, name: str, arguments: Mapping[str, Any] | None = None) -> Any:
        """Invoke the caller's API adapter without changing its operation name."""
        return await self._call_tool(name, arguments=arguments)


def _as_mapping(value: Any) -> dict[str, Any]:
    """Copy wire mappings or SDK models while retaining their alias fields."""
    if isinstance(value, Mapping):
        return deepcopy(dict(value))
    if callable(getattr(value, "model_dump", None)):
        value = value.model_dump(mode="json", by_alias=True, exclude_none=True)
    elif callable(getattr(value, "dict", None)):
        value = value.dict(by_alias=True, exclude_none=True)
    if not isinstance(value, Mapping):
        raise DesignBridgeError("upstream_error", reason="invalid_response")
    return deepcopy(dict(value))


def _public_error(exc: Exception, *, dispatched: bool = False) -> DesignBridgeError:
    """Classify typed failures without reading exception messages or bodies."""
    if isinstance(exc, DesignBridgeError):
        return DesignBridgeError(exc.code, dispatched=dispatched or exc.dispatched, reason=exc.reason)
    status_code = getattr(exc, "status_code", None)
    if status_code is None:
        status_code = getattr(getattr(exc, "response", None), "status_code", None)
    if status_code == 401:
        code = "auth_required"
    elif status_code == 403 or isinstance(exc, PermissionError):
        code = "permission_denied"
    elif isinstance(exc, (ConnectionError, TimeoutError, OSError)):
        code = "unavailable"
    else:
        code = "upstream_error"
    return DesignBridgeError(code, dispatched=dispatched)


@dataclass(frozen=True)
class DesignBridgeCallResult:
    """Keep the vendor response separate from transport and task acceptance.

    ``tool_error`` is ``True`` only for an explicit upstream ``isError``.
    Plain text results remain unknown even if ``isError`` is false: prose can
    describe a business failure without setting the MCP error flag. No result
    here proves a write, save, reopen, export, or other native postcondition.
    """

    provider: str
    generation: int
    tool_name: str
    upstream_result: Mapping[str, Any]
    tool_error: bool | None
    transport_success: bool = True

    def to_dict(self) -> dict[str, Any]:
        """Return a Core envelope with the complete vendor response in context.

        Core's outer MCP handlers may wrap this envelope; this method does
        not promise unchanged image/resource blocks at the outer wire level.
        ``success`` acknowledges receipt unless the tool explicitly failed.
        Native acceptance always needs separate host-owned readback evidence.
        """
        context = {
            "provider": self.provider,
            "generation": self.generation,
            "tool_name": self.tool_name,
            "transport_success": self.transport_success,
            "tool_error": self.tool_error,
            "upstream_result": deepcopy(dict(self.upstream_result)),
            "outcome": "upstream_tool_error" if self.tool_error is True else "unverified",
            "native_acceptance": "unverified",
        }
        if self.tool_error is True:
            return ToolResultEnvelope.fail(
                "The upstream tool reported an error.", error="upstream_error", **context
            ).to_dict()
        return ToolResultEnvelope.ok(
            "Upstream response received; native effect requires readback.", verified=False, **context
        ).to_dict()


class DesignSessionBridge:
    """Discover and invoke exact upstream tools on one externally owned session.

    Instances are intended for one async event loop. No retries, transport
    creation, host launch, OAuth, persistent state or session cleanup are owned
    here. Reconnection and catalog refresh advance ``generation``; callers
    should pass the generation from discovery when describing or invoking a
    tool. Results from an older generation fail closed even if their operation
    may already have changed the upstream host.

    Args:
        provider: A provider identifier or explicit provider profile.
        max_pages: Positive discovery page bound; pagination never truncates
            into a successful partial catalog.

    """

    def __init__(self, provider: str | DesignProviderProfile, *, max_pages: int = 100) -> None:
        self.profile = get_design_provider_profile(provider) if isinstance(provider, str) else provider
        if not isinstance(self.profile, DesignProviderProfile) or self.profile.protocol not in {"mcp", "api"}:
            raise ValueError("Invalid design provider profile")
        if not isinstance(max_pages, int) or isinstance(max_pages, bool) or max_pages < 1:
            raise ValueError("max_pages must be a positive integer")
        self._max_pages = max_pages
        self._generation = 0
        self._session: Any = None
        self._state = "disconnected"
        self._last_error: str | None = None
        self._tools: dict[str, dict[str, Any]] = {}
        self._pages: list[dict[str, Any]] = []

    def _invalidate(self, state: str) -> int:
        self._generation += 1
        self._state = state
        self._last_error = None
        self._tools = {}
        self._pages = []
        return self._generation

    def _abandon(self, generation: int) -> None:
        if generation == self._generation:
            self._invalidate("unavailable")
            self._last_error = "unavailable"
            self._session = None

    def _check_generation(self, expected_generation: int | None, *, dispatched: bool = False) -> None:
        if expected_generation is not None and (
            not isinstance(expected_generation, int) or isinstance(expected_generation, bool)
        ):
            raise ValueError("expected_generation must be an integer or None")
        if expected_generation is not None and expected_generation != self._generation:
            raise DesignBridgeError("unavailable", reason="stale_session", dispatched=dispatched)

    def _ready_session(self, expected_generation: int | None = None) -> Any:
        self._check_generation(expected_generation)
        if self._state != "ready" or self._session is None:
            raise DesignBridgeError(self._state if self._state in _ERROR_MESSAGES else "unavailable")
        return self._session

    def status(self) -> dict[str, Any]:
        """Return observed bridge state without endpoint, credential or account data."""
        return {
            **self.profile.to_dict(),
            "state": self._state,
            "connected": self._state == "ready",
            "generation": self._generation,
            "tool_count": len(self._tools),
            "last_error": self._last_error,
            "authentication": "unknown",
            "document_binding": "unverified",
            "host_availability": "unverified",
            "live_capabilities_verified": self._state == "ready",
            "native_acceptance": "unverified",
        }

    def capabilities(self) -> dict[str, Any]:
        """Return only advertised descriptors, with complete schemas and metadata."""
        return {
            **self.status(),
            "tools": deepcopy(list(self._tools.values())),
            "catalogue_pages": deepcopy(self._pages),
        }

    def disconnect(self) -> dict[str, Any]:
        """Detach without closing or otherwise changing the caller-owned session."""
        self._invalidate("disconnected")
        self._session = None
        return self.status()

    async def connect(self, session: Any, *, host_platform: str | None = None) -> dict[str, Any]:
        """Attach an authorized session and atomically discover its entire catalog.

        Args:
            session: Initialized MCP SDK session, or ``FramerApiFacade`` for
                the API provider. Authentication is a caller responsibility.
            host_platform: Platform of the upstream host using ``sys.platform``
                identifiers; omitted values use this machine's platform.

        Raises:
            DesignBridgeError: For unsupported hosts, invalid sessions,
                discovery failures or a concurrently replaced attachment.

        """
        generation = self._invalidate("connecting")
        self._session = None
        platform = sys.platform if host_platform is None else host_platform
        if self.profile.supported_host_platforms and platform not in self.profile.supported_host_platforms:
            self._state = self._last_error = "unsupported_host"
            raise DesignBridgeError("unsupported_host")
        if (
            not callable(getattr(session, "list_tools", None))
            or not callable(getattr(session, "call_tool", None))
            or (self.profile.protocol == "api" and not isinstance(session, FramerApiFacade))
        ):
            self._state = self._last_error = "unavailable"
            raise DesignBridgeError("unavailable", reason="invalid_session")
        self._session = session
        return await self._discover(session, generation)

    async def refresh_tools(self, *, expected_generation: int | None = None) -> dict[str, Any]:
        """Replace the catalog atomically and invalidate previously discovered names.

        Call this after an upstream tools/list_changed notification. A failed
        refresh exposes no old or partial catalog; manual reconnection is
        required after a denial or loss of availability.
        """
        session = self._ready_session(expected_generation)
        generation = self._invalidate("connecting")
        return await self._discover(session, generation)

    async def _discover(self, session: Any, generation: int) -> dict[str, Any]:
        tools: dict[str, dict[str, Any]] = {}
        pages: list[dict[str, Any]] = []
        cursor: str | None = None
        seen_cursors: set[str] = set()
        try:
            for _ in range(self._max_pages):
                page = _as_mapping(await session.list_tools(cursor=cursor))
                self._check_generation(generation)
                if not isinstance(page.get("tools"), list):
                    raise DesignBridgeError("upstream_error", reason="invalid_response")
                normalized_tools = []
                for entry in page["tools"]:
                    tool = _as_mapping(entry)
                    name = tool.get("name")
                    if not isinstance(name, str) or not name or name in tools:
                        raise DesignBridgeError("upstream_error", reason="invalid_response")
                    if not isinstance(tool.get("inputSchema"), Mapping):
                        raise DesignBridgeError("upstream_error", reason="invalid_response")
                    tools[name] = tool
                    normalized_tools.append(tool)
                page["tools"] = normalized_tools
                pages.append(page)
                cursor = page.get("nextCursor")
                if cursor is None:
                    break
                if not isinstance(cursor, str) or cursor in seen_cursors:
                    raise DesignBridgeError("upstream_error", reason="invalid_response")
                seen_cursors.add(cursor)
            else:
                raise DesignBridgeError("upstream_error", reason="invalid_response")
            self._check_generation(generation)
        except asyncio.CancelledError:
            self._abandon(generation)
            raise
        except Exception as exc:
            error = _public_error(exc)
            if generation == self._generation:
                self._state = self._last_error = error.code
                self._session = None
            raise error from None
        except BaseException:
            self._abandon(generation)
            raise
        self._tools = tools
        self._pages = pages
        self._state = "ready"
        self._last_error = None
        return self.capabilities()

    def describe_tool(self, name: str, *, expected_generation: int | None = None) -> dict[str, Any]:
        """Return an unchanged advertised descriptor, refusing guessed names."""
        self._ready_session(expected_generation)
        if not isinstance(name, str):
            raise ValueError("Tool name must be a string")
        if name not in self._tools:
            raise DesignBridgeError("unavailable", reason="tool_not_advertised")
        return deepcopy(self._tools[name])

    async def call_tool(
        self,
        name: str,
        arguments: Mapping[str, Any] | None = None,
        *,
        expected_generation: int | None = None,
    ) -> DesignBridgeCallResult:
        """Invoke one advertised name once, preserving names, arguments and results.

        Args:
            name: Exact name returned by the current upstream catalog.
            arguments: Upstream arguments; no invented provider parameters.
            expected_generation: Discovery generation to prevent stale calls.

        Raises:
            DesignBridgeError: If unavailable, denied, stale, or the upstream
                response is invalid. Dispatched failures are indeterminate;
                the caller must read back rather than retry a mutation.

        """
        session = self._ready_session(expected_generation)
        self.describe_tool(name, expected_generation=expected_generation)
        if arguments is not None and not isinstance(arguments, Mapping):
            raise ValueError("Tool arguments must be a mapping or None")
        generation = self._generation
        try:
            copied_arguments = deepcopy(dict(arguments)) if arguments is not None else None
        except Exception:
            raise DesignBridgeError("upstream_error", reason="invalid_arguments") from None
        self._check_generation(generation)
        try:
            response = await session.call_tool(name, arguments=copied_arguments)
            self._check_generation(generation, dispatched=True)
            result = _as_mapping(response)
            if not isinstance(result.get("content"), list):
                raise DesignBridgeError("upstream_error", reason="invalid_response")
            normalized_content = []
            for entry in result["content"]:
                block = _as_mapping(entry)
                if not isinstance(block.get("type"), str) or not block["type"]:
                    raise DesignBridgeError("upstream_error", reason="invalid_response")
                normalized_content.append(block)
            result["content"] = normalized_content
            flag = result.get("isError")
            if flag is not None and not isinstance(flag, bool):
                raise DesignBridgeError("upstream_error", reason="invalid_response")
            text_only = (
                bool(result["content"])
                and all(isinstance(block, Mapping) and block.get("type") == "text" for block in result["content"])
                and result.get("structuredContent") is None
            )
            tool_error = True if flag is True else (None if text_only or flag is None else False)
            self._check_generation(generation, dispatched=True)
        except asyncio.CancelledError:
            self._abandon(generation)
            raise
        except Exception as exc:
            error = _public_error(exc, dispatched=True)
            if generation == self._generation:
                self._last_error = error.code
                if error.code in {"auth_required", "permission_denied", "unavailable"}:
                    self._invalidate(error.code)
                    self._last_error = error.code
                    self._session = None
            raise error from None
        except BaseException:
            self._abandon(generation)
            raise
        return DesignBridgeCallResult(self.profile.provider, generation, name, result, tool_error)
