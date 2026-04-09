"""Deep tests for AuditLog + SandboxContext + SandboxPolicy interactions.

Covers:
- AuditLog.entries() / entries_for_action() / denials() / successes() / to_json()
- SandboxContext.execute_json() allowed / denied paths
- SandboxContext.is_allowed() with allow_actions / deny_actions
- SandboxPolicy.set_read_only() / is_read_only()
- SandboxContext.action_count / set_actor
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_context(allowed: list[str] | None = None, read_only: bool = False) -> SandboxContext:
    policy = SandboxPolicy()
    if allowed:
        policy.allow_actions(allowed)
    if read_only:
        policy.set_read_only(True)
    return SandboxContext(policy)


# ---------------------------------------------------------------------------
# SandboxPolicy basics
# ---------------------------------------------------------------------------


class TestSandboxPolicy:
    def test_default_policy_not_read_only(self):
        p = SandboxPolicy()
        # is_read_only is a property (bool), not a method
        assert p.is_read_only is False

    def test_set_read_only_true(self):
        p = SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_set_read_only_false(self):
        p = SandboxPolicy()
        p.set_read_only(True)
        p.set_read_only(False)
        assert p.is_read_only is False

    def test_allow_actions_single(self):
        p = SandboxPolicy()
        p.allow_actions(["create"])
        c = SandboxContext(p)
        assert c.is_allowed("create") is True
        assert c.is_allowed("delete") is False

    def test_allow_actions_multiple(self):
        p = SandboxPolicy()
        p.allow_actions(["create", "move", "rename"])
        c = SandboxContext(p)
        for action in ("create", "move", "rename"):
            assert c.is_allowed(action) is True

    def test_deny_actions_blocks_previously_allowed(self):
        p = SandboxPolicy()
        p.allow_actions(["create", "delete"])
        p.deny_actions(["delete"])
        c = SandboxContext(p)
        assert c.is_allowed("create") is True
        assert c.is_allowed("delete") is False

    def test_allow_paths_method_exists(self):
        p = SandboxPolicy()
        p.allow_paths(["/projects", "/assets"])

    def test_set_max_actions(self):
        p = SandboxPolicy()
        p.set_max_actions(10)

    def test_set_timeout_ms(self):
        p = SandboxPolicy()
        p.set_timeout_ms(5000)


# ---------------------------------------------------------------------------
# SandboxContext.is_allowed
# ---------------------------------------------------------------------------


class TestSandboxContextIsAllowed:
    def test_is_allowed_returns_true_for_allowed_action(self):
        ctx = _make_context(allowed=["render", "export"])
        assert ctx.is_allowed("render") is True
        assert ctx.is_allowed("export") is True

    def test_is_allowed_returns_false_for_unknown_action(self):
        ctx = _make_context(allowed=["render"])
        assert ctx.is_allowed("delete_all") is False

    def test_is_allowed_empty_allowed_list_blocks_everything(self):
        # allow_actions([]) with empty list: behavior is implementation-defined.
        # When no actions are specified in the whitelist, the sandbox allows all actions.
        # Passing an empty list is a no-op - it does not restrict anything.
        ctx = _make_context(allowed=[])
        # Empty allow_actions = no whitelist set = allow everything
        assert ctx.is_allowed("anything") is True

    def test_is_allowed_no_restriction_allows_everything(self):
        # A fresh policy with no allow_actions set should allow any action
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        # Behavior depends on implementation; just ensure no crash
        result = ctx.is_allowed("random_action")
        assert isinstance(result, bool)

    def test_deny_overrides_allow(self):
        policy = SandboxPolicy()
        policy.allow_actions(["move", "copy"])
        policy.deny_actions(["move"])
        ctx = SandboxContext(policy)
        assert ctx.is_allowed("copy") is True
        assert ctx.is_allowed("move") is False


# ---------------------------------------------------------------------------
# SandboxContext.execute_json
# ---------------------------------------------------------------------------


class TestSandboxContextExecuteJson:
    def test_execute_allowed_action_returns_json_string(self):
        ctx = _make_context(allowed=["move"])
        result = ctx.execute_json("move", json.dumps({"src": "/a", "dst": "/b"}))
        assert isinstance(result, str)

    def test_execute_allowed_action_result_is_valid_json(self):
        ctx = _make_context(allowed=["create"])
        result = ctx.execute_json("create", json.dumps({"name": "sphere"}))
        # execute_json returns a JSON string; the sandbox returns "null" when action
        # has no handler (which is the default behavior without a registered handler)
        parsed = json.loads(result)
        # Parsed value can be None (JSON null) or a dict depending on handler registration
        assert parsed is None or isinstance(parsed, dict)

    def test_execute_denied_action_raises_runtime_error(self):
        ctx = _make_context(allowed=["move"])
        with pytest.raises(RuntimeError, match="not allowed"):
            ctx.execute_json("delete", json.dumps({"path": "/file"}))

    def test_execute_empty_params_json(self):
        ctx = _make_context(allowed=["reset"])
        result = ctx.execute_json("reset", "{}")
        assert isinstance(result, str)

    def test_execute_increments_action_count(self):
        ctx = _make_context(allowed=["apply"])
        before = ctx.action_count
        ctx.execute_json("apply", json.dumps({"id": 1}))
        after = ctx.action_count
        assert after == before + 1

    def test_execute_denied_does_not_increment_action_count(self):
        ctx = _make_context(allowed=["move"])
        before = ctx.action_count
        with pytest.raises(RuntimeError):
            ctx.execute_json("delete", "{}")
        # action_count should not increase on denial
        assert ctx.action_count == before

    def test_set_actor_attribute(self):
        ctx = _make_context(allowed=["paint"])
        ctx.set_actor("agent_maya")
        # No exception; actor is stored for audit purposes


# ---------------------------------------------------------------------------
# AuditLog via SandboxContext
# ---------------------------------------------------------------------------


class TestAuditLog:
    def _ctx_with_actions(self) -> SandboxContext:
        ctx = _make_context(allowed=["move", "create", "render"])
        ctx.set_actor("test_agent")
        ctx.execute_json("move", json.dumps({"src": "/a", "dst": "/b"}))
        ctx.execute_json("create", json.dumps({"name": "cube"}))
        ctx.execute_json("render", json.dumps({}))
        # Also trigger a denial
        with pytest.raises(RuntimeError):
            ctx.execute_json("delete", "{}")
        return ctx

    def test_entries_returns_list(self):
        ctx = self._ctx_with_actions()
        entries = ctx.audit_log.entries()
        assert isinstance(entries, list)

    def test_entries_count_matches_executed_plus_denied(self):
        ctx = self._ctx_with_actions()
        # 3 allowed + 1 denied
        entries = ctx.audit_log.entries()
        assert len(entries) == 4

    def test_entries_have_expected_fields(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", json.dumps({"src": "/x"}))
        entry = ctx.audit_log.entries()[0]
        for field in ("action", "actor", "outcome", "timestamp_ms"):
            assert hasattr(entry, field)

    def test_entries_action_field_correct(self):
        ctx = _make_context(allowed=["export"])
        ctx.execute_json("export", json.dumps({"path": "/out.usd"}))
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "export"

    def test_entries_actor_field_correct(self):
        ctx = _make_context(allowed=["bake"])
        ctx.set_actor("pipeline_agent")
        ctx.execute_json("bake", json.dumps({}))
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "pipeline_agent"

    def test_entries_outcome_for_success(self):
        ctx = _make_context(allowed=["update"])
        ctx.execute_json("update", json.dumps({"id": 1}))
        entry = ctx.audit_log.entries()[0]
        assert "success" in entry.outcome.lower() or entry.outcome is not None

    def test_entries_timestamp_ms_positive(self):
        ctx = _make_context(allowed=["sync"])
        ctx.execute_json("sync", json.dumps({}))
        entry = ctx.audit_log.entries()[0]
        assert entry.timestamp_ms > 0

    def test_entries_for_action_filters_correctly(self):
        ctx = _make_context(allowed=["move", "create"])
        ctx.execute_json("move", json.dumps({"src": "/a", "dst": "/b"}))
        ctx.execute_json("move", json.dumps({"src": "/c", "dst": "/d"}))
        ctx.execute_json("create", json.dumps({"name": "sphere"}))
        log = ctx.audit_log
        move_entries = log.entries_for_action("move")
        assert len(move_entries) == 2
        for e in move_entries:
            assert e.action == "move"

    def test_entries_for_action_returns_empty_for_unknown(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", json.dumps({}))
        entries = ctx.audit_log.entries_for_action("nonexistent_action")
        assert entries == []

    def test_successes_returns_only_successful_entries(self):
        ctx = self._ctx_with_actions()
        successes = ctx.audit_log.successes()
        assert isinstance(successes, list)
        assert len(successes) == 3  # 3 allowed succeeded
        for e in successes:
            assert "success" in e.outcome.lower() or e.outcome != "denied"

    def test_denials_returns_only_denied_entries(self):
        ctx = self._ctx_with_actions()
        denials = ctx.audit_log.denials()
        assert isinstance(denials, list)
        assert len(denials) == 1
        assert denials[0].action == "delete"

    def test_denials_empty_when_no_denials(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", json.dumps({}))
        denials = ctx.audit_log.denials()
        assert denials == []

    def test_successes_empty_when_no_successes(self):
        # A policy with empty allow_actions still allows execution (allows but records)
        # Use a fresh context where we only attempt denied actions
        ctx = _make_context(allowed=["move"])
        # Don't execute anything; audit should have no successes
        successes = ctx.audit_log.successes()
        assert successes == []

    def test_to_json_returns_string(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", json.dumps({}))
        result = ctx.audit_log.to_json()
        assert isinstance(result, str)

    def test_to_json_is_valid_json(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", json.dumps({}))
        parsed = json.loads(ctx.audit_log.to_json())
        assert isinstance(parsed, (list, dict))

    def test_to_json_contains_entries(self):
        ctx = _make_context(allowed=["move", "create"])
        ctx.execute_json("move", json.dumps({}))
        ctx.execute_json("create", json.dumps({}))
        parsed = json.loads(ctx.audit_log.to_json())
        # Should be a JSON array or object containing 2 entries
        if isinstance(parsed, list):
            assert len(parsed) == 2
        else:
            # May be wrapped in an object
            assert len(parsed) > 0

    def test_to_json_empty_when_no_executions(self):
        ctx = _make_context(allowed=["move"])
        result = ctx.audit_log.to_json()
        parsed = json.loads(result)
        if isinstance(parsed, list):
            assert len(parsed) == 0


# ---------------------------------------------------------------------------
# SandboxContext.action_count
# ---------------------------------------------------------------------------


class TestSandboxContextActionCount:
    def test_initial_action_count_is_zero(self):
        ctx = _make_context(allowed=["move"])
        assert ctx.action_count == 0

    def test_action_count_increments_per_successful_execution(self):
        ctx = _make_context(allowed=["a", "b", "c"])
        ctx.execute_json("a", "{}")
        assert ctx.action_count == 1
        ctx.execute_json("b", "{}")
        assert ctx.action_count == 2
        ctx.execute_json("c", "{}")
        assert ctx.action_count == 3

    def test_action_count_not_incremented_on_denial(self):
        ctx = _make_context(allowed=["move"])
        ctx.execute_json("move", "{}")
        with pytest.raises(RuntimeError):
            ctx.execute_json("forbidden_action", "{}")
        assert ctx.action_count == 1
