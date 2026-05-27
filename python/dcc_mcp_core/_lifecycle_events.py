"""Lifecycle hook dispatch helper for :class:`DccServerBase` (#1337)."""

from __future__ import annotations

from typing import Any
from typing import Callable

from dcc_mcp_core.lifecycle_hooks import HookContext
from dcc_mcp_core.lifecycle_hooks import HookEvent


class LifecycleEventDispatcher:
    """Small adapter around the optional ``LifecycleHooks`` registry.

    ``DccServerBase`` owns the public API; this collaborator keeps event
    construction, event coercion, and fail-safe no-registry behavior out of the
    already-large server class.
    """

    def __init__(self, dcc_name: str, hooks_getter: Callable[[], Any | None]) -> None:
        self._dcc_name = dcc_name
        self._hooks_getter = hooks_getter

    def dispatch(
        self,
        event: HookEvent | str,
        *,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        """Dispatch a lifecycle event and return the mutable payload.

        Returning the same payload object lets ``before_*`` hooks enrich bounded
        context (for example search tags or policy metadata) without inventing a
        separate return protocol. If no registry is installed this is a no-op.
        """
        event = event if isinstance(event, HookEvent) else HookEvent(event)
        event_payload = dict(payload or {})
        hooks = self._hooks_getter()
        if hooks is None:
            return event_payload
        hooks.dispatch(
            HookContext(
                event=event,
                dcc_name=self._dcc_name,
                session_id=session_id,
                payload=event_payload,
            )
        )
        return event_payload


__all__ = ["LifecycleEventDispatcher"]
