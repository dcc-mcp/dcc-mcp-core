"""Tests for the escape-hatch demotion policy (issue #1325)."""

from __future__ import annotations

import pytest
from tests._support.server import make_test_server

from dcc_mcp_core import DEFAULT_PROMOTION_THRESHOLD
from dcc_mcp_core import MAX_TRACKED_SCRIPTS
from dcc_mcp_core import PROMOTION_HINT_KEY
from dcc_mcp_core import EscapeHatchInvocation
from dcc_mcp_core import EscapeHatchPolicy
from dcc_mcp_core import EscapeHatchPromotionCandidate
from dcc_mcp_core import HookContext
from dcc_mcp_core import HookDeny
from dcc_mcp_core import HookEvent
from dcc_mcp_core import LifecycleHooks
from dcc_mcp_core import ObservabilityQuery
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
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        for index in range(MAX_TRACKED_SCRIPTS + 8):
            _run(hooks, **_escape_hatch_call(script_sha256=f"{index:064x}"))
        # Only identity is tracked; no script body or prompt text is retained.
        assert policy.tracked_script_count() <= MAX_TRACKED_SCRIPTS

    def test_eviction_drops_the_least_recently_repeated_script(self) -> None:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        cold = f"{0:064x}"
        warm = f"{1:064x}"
        _run(hooks, **_escape_hatch_call(script_sha256=cold))
        _run(hooks, **_escape_hatch_call(script_sha256=warm))

        # Overflow the tracker while re-touching ``warm`` on every round, so
        # ``cold`` is the only script left stale when eviction kicks in.
        for index in range(2, MAX_TRACKED_SCRIPTS + 8):
            _run(hooks, **_escape_hatch_call(script_sha256=f"{index:064x}"))
            _run(hooks, **_escape_hatch_call(script_sha256=warm))

        assert policy.tracked_script_count() <= MAX_TRACKED_SCRIPTS
        # ``warm`` accumulated enough repeats to be a candidate; ``cold`` was
        # evicted, so restarting it must begin from a clean counter.
        tracked = {candidate.sha256 for candidate in policy.promotion_candidates()}
        assert warm in tracked
        assert cold not in tracked

    def test_invalid_threshold_is_rejected(self) -> None:
        with pytest.raises(ValueError):
            EscapeHatchPolicy(promotion_threshold=0)

    @pytest.mark.parametrize("threshold", [1.9, 3.0, "3", True])
    def test_non_integer_threshold_is_rejected(self, threshold: object) -> None:
        """Non-integers must be rejected rather than truncated by ``int()``.

        Truncating 1.9 to 1 would raise the promotion hint one run earlier than
        the caller configured, so a wrong type must fail loudly.
        """
        with pytest.raises(ValueError):
            EscapeHatchPolicy(promotion_threshold=threshold)

    @pytest.mark.parametrize("threshold", [1.9, 3.0, "3", True])
    def test_install_rejects_non_integer_threshold(self, threshold: object) -> None:
        """The public ``install_escape_hatch_policy`` path rejects them too."""
        with pytest.raises(ValueError):
            install_escape_hatch_policy(LifecycleHooks(), promotion_threshold=threshold)

    def test_integer_threshold_is_kept_verbatim(self) -> None:
        """A valid integer threshold is stored without any coercion."""
        policy = install_escape_hatch_policy(LifecycleHooks(), promotion_threshold=7)
        assert policy._promotion_threshold == 7
        assert isinstance(policy._promotion_threshold, int)


