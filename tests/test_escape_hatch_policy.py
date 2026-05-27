"""Tests for the escape-hatch demotion policy (issue #1325)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import EscapeHatchInvocation
from dcc_mcp_core import EscapeHatchPolicy
from dcc_mcp_core import HookContext
from dcc_mcp_core import HookDeny
from dcc_mcp_core import HookEvent
from dcc_mcp_core import LifecycleHooks


def _context(**payload) -> HookContext:
    return HookContext(event=HookEvent.BEFORE_TOOL_CALL, dcc_name="maya", payload=payload)


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
