"""Escape-hatch demotion policy for generic-scripting tools (issue #1325).

When a backend exposes generic scripting tools (``execute_python``, host
script eval, MaxScript-style execution, …), the gateway search ranker
already demotes them (see ``ESCAPE_HATCH_DIVISOR`` in
``crates/dcc-mcp-gateway-search/src/ranking.rs``) so typed alternatives
surface first. This module adds the matching invocation-time policy:

* invoking a tool whose ``tool_role`` is ``escape_hatch`` requires a
  structured ``reason`` in the call meta;
* the policy fires through the :class:`LifecycleHooks`
  ``BEFORE_TOOL_CALL`` event and raises :class:`HookDeny` when a reason
  is missing;
* a bounded telemetry counter records every justified escape-hatch
  invocation by ``(dcc_name, tool_role, reason_category)``.

External design references:

* Claude Code Bash permissions and ``PreToolUse`` hooks
  (https://code.claude.com/docs/en/settings, https://code.claude.com/docs/en/hooks).
* OpenAI Codex shell safety: sandboxing + approvals + audit
  (https://openai.com/index/running-codex-safely/).
"""

from __future__ import annotations

from dataclasses import dataclass
import logging
from typing import Any
from typing import Callable

from dcc_mcp_core.lifecycle_hooks import HookContext
from dcc_mcp_core.lifecycle_hooks import HookDeny
from dcc_mcp_core.lifecycle_hooks import HookEvent
from dcc_mcp_core.lifecycle_hooks import LifecycleHooks

logger = logging.getLogger(__name__)

ESCAPE_HATCH_ROLE = "escape_hatch"
HOST_SCRIPT_RISK = "host_script_execution"

# Keys the policy looks for in the ``before_tool_call`` payload. Adapter code
# building the HookContext is expected to populate these from the call's MCP
# meta block (``arguments.meta.escape_hatch_reason`` / similar) and the
# selected tool's declared metadata.
REASON_KEY = "escape_hatch_reason"
ROLE_KEY = "tool_role"
RISK_KEY = "risk"


@dataclass(frozen=True)
class EscapeHatchInvocation:
    """One justified escape-hatch invocation observed by the policy."""

    dcc_name: str
    tool_name: str
    tool_role: str
    reason_category: str
    reason: str


class EscapeHatchPolicy:
    """Enforce the structured-reason requirement for escape-hatch tools.

    Install once per ``DccServerBase`` by binding to a :class:`LifecycleHooks`
    registry::

        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        server.register_lifecycle_hooks(hooks)

    The policy is intentionally pure: it does not store raw prompts, only
    short ``(dcc_name, tool_name, tool_role, reason_category)`` tuples for
    audit/telemetry export.
    """

    def __init__(
        self,
        *,
        telemetry_sink: Callable[[EscapeHatchInvocation], None] | None = None,
    ) -> None:
        self._telemetry_sink = telemetry_sink
        self._observed: list[EscapeHatchInvocation] = []

    def install(self, hooks: LifecycleHooks) -> EscapeHatchPolicy:
        """Subscribe ``BEFORE_TOOL_CALL`` on ``hooks``; return ``self`` for chaining."""
        hooks.on(HookEvent.BEFORE_TOOL_CALL, self._on_before_tool_call)
        return self

    def observed(self) -> tuple[EscapeHatchInvocation, ...]:
        """Read-only snapshot of justified escape-hatch invocations."""
        return tuple(self._observed)

    def _on_before_tool_call(self, ctx: HookContext) -> None:
        payload = ctx.payload or {}
        role = _str(payload.get(ROLE_KEY))
        risk = _str(payload.get(RISK_KEY))
        if role != ESCAPE_HATCH_ROLE and risk != HOST_SCRIPT_RISK:
            return  # not an escape-hatch invocation

        reason = _str(payload.get(REASON_KEY))
        if not reason:
            raise HookDeny(
                f"tool {payload.get('tool_name')!r} is an escape-hatch "
                f"(tool_role={role or 'unset'}, risk={risk or 'unset'}); "
                "callers must supply meta.escape_hatch_reason with the "
                "missing typed capability or failed search intent",
                hint="search for a typed skill first, or pass "
                "meta.escape_hatch_reason='no_typed_skill_found' "
                "after confirming nothing typed matches the query",
            )

        invocation = EscapeHatchInvocation(
            dcc_name=ctx.dcc_name,
            tool_name=_str(payload.get("tool_name")),
            tool_role=role or ESCAPE_HATCH_ROLE,
            reason_category=_categorise_reason(reason),
            reason=reason,
        )
        self._observed.append(invocation)
        if self._telemetry_sink is not None:
            try:
                self._telemetry_sink(invocation)
            except Exception as exc:
                logger.warning("[escape-hatch] telemetry sink failed: %s", exc)


_KNOWN_REASON_CATEGORIES = {
    "no_typed_skill_found",
    "debug",
    "user_requested_script",
    "automation",
}


def _categorise_reason(reason: str) -> str:
    lower = reason.strip().lower()
    if lower in _KNOWN_REASON_CATEGORIES:
        return lower
    return "custom"


def _str(value: Any) -> str:
    if value is None:
        return ""
    return str(value)


__all__ = ["ESCAPE_HATCH_ROLE", "HOST_SCRIPT_RISK", "EscapeHatchInvocation", "EscapeHatchPolicy"]