class TestCandidateIdentityParity:
    """The in-process hint id must equal the durable query id (contract)."""

    DCC = "maya"
    TOOL = "execute_python"
    REUSE_KEY = "layout-export"

    def _durable_candidate_id(self, sha256: str, reuse_key: str | None) -> str:
        row = {
            "sha256": sha256,
            "reuse_key": reuse_key,
            "dcc_type": self.DCC,
            "tool_name": self.TOOL,
            "execution_count": 4,
            "session_count": 1,
            "reused_count": 0,
            "rematerialized_count": 4,
            "first_seen_ms": 1,
            "last_seen_ms": 4,
        }
        response = ObservabilityQuery(read_json_fn=lambda _sql, _params: [row]).get_repeated_scripts(min_repeats=2)
        return response["data"]["promotion_candidates"][0]["candidate_id"]

    def _in_process_candidate_id(self, sha256: str, reuse_key: str | None) -> str:
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        payload = _escape_hatch_call(script_sha256=sha256)
        if reuse_key is not None:
            payload["script_reuse_key"] = reuse_key
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 1):
            _run(hooks, **payload)
        (candidate,) = policy.promotion_candidates()
        return candidate.candidate_id

    def test_without_reuse_key_matches_the_durable_digest(self) -> None:
        sha256 = SHA_A
        assert self._in_process_candidate_id(sha256, None) == self._durable_candidate_id(sha256, None)

    def test_with_reuse_key_matches_the_durable_digest(self) -> None:
        """Regression: reuse_key is hashed by the durable side, so we must hash it too."""
        sha256 = SHA_A
        assert self._in_process_candidate_id(sha256, self.REUSE_KEY) == self._durable_candidate_id(
            sha256, self.REUSE_KEY
        )

    def test_reuse_key_changes_the_identity(self) -> None:
        assert self._in_process_candidate_id(SHA_A, None) != self._in_process_candidate_id(SHA_A, self.REUSE_KEY)

    def test_one_counter_never_feeds_two_identities(self) -> None:
        """Same script, two reuse keys: counted separately, hinted separately."""
        hooks = LifecycleHooks()
        policy = install_escape_hatch_policy(hooks)
        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            _run(hooks, **_escape_hatch_call(script_sha256=SHA_A, script_reuse_key="key-a"))

        # A different reuse key must not inherit the accumulated count.
        before, _ = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A, script_reuse_key="key-b"))
        assert PROMOTION_HINT_KEY not in before

        before, _ = _run(hooks, **_escape_hatch_call(script_sha256=SHA_A, script_reuse_key="key-a"))
        assert PROMOTION_HINT_KEY in before

        ids = {candidate.candidate_id for candidate in policy.promotion_candidates()}
        assert ids == {
            self._durable_candidate_id(SHA_A, "key-a"),
        }

    def test_before_and_after_hints_carry_the_same_identity(self) -> None:
        hooks = LifecycleHooks()
        install_escape_hatch_policy(hooks)
        payload = _escape_hatch_call(
            script_sha256=SHA_A,
            materialized_script={"sha256": SHA_A, "reuse_key": self.REUSE_KEY},
        )
        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            _run(hooks, **payload)

        before, after = _run(hooks, **payload)
        assert before[PROMOTION_HINT_KEY] == after[PROMOTION_HINT_KEY]
        assert self._durable_candidate_id(SHA_A, self.REUSE_KEY) in before[PROMOTION_HINT_KEY]


class TestServerIntegration:
    """End-to-end: the hint must survive a real server dispatch round-trip."""

    def test_hint_reaches_the_dispatch_before_tool_call_return_value(self) -> None:
        server = make_test_server(server=object(), dcc_name="maya")
        install_escape_hatch_policy(server)

        payload = {
            "tool_name": "execute_python",
            "tool_role": "escape_hatch",
            "escape_hatch_reason": "no_typed_skill_found",
            "script_sha256": SHA_A,
        }
        for _ in range(DEFAULT_PROMOTION_THRESHOLD):
            returned = server.dispatch_before_tool_call("execute_python", payload=dict(payload))
            assert PROMOTION_HINT_KEY not in returned

        returned = server.dispatch_before_tool_call("execute_python", payload=dict(payload))
        assert "candidate_id=" in returned[PROMOTION_HINT_KEY]
        assert "suggested_skill_name=" in returned[PROMOTION_HINT_KEY]

    def test_hint_reaches_the_dispatch_after_tool_call_return_value(self) -> None:
        server = make_test_server(server=object(), dcc_name="maya")
        install_escape_hatch_policy(server)

        before_payload = {
            "tool_name": "execute_python",
            "tool_role": "escape_hatch",
            "escape_hatch_reason": "no_typed_skill_found",
            "script_sha256": SHA_A,
        }
        for _ in range(DEFAULT_PROMOTION_THRESHOLD + 1):
            server.dispatch_before_tool_call("execute_python", payload=dict(before_payload))

        returned = server.dispatch_after_tool_call("execute_python", ok=True, payload={"script_sha256": SHA_A})
        assert "candidate_id=" in returned[PROMOTION_HINT_KEY]

    def test_install_on_server_binds_a_registry(self) -> None:
        server = make_test_server(server=object(), dcc_name="maya")
        policy = install_escape_hatch_policy(server)
        assert server.lifecycle_hooks() is not None
        assert len(server.lifecycle_hooks().handlers(HookEvent.AFTER_TOOL_CALL)) == 1
        assert policy.promotion_candidates() == ()
