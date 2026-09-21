"""Tests for the escape-hatch demotion policy (issue #1325)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import DEFAULT_PROMOTION_THRESHOLD
from dcc_mcp_core import PROMOTION_HINT_KEY
from dcc_mcp_core import EscapeHatchInvocation
from dcc_mcp_core import EscapeHatchPolicy
from dcc_mcp_core import EscapeHatchPromotionCandidate
from dcc_mcp_core import HookContext
from dcc_mcp_core import HookDeny
from dcc_mcp_core import HookEvent
from dcc_mcp_core import LifecycleHooks
from dcc_mcp_core import install_escape_hatch_policy

SHA_A = "a" * 64
SHA_B = "b" * 64


def _context(**payload) -> HookContext:
    return HookContext(event=HookEvent.BEFORE_TOOL_CALL, dcc_name="maya", payload=payload)


def _after_context(**payload) -> HookContext:
    return HookContext(event=HookEvent.AFTER_TOOL_CALL, dcc_name="maya", payload=payload)


def _escape_hatch_call(**payload) -> dict:
    """Payload for one justified escape-hatch invocation."""
    return {
        "tool_name": "execute_python",
        "tool_role": "escape_hatch",
        "escape_hatch_reason": "no_typed_skill_found",
        **payload,
    }


def _run(hooks: LifecycleHooks, **payload) -> tuple[dict, dict]:
    """Drive one before/after tool-call pair and return both payloads."""
    before = _context(**payload)
    hooks.dispatch(before)
    after = _after_context(
        tool_name=payload.get("tool_name", "execute_python"),
        ok=True,
        **{key: value for key, value in payload.items() if key.startswith("script_") or key == "materialized_script"},
    )
    hooks.dispatch(after)
    return before.payload, after.payload


class TestEscapeHatchPolicy:
    def test_install_returns_self_for_chaining(self) -> None:
        hooks = LifecycleHooks()
        policy = EscapeHatchPolicy()
        assert policy.install(hooks) is policy
        assert hooks.handlers(HookEvent.BEFORE_TOOL_CALL) != ()

    def test_typed_tool_call_is_allowed_without_reason(self) -> None:
        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        # tool_role: action — no demotion, no reason required
        hooks.dispatch(_context(tool_name="usd_import", tool_role="action"))

    def test_escape_hatch_without_reason_is_denied(self) -> None:
        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        with pytest.raises(HookDeny) as info:
            hooks.dispatch(_context(tool_name="execute_python", tool_role="escape_hatch"))
        assert "escape-hatch" in info.value.reason
        assert info.value.hint is not None

    def test_escape_hatch_with_known_reason_is_recorded(self) -> None:
        hooks = LifecycleHooks()
        policy = EscapeHatchPolicy().install(hooks)
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="no_typed_skill_found",
            )
        )
        assert policy.observed() == (
            EscapeHatchInvocation(
                dcc_name="maya",
                tool_name="execute_python",
                tool_role="escape_hatch",
                reason_category="no_typed_skill_found",
                reason="no_typed_skill_found",
            ),
        )

    def test_unknown_reason_is_categorised_as_custom(self) -> None:
        hooks = LifecycleHooks()
        policy = EscapeHatchPolicy().install(hooks)
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="studio-specific exporter workaround",
            )
        )
        assert policy.observed()[0].reason_category == "custom"

    def test_host_script_risk_alone_triggers_policy(self) -> None:
        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        # tool_role unset, risk = host_script_execution must still require a reason
        with pytest.raises(HookDeny):
            hooks.dispatch(_context(tool_name="maxscript_eval", risk="host_script_execution"))

    def test_telemetry_sink_receives_each_invocation(self) -> None:
        hooks = LifecycleHooks()
        captured: list[EscapeHatchInvocation] = []
        policy = EscapeHatchPolicy(telemetry_sink=captured.append).install(hooks)
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="debug",
            )
        )
        assert len(captured) == 1
        assert captured[0].reason_category == "debug"
        assert policy.observed() == tuple(captured)

    def test_telemetry_records_only_valid_script_identity(self) -> None:
        hooks = LifecycleHooks()
        policy = EscapeHatchPolicy().install(hooks)
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="debug",
                script_sha256="b" * 64,
                script_reused=False,
                script_reuse_key="layout-export",
                code="print('must never enter telemetry')",
            )
        )

        invocation = policy.observed()[0]
        assert invocation.script_sha256 == "b" * 64
        assert invocation.script_reused is False
        assert invocation.script_reuse_key == "layout-export"
        assert "must never enter telemetry" not in repr(invocation)

    def test_telemetry_sink_failure_does_not_crash_dispatch(self) -> None:
        def broken(_inv: EscapeHatchInvocation) -> None:
            raise RuntimeError("sink down")

        hooks = LifecycleHooks()
        policy = EscapeHatchPolicy(telemetry_sink=broken).install(hooks)
        # Must not raise
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="debug",
            )
        )
        assert len(policy.observed()) == 1

    def test_observed_snapshot_is_immutable_tuple(self) -> None:
        policy = EscapeHatchPolicy()
        hooks = LifecycleHooks()
        policy.install(hooks)
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="debug",
            )
        )
        snap = policy.observed()
        assert isinstance(snap, tuple)
        # Mutating after snapshot still grows the underlying list
        hooks.dispatch(
            _context(
                tool_name="execute_python",
                tool_role="escape_hatch",
                escape_hatch_reason="debug",
            )
        )
        assert len(snap) == 1
        assert len(policy.observed()) == 2

    def test_empty_payload_is_allowed(self) -> None:
        hooks = LifecycleHooks()
        EscapeHatchPolicy().install(hooks)
        # No role / risk -> not an escape-hatch, no deny
        hooks.dispatch(_context(tool_name="other_tool"))


class TestInstallEntryPoint:
    def test_install_escape_hatch_policy_registers_both_events(self) -> None:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        assert isinstance(policy, EscapeHatchPolicy)
        assert len(hooks.handlers(HookEvent.BEFORE_TOOL_CALL)) == 1
        assert len(hooks.handlers(HookEvent.AFTER_TOOL_CALL)) == 1

    def test_server_target_gets_a_registry_bound(self) -> None:
        class _Server:
            def __init__(self) -> None:
                self.hooks = None

            def lifecycle_hooks(self):
                return self.hooks

            def register_lifecycle_hooks(self, hooks):
                self.hooks = hooks
                return hooks

        server = _Server()
        install_escape_hatch_policy(server)
        assert isinstance(server.hooks, LifecycleHooks)
        assert len(server.hooks.handlers(HookEvent.AFTER_TOOL_CALL)) == 1

    def test_existing_server_hooks_are_reused(self) -> None:
        class _Server:
            def __init__(self, hooks):
                self.hooks = hooks

            def lifecycle_hooks(self):
                return self.hooks

        hooks = LifecycleHooks()
        install_escape_hatch_policy(_Server(hooks))
        assert len(hooks.handlers(HookEvent.BEFORE_TOOL_CALL)) == 1

    def test_unsupported_target_is_rejected(self) -> None:
        with pytest.raises(TypeError):
            install_escape_hatch_policy(object())


class TestPromotionHint:
    def test_hint_appears_on_the_call_after_the_threshold(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)

        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            before, after = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
            assert PROMOTION_HINT_KEY not in before
            assert PROMOTION_HINT_KEY not in after

        before, after = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
        assert "candidate_id=" in before[PROMOTION_HINT_KEY]
        assert "suggested_skill_name=" in before[PROMOTION_HINT_KEY]
        assert "candidate_id=" in after[PROMOTION_HINT_KEY]
        assert "suggested_skill_name=" in after[PROMOTION_HINT_KEY]

    def test_hint_uses_the_durable_candidate_identity(self) -> None:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 1):
            _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))

        (candidate,) = policy.promotion_candidates()
        assert isinstance(candidate, EscapeHatchPromotionCandidate)
        assert candidate.candidate_id.startswith("script:")
        assert candidate.sha256 == SHA_A
        assert candidate.tool_name == "execute_python"
        assert candidate.execution_count == DEFAULT_PROMOTION_THRESHOLD + 1
        assert candidate.threshold == DEFAULT_PROMOTION_THRESHOLD
        assert candidate.hint().startswith("escape-hatch script")

    def test_threshold_is_configurable(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks, promotion_threshold=1)

        _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
        before, _ = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
        assert PROMOTION_HINT_KEY in before

    def test_hints_can_be_switched_off(self) -> None:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks, promotion_hints=False)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 2):
            before, after = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
            assert PROMOTION_HINT_KEY not in before
            assert PROMOTION_HINT_KEY not in after
        # Counting still happens; only the advisory output is suppressed.
        assert len(policy.promotion_candidates()) == 1

    def test_scripts_are_counted_independently(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))

        before, _ = _run(hooks, **_escape_hatch_call(script_sha256=SHA_B))
        assert PROMOTION_HINT_KEY not in before

    def test_call_without_script_identity_is_never_hinted(self) -> None:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 2):
            before, after = _run(hooks, **_escape_hatch_call(script_sha256="not-a-digest"))
            assert PROMOTION_HINT_KEY not in before
            assert PROMOTION_HINT_KEY not in after
        assert policy.promotion_candidates() == ()

    def test_denied_call_does_not_count_towards_the_threshold(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)
        # Without a reason the policy vetoes, so nothing is recorded.
        with pytest.raises(HookDeny):
            hooks.dispatch(_context(tool_name="execute_python", tool_role="escape_hatch", script_sha256=SHA_A))

        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            before, _ = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))
            assert PROMOTION_HINT_KEY not in before

    def test_after_call_without_before_still_reports_a_known_script(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 1):
            _run(hooks, **_escape_hatch_call(script_sha256=SHA_A))

        after = _after_context(tool_name="execute_python", ok=True, script_sha256=SHA_A)
        hooks.dispatch(after)
        assert PROMOTION_HINT_KEY in after.payload

    def test_materialized_script_payload_is_understood(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)
        script = {"sha256": SHA_A, "reused": True, "reuse_key": "layout-export"}
        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            _run(hooks, **_escape_hatch_call(materialized_script=script))

        before, after = _run(hooks, **_escape_hatch_call(materialized_script=script))
        assert PROMOTION_HINT_KEY in before
        assert PROMOTION_HINT_KEY in after

    def test_repeat_tracker_is_bounded(self) -> None:
        from dcc_mcp_core.escape_hatch_policy import MAX_TRACKED_SCRIPTS

        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        for index in range(MAX_TRACKED_SCRIPTS + 8):
            _run(hooks, **_escape_hatch_call(script_sha256=f"{index:064x}"))
        # Only identity is tracked; no script body or prompt text is retained.
        assert len(policy._repeats) <= MAX_TRACKED_SCRIPTS

    def test_invalid_threshold_is_rejected(self) -> None:
        with pytest.raises(ValueError):
            EscapeHatchPolicy(promotion_threshold=0)
