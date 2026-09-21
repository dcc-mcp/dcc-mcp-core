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

Skill-promotion hints (issue #2297):

* every justified invocation that carries a materialized ``script_sha256``
  also bumps an in-process per-script repeat counter;
* once a script has been run more than ``promotion_threshold`` times the
  policy writes an advisory hint into the mutable hook payload under
  :data:`PROMOTION_HINT_KEY`. ``LifecycleEventDispatcher.dispatch`` returns
  that same payload dict to ``dispatch_before_tool_call`` /
  ``dispatch_after_tool_call``, so the hint reaches the agent through the
  existing tool-call response channel instead of a new response type. The
  hint reuses the agent-facing remediation semantics of :class:`HookDeny`'s
  ``hint`` (advice, never a veto): it names a ``candidate_id`` and a
  ``suggested_skill_name`` so the agent can promote the repeated script to
  a typed skill.
* the policy is installed explicitly through
  :func:`install_escape_hatch_policy`, which accepts either a
  :class:`LifecycleHooks` registry or a server exposing
  ``register_lifecycle_hooks()``.

External design references:

* Claude Code Bash permissions and ``PreToolUse`` hooks
  (https://code.claude.com/docs/en/settings, https://code.claude.com/docs/en/hooks).
* OpenAI Codex shell safety: sandboxing + approvals + audit
  (https://openai.com/index/running-codex-safely/).
"""

from __future__ import annotations

from dataclasses import dataclass
import logging
import time
from typing import Any
from typing import Callable

from dcc_mcp_core.lifecycle_hooks import HookContext
from dcc_mcp_core.lifecycle_hooks import HookDeny
from dcc_mcp_core.lifecycle_hooks import HookEvent
from dcc_mcp_core.lifecycle_hooks import LifecycleHooks

logger = logging.getLogger(__name__)

ESCAPE_HATCH_ROLE = "escape_hatch"
HOST_SCRIPT_RISK = "host_script_execution"

# Keys the policy looks for in the ``before_tool_call`` / ``after_tool_call``
# payload. Adapter code building the HookContext is expected to populate these
# from the call's MCP meta block (``arguments.meta.escape_hatch_reason`` /
# similar) and the selected tool's declared metadata.
REASON_KEY = "escape_hatch_reason"
ROLE_KEY = "tool_role"
RISK_KEY = "risk"

# Skill-promotion hint configuration (#2297). A script that has already been
# executed ``DEFAULT_PROMOTION_THRESHOLD`` times carries a promotion hint on
# every later invocation, so with the default the hint first appears on the
# fourth run of the same script.
DEFAULT_PROMOTION_THRESHOLD = 3
PROMOTION_HINT_KEY = "escape_hatch_promotion_hint"

# Bound for the in-process repeat tracker: keys are (dcc, tool, sha256) tuples
# and the oldest entry is evicted once the bound is reached.
MAX_TRACKED_SCRIPTS = 256


@dataclass(frozen=True)
class EscapeHatchInvocation:
    """One justified escape-hatch invocation observed by the policy."""

    dcc_name: str
    tool_name: str
    tool_role: str
    reason_category: str
    reason: str
    script_sha256: str | None = None
    script_reused: bool | None = None
    script_reuse_key: str | None = None


@dataclass(frozen=True)
class EscapeHatchPromotionCandidate:
    """Advisory skill-promotion candidate for one repeated escape-hatch script.

    The payload is advisory only: it never writes a skill, never publishes
    anything, and never vetoes a call. Field names mirror the durable
    ``SkillPromotionProposal`` produced by ``observability_query`` so an
    in-process hint and a durable audit candidate can be correlated by
    ``candidate_id``.
    """

    candidate_id: str
    sha256: str
    dcc_name: str
    tool_name: str
    execution_count: int
    threshold: int
    suggested_skill_name: str

    def hint(self) -> str:
        """Render the agent-facing remediation string for this candidate."""
        return (
            f"escape-hatch script {self.sha256[:12]} has now run "
            f"{self.execution_count} times (threshold {self.threshold}); "
            f"promote it to a typed skill instead of re-running it: "
            f"candidate_id={self.candidate_id}, "
            f"suggested_skill_name={self.suggested_skill_name}."
        )


@dataclass(frozen=True)
class _ScriptRepeat:
    """Bounded in-process repeat counter for one tracked script."""

    count: int
    first_seen_ms: int
    last_seen_ms: int


class EscapeHatchPolicy:
    """Enforce the structured-reason requirement for escape-hatch tools.

    Install once per ``DccServerBase`` by binding to a :class:`LifecycleHooks`
    registry::

        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        server.register_lifecycle_hooks(hooks)

    The policy is intentionally pure: it does not store raw prompts, only
    short ``(dcc_name, tool_name, tool_role, reason_category)`` tuples for
    audit/telemetry export. Skill-promotion hints additionally keep a bounded
    ``(dcc_name, tool_name, sha256)`` repeat counter; script bodies are never
    retained.
    """

    def __init__(
        self,
        *,
        telemetry_sink: Callable[[EscapeHatchInvocation], None] | None = None,
        promotion_threshold: int = DEFAULT_PROMOTION_THRESHOLD,
        promotion_hints: bool = True,
    ) -> None:
        if isinstance(promotion_threshold, bool) or promotion_threshold < 1:
            raise ValueError("promotion_threshold must be a positive integer")
        self._telemetry_sink = telemetry_sink
        self._promotion_threshold = int(promotion_threshold)
        self._promotion_hints = bool(promotion_hints)
        self._observed: list[EscapeHatchInvocation] = []
        self._repeats: dict[tuple[str, str, str], _ScriptRepeat] = {}

    def install(self, hooks: LifecycleHooks) -> EscapeHatchPolicy:
        """Subscribe ``BEFORE_TOOL_CALL``/``AFTER_TOOL_CALL``; return ``self``."""
        hooks.on(HookEvent.BEFORE_TOOL_CALL, self._on_before_tool_call)
        hooks.on(HookEvent.AFTER_TOOL_CALL, self._on_after_tool_call)
        return self

    def observed(self) -> tuple[EscapeHatchInvocation, ...]:
        """Read-only snapshot of justified escape-hatch invocations."""
        return tuple(self._observed)

    def promotion_candidates(self) -> tuple[EscapeHatchPromotionCandidate, ...]:
        """Candidates whose repeat count already exceeds the threshold."""
        return tuple(
            self._candidate(dcc_name, tool_name, sha256, repeat)
            for (dcc_name, tool_name, sha256), repeat in self._repeats.items()
            if repeat.count > self._promotion_threshold
        )

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

        script = payload.get("materialized_script")
        script_meta = script if isinstance(script, dict) else {}
        invocation = EscapeHatchInvocation(
            dcc_name=ctx.dcc_name,
            tool_name=_str(payload.get("tool_name")),
            tool_role=role or ESCAPE_HATCH_ROLE,
            reason_category=_categorise_reason(reason),
            reason=reason,
            script_sha256=_script_sha256(
                payload.get("script_sha256", script_meta.get("sha256")),
            ),
            script_reused=_optional_bool(
                payload.get("script_reused", script_meta.get("reused")),
            ),
            script_reuse_key=_bounded_identifier(
                payload.get("script_reuse_key", script_meta.get("reuse_key")),
            ),
        )
        self._observed.append(invocation)
        if self._telemetry_sink is not None:
            try:
                self._telemetry_sink(invocation)
            except Exception as exc:
                logger.warning("[escape-hatch] telemetry sink failed: %s", exc)

        if invocation.script_sha256:
            self._bump_repeat(ctx.dcc_name, invocation.tool_name, invocation.script_sha256)
            self._attach_promotion_hint(ctx, invocation.script_sha256, invocation.tool_name)

    def _on_after_tool_call(self, ctx: HookContext) -> None:
        """Re-attach the promotion hint to the post-call payload.

        ``BEFORE_TOOL_CALL`` already writes the hint, but hosts that only merge
        the ``after_tool_call`` payload into the tool response (or that build the
        before-payload before the policy can see the script identity) still get
        it. The handler never vetoes: it is advisory-only.
        """
        payload = ctx.payload or {}
        script = payload.get("materialized_script")
        script_meta = script if isinstance(script, dict) else {}
        sha256 = _script_sha256(payload.get("script_sha256", script_meta.get("sha256")))
        if not sha256:
            return
        self._attach_promotion_hint(ctx, sha256, _str(payload.get("tool_name")))

    def _bump_repeat(self, dcc_name: str, tool_name: str, sha256: str) -> None:
        key = (dcc_name, tool_name, sha256)
        now_ms = _now_ms()
        existing = self._repeats.get(key)
        if existing is None:
            if len(self._repeats) >= MAX_TRACKED_SCRIPTS:
                self._repeats.pop(next(iter(self._repeats)), None)
            self._repeats[key] = _ScriptRepeat(
                count=1,
                first_seen_ms=now_ms,
                last_seen_ms=now_ms,
            )
            return
        # Re-insert so eviction order stays least-recently-bumped.
        del self._repeats[key]
        self._repeats[key] = _ScriptRepeat(
            count=existing.count + 1,
            first_seen_ms=existing.first_seen_ms,
            last_seen_ms=now_ms,
        )

    def _attach_promotion_hint(self, ctx: HookContext, sha256: str, tool_name: str) -> None:
        if not self._promotion_hints:
            return
        repeat = self._repeats.get((ctx.dcc_name, tool_name, sha256))
        if repeat is None or repeat.count <= self._promotion_threshold:
            return
        payload = ctx.payload
        if payload is None:
            return
        try:
            payload[PROMOTION_HINT_KEY] = self._candidate(
                ctx.dcc_name,
                tool_name,
                sha256,
                repeat,
            ).hint()
        except Exception as exc:  # never let advisory output break a tool call
            logger.warning("[escape-hatch] promotion hint failed: %s", exc)

    def _candidate(
        self,
        dcc_name: str,
        tool_name: str,
        sha256: str,
        repeat: _ScriptRepeat,
    ) -> EscapeHatchPromotionCandidate:
        return EscapeHatchPromotionCandidate(
            candidate_id=escape_hatch_candidate_id(
                sha256=sha256,
                dcc_name=dcc_name,
                tool_name=tool_name,
            ),
            sha256=sha256,
            dcc_name=dcc_name,
            tool_name=tool_name,
            execution_count=repeat.count,
            threshold=self._promotion_threshold,
            suggested_skill_name=suggested_skill_name(dcc_name, tool_name, sha256),
        )


_KNOWN_REASON_CATEGORIES = {
    "no_typed_skill_found",
    "debug",
    "user_requested_script",
    "automation",
}


def install_escape_hatch_policy(
    target: Any,
    *,
    telemetry_sink: Callable[[EscapeHatchInvocation], None] | None = None,
    promotion_threshold: int = DEFAULT_PROMOTION_THRESHOLD,
    promotion_hints: bool = True,
) -> EscapeHatchPolicy:
    """Install :class:`EscapeHatchPolicy` on a hooks registry or a server.

    ``target`` may be a :class:`LifecycleHooks` registry (returned unchanged
    after installation) or any server exposing ``lifecycle_hooks()`` and
    ``register_lifecycle_hooks()`` — a registry is created and bound in that
    case, which is the one-line host installation path::

        install_escape_hatch_policy(server)

    Returns the installed policy so callers can read :meth:`observed` and
    :meth:`promotion_candidates`.
    """
    policy = EscapeHatchPolicy(
        telemetry_sink=telemetry_sink,
        promotion_threshold=promotion_threshold,
        promotion_hints=promotion_hints,
    )
    return policy.install(_resolve_hooks(target))


def _resolve_hooks(target: Any) -> LifecycleHooks:
    if isinstance(target, LifecycleHooks):
        return target
    # Duck-typed registry: exposes ``on``/``handlers`` like LifecycleHooks.
    if callable(getattr(target, "on", None)) and callable(getattr(target, "handlers", None)):
        return target
    existing = getattr(target, "lifecycle_hooks", None)
    if callable(existing):
        hooks = existing()
        if hooks is not None:
            return hooks
    register = getattr(target, "register_lifecycle_hooks", None)
    if callable(register):
        hooks = LifecycleHooks()
        register(hooks)
        return hooks
    raise TypeError(
        "install_escape_hatch_policy() expects a LifecycleHooks registry or an "
        f"object exposing register_lifecycle_hooks(); got {type(target).__name__}"
    )


def escape_hatch_candidate_id(
    *,
    sha256: str,
    dcc_name: str,
    tool_name: str,
    reuse_key: str | None = None,
) -> str:
    """Return the stable ``script:`` candidate identity for one script.

    Delegates to the durable query layer so an in-process hint and a stored
    ``promotion_candidate`` derive the same identity and can be correlated.
    """
    from dcc_mcp_core.observability_query import script_candidate_id

    return script_candidate_id(
        {
            "sha256": sha256,
            "reuse_key": reuse_key,
            "dcc_type": dcc_name,
            "tool_name": tool_name,
        }
    )


def suggested_skill_name(dcc_name: str, tool_name: str, sha256: str) -> str:
    """Return a deterministic, filesystem-safe skill name for a script."""
    from dcc_mcp_core.script_materialization import sanitize_materialization_segment

    dcc = sanitize_materialization_segment(dcc_name, default="any")
    tool = sanitize_materialization_segment(tool_name, default="script")
    return f"{dcc}-{tool}-{sha256[:8].lower()}"


def _categorise_reason(reason: str) -> str:
    lower = reason.strip().lower()
    if lower in _KNOWN_REASON_CATEGORIES:
        return lower
    return "custom"


def _now_ms() -> int:
    return time.time_ns() // 1_000_000


def _str(value: Any) -> str:
    if value is None:
        return ""
    return str(value)


def _script_sha256(value: Any) -> str | None:
    text = _str(value).strip().lower()
    if len(text) != 64 or any(char not in "0123456789abcdef" for char in text):
        return None
    return text


def _optional_bool(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def _bounded_identifier(value: Any) -> str | None:
    text = _str(value).strip()
    return text[:128] if text else None


__all__ = [
    "DEFAULT_PROMOTION_THRESHOLD",
    "ESCAPE_HATCH_ROLE",
    "HOST_SCRIPT_RISK",
    "PROMOTION_HINT_KEY",
    "EscapeHatchInvocation",
    "EscapeHatchPolicy",
    "EscapeHatchPromotionCandidate",
    "install_escape_hatch_policy",
]
