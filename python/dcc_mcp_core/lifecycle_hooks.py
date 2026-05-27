"""Typed lifecycle-hook framework for DCC adapters (issue #1337).

The framework lets adapter and policy code subscribe to discovery, tool-call
and session events without patching ``DccServerBase`` internals. Hook fan-out
is bounded and fail-safe: an unexpected exception in one handler is logged
and never aborts a tool-call, but a handler may explicitly veto a policy
event by raising :class:`HookDeny`.

``BEFORE_SKILL_LOAD`` and ``AFTER_SKILL_LOAD`` are bridged automatically by
``DccServerBase.register_lifecycle_hooks``. ``DccServerBase.search_skills``
emits ``BEFORE_SEARCH`` / ``AFTER_SEARCH``, and adapters can bridge host-owned
session or tool execution boundaries with ``dispatch_session_start``,
``dispatch_before_tool_call``, ``dispatch_after_tool_call``, and
``dispatch_session_end`` without patching private server state.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from enum import Enum
import logging
from typing import Any
from typing import Callable

logger = logging.getLogger(__name__)


class HookEvent(str, Enum):
    """All typed lifecycle hook points emitted by ``DccServerBase``."""

    SESSION_START = "on_session_start"
    BEFORE_SEARCH = "before_search"
    AFTER_SEARCH = "after_search"
    BEFORE_SKILL_LOAD = "before_skill_load"
    AFTER_SKILL_LOAD = "after_skill_load"
    BEFORE_TOOL_CALL = "before_tool_call"
    AFTER_TOOL_CALL = "after_tool_call"
    SESSION_END = "on_session_end"

    @classmethod
    def policy_events(cls) -> frozenset[HookEvent]:
        """Events where a handler may veto via :class:`HookDeny`."""
        return frozenset({cls.BEFORE_SKILL_LOAD, cls.BEFORE_TOOL_CALL, cls.BEFORE_SEARCH})


@dataclass(frozen=True)
class HookContext:
    """Immutable payload passed to every hook handler.

    Attributes:
        event: Which :class:`HookEvent` is firing.
        dcc_name: Adapter identifier (``"maya"``, ``"blender"``, ``"any"``…).
        session_id: Optional stable identifier for the current agent session.
        payload: Event-specific structured data (skill name, tool name,
            search query, result count, …). Always JSON-safe so memory and
            telemetry layers can record it.

    """

    event: HookEvent
    dcc_name: str
    payload: dict[str, Any] = field(default_factory=dict)
    session_id: str | None = None


class HookDeny(Exception):
    """Raised by a policy hook to veto a discovery, load, or call event.

    Only meaningful for events listed in :meth:`HookEvent.policy_events`. The
    ``reason`` is surfaced to telemetry and to the caller as the deny reason;
    the ``hint`` is an agent-facing remediation string.
    """

    def __init__(self, reason: str, *, hint: str | None = None) -> None:
        super().__init__(reason)
        self.reason = reason
        self.hint = hint

    def __repr__(self) -> str:
        return f"HookDeny(reason={self.reason!r}, hint={self.hint!r})"


HookHandler = Callable[[HookContext], Any]


class LifecycleHooks:
    """Bounded, fail-safe registry of typed lifecycle handlers.

    Handlers are dispatched in registration order. For non-policy events,
    handler exceptions are logged at WARNING and swallowed so they cannot
    abort host execution. For policy events, a :class:`HookDeny` raised by
    any handler propagates to the caller; other exceptions are logged and
    treated as "no decision".
    """

    def __init__(self) -> None:
        self._handlers: dict[HookEvent, list[HookHandler]] = {evt: [] for evt in HookEvent}

    def on(self, event: HookEvent, handler: HookHandler) -> HookHandler:
        """Register ``handler`` for ``event`` and return it for use as a decorator."""
        if not callable(handler):
            raise TypeError("lifecycle hook handler must be callable")
        self._handlers[event].append(handler)
        return handler

    def off(self, event: HookEvent, handler: HookHandler) -> bool:
        """Remove a previously registered handler. Returns ``True`` if removed."""
        handlers = self._handlers[event]
        for idx in range(len(handlers) - 1, -1, -1):
            if handlers[idx] is handler:
                del handlers[idx]
                return True
        return False

    def handlers(self, event: HookEvent) -> tuple[HookHandler, ...]:
        """Snapshot of currently-registered handlers (immutable view)."""
        return tuple(self._handlers[event])

    def dispatch(self, context: HookContext) -> None:
        """Fan-out ``context`` to every handler registered for its event.

        Propagates :class:`HookDeny` from policy events; logs everything else.
        """
        is_policy = context.event in HookEvent.policy_events()
        for handler in self._handlers[context.event]:
            try:
                handler(context)
            except HookDeny:
                if is_policy:
                    raise
                logger.warning(
                    "[lifecycle] HookDeny raised by non-policy event %s; treating as logged-only",
                    context.event.value,
                )
            except Exception as exc:
                logger.warning(
                    "[lifecycle] handler for %s failed: %s",
                    context.event.value,
                    exc,
                    exc_info=True,
                )


__all__ = ["HookContext", "HookDeny", "HookEvent", "HookHandler", "LifecycleHooks"]
