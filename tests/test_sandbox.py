"""Tests for dcc-mcp-sandbox Python bindings.

Covers SandboxPolicy, SandboxContext, AuditLog, AuditEntry, InputValidator.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── SandboxPolicy ─────────────────────────────────────────────────────────────


class TestSandboxPolicy:
    def test_default_policy_not_read_only(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        assert policy.is_read_only is False

    def test_set_read_only(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.set_read_only(True)
        assert policy.is_read_only is True

    def test_set_read_only_false(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.set_read_only(True)
        policy.set_read_only(False)
        assert policy.is_read_only is False

    def test_set_timeout_ms(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.set_timeout_ms(5000)
        # No getter, but should not raise
        assert policy is not None

    def test_set_max_actions(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.set_max_actions(100)
        assert policy is not None

    def test_allow_actions(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_actions(["get_scene_info", "list_objects"])
        assert policy is not None

    def test_deny_actions(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.deny_actions(["delete_scene", "nuke_project"])
        assert policy is not None

    def test_allow_paths(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_paths(["/tmp/project", "/home/user/scenes"])
        assert policy is not None

    def test_repr_contains_policy_info(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        r = repr(policy)
        assert "SandboxPolicy" in r


# ── SandboxContext ────────────────────────────────────────────────────────────


class TestSandboxContext:
    def _make_open_context(self) -> dcc_mcp_core.SandboxContext:
        """Return a context with no action restrictions."""
        policy = dcc_mcp_core.SandboxPolicy()
        return dcc_mcp_core.SandboxContext(policy)

    def _make_restricted_context(self, allowed: list[str]) -> dcc_mcp_core.SandboxContext:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_actions(allowed)
        return dcc_mcp_core.SandboxContext(policy)

    def test_initial_action_count(self) -> None:
        ctx = self._make_open_context()
        assert ctx.action_count == 0

    def test_execute_json_open_policy(self) -> None:
        ctx = self._make_open_context()
        result_json = ctx.execute_json("any_action", "{}")
        assert result_json is not None

    def test_execute_json_increments_action_count(self) -> None:
        ctx = self._make_open_context()
        ctx.execute_json("action1", "{}")
        ctx.execute_json("action2", "{}")
        assert ctx.action_count == 2

    def test_execute_json_denied_action_raises(self) -> None:
        ctx = self._make_restricted_context(["allowed_action"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("forbidden_action", "{}")

    def test_execute_json_allowed_action_succeeds(self) -> None:
        ctx = self._make_restricted_context(["allowed_action"])
        result = ctx.execute_json("allowed_action", "{}")
        assert result is not None

    def test_execute_json_invalid_json_raises(self) -> None:
        ctx = self._make_open_context()
        with pytest.raises(RuntimeError):
            ctx.execute_json("act", "{ invalid json }")

    def test_is_allowed_open_policy(self) -> None:
        ctx = self._make_open_context()
        assert ctx.is_allowed("any_action") is True

    def test_is_allowed_restricted_policy(self) -> None:
        ctx = self._make_restricted_context(["read_only"])
        assert ctx.is_allowed("read_only") is True
        assert ctx.is_allowed("write_data") is False

    def test_is_path_allowed_no_restriction(self) -> None:
        ctx = self._make_open_context()
        # No path restrictions → all paths allowed
        assert ctx.is_path_allowed("/any/path") is True

    def test_is_path_allowed_with_restriction(self) -> None:
        from pathlib import Path
        import tempfile

        with tempfile.TemporaryDirectory() as tmpdir:
            policy = dcc_mcp_core.SandboxPolicy()
            policy.allow_paths([tmpdir])
            ctx = dcc_mcp_core.SandboxContext(policy)
            allowed_path = str(Path(tmpdir) / "scene.mb")
            assert ctx.is_path_allowed(allowed_path) is True
            assert ctx.is_path_allowed("/etc/passwd") is False

    def test_audit_log_initial_empty(self) -> None:
        ctx = self._make_open_context()
        assert len(ctx.audit_log) == 0

    def test_audit_log_records_after_execute(self) -> None:
        ctx = self._make_open_context()
        ctx.execute_json("scene_info", "{}")
        log = ctx.audit_log
        assert len(log) >= 1

    def test_set_actor_does_not_raise(self) -> None:
        ctx = self._make_open_context()
        ctx.set_actor("my-agent")
        assert ctx is not None

    def test_repr_contains_action_count(self) -> None:
        ctx = self._make_open_context()
        r = repr(ctx)
        assert "SandboxContext" in r

    def test_read_only_policy_allows_read_actions(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.set_read_only(True)
        ctx = dcc_mcp_core.SandboxContext(policy)
        # In a read-only context, execution is allowed when no action allowlist set
        result = ctx.execute_json("get_scene_info", "{}")
        assert result is not None


# ── AuditLog and AuditEntry ───────────────────────────────────────────────────


class TestAuditLog:
    def _execute_n(self, ctx: dcc_mcp_core.SandboxContext, n: int, action: str = "act") -> None:
        for _ in range(n):
            ctx.execute_json(action, "{}")

    def _open_ctx(self) -> dcc_mcp_core.SandboxContext:
        return dcc_mcp_core.SandboxContext(dcc_mcp_core.SandboxPolicy())

    def test_len_matches_executions(self) -> None:
        ctx = self._open_ctx()
        self._execute_n(ctx, 3)
        assert len(ctx.audit_log) == 3

    def test_entries_returns_list(self) -> None:
        ctx = self._open_ctx()
        self._execute_n(ctx, 2)
        entries = ctx.audit_log.entries()
        assert len(entries) == 2

    def test_entry_action_name(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("render_scene", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "render_scene"

    def test_entry_outcome_success(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("render_scene", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.outcome == "success"

    def test_entry_timestamp_ms_positive(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("act", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.timestamp_ms > 0

    def test_entry_duration_ms_nonnegative(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("act", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.duration_ms >= 0

    def test_entry_repr_contains_action(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("my_action", "{}")
        entry = ctx.audit_log.entries()[0]
        assert "my_action" in repr(entry)

    def test_entries_for_action_filters_correctly(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("action_a", "{}")
        ctx.execute_json("action_b", "{}")
        ctx.execute_json("action_a", "{}")
        filtered = ctx.audit_log.entries_for_action("action_a")
        assert len(filtered) == 2
        assert all(e.action == "action_a" for e in filtered)

    def test_successes_filter(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("ok_action", "{}")
        successes = ctx.audit_log.successes()
        assert len(successes) >= 1
        assert all(e.outcome == "success" for e in successes)

    def test_to_json_is_valid_json(self) -> None:
        ctx = self._open_ctx()
        ctx.execute_json("act", "{}")
        json_str = ctx.audit_log.to_json()
        parsed = json.loads(json_str)
        assert isinstance(parsed, list)

    def test_repr_contains_len(self) -> None:
        ctx = self._open_ctx()
        self._execute_n(ctx, 2)
        r = repr(ctx.audit_log)
        assert "2" in r

    def test_denied_entry_recorded(self) -> None:
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_actions(["safe"])
        policy.deny_actions(["dangerous"])
        ctx = dcc_mcp_core.SandboxContext(policy)
        with pytest.raises(RuntimeError):
            ctx.execute_json("dangerous", "{}")
        # After a denied attempt the log should contain a denial entry
        denials = ctx.audit_log.denials()
        assert len(denials) >= 1
        assert all(e.outcome == "denied" for e in denials)


# ── InputValidator ────────────────────────────────────────────────────────────


class TestInputValidator:
    def test_empty_validator_accepts_empty_object(self) -> None:
        v = dcc_mcp_core.InputValidator()
        ok, err = v.validate("{}")
        assert ok is True
        assert err is None

    def test_require_string_accepts_valid(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_string("name", max_length=100, min_length=None)
        ok, _err = v.validate('{"name": "sphere"}')
        assert ok is True

    def test_require_string_rejects_missing(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_string("name", max_length=None, min_length=None)
        ok, err = v.validate("{}")
        assert ok is False
        assert err is not None

    def test_require_string_rejects_too_long(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_string("name", max_length=5, min_length=None)
        ok, _err = v.validate('{"name": "this_is_too_long"}')
        assert ok is False

    def test_require_number_accepts_valid(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_number("count", min_value=0.0, max_value=1000.0)
        ok, _err = v.validate('{"count": 42}')
        assert ok is True

    def test_require_number_rejects_too_small(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_number("count", min_value=0.0, max_value=None)
        ok, _err = v.validate('{"count": -1}')
        assert ok is False

    def test_require_number_rejects_too_large(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_number("count", min_value=None, max_value=10.0)
        ok, _err = v.validate('{"count": 100}')
        assert ok is False

    def test_forbid_substrings_rejects_injection(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.forbid_substrings("script", ["__import__", "os.system", "eval("])
        ok, _err = v.validate('{"script": "eval(bad code)"}')
        assert ok is False

    def test_forbid_substrings_accepts_clean(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.forbid_substrings("script", ["__import__"])
        ok, _err = v.validate('{"script": "print(42)"}')
        assert ok is True

    def test_validate_invalid_json_raises(self) -> None:
        v = dcc_mcp_core.InputValidator()
        with pytest.raises(RuntimeError):
            v.validate("{ not valid json }")

    def test_repr_is_string(self) -> None:
        v = dcc_mcp_core.InputValidator()
        assert isinstance(repr(v), str)

    def test_multiple_fields(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_string("name", max_length=50, min_length=None)
        v.require_number("size", min_value=0.0, max_value=None)
        ok, _ = v.validate('{"name": "cube", "size": 1.5}')
        assert ok is True

    def test_multiple_fields_partial_miss(self) -> None:
        v = dcc_mcp_core.InputValidator()
        v.require_string("name", max_length=50, min_length=None)
        v.require_number("size", min_value=0.0, max_value=None)
        ok, err = v.validate('{"name": "cube"}')
        # 'size' is missing
        assert ok is False
        assert err is not None
