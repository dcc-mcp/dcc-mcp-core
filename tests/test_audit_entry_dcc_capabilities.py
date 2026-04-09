"""Tests for AuditEntry deep field inspection, AuditLog.to_json, DccCapabilities, ScriptLanguage, and paths.

These tests focus on fields/APIs that are present in the public bindings but
were not exercised by the existing test_sandbox.py or test_adapters_python.py
test files.
"""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_ctx(
    allow: list[str] | None = None, deny: list[str] | None = None, actor: str = "agent"
) -> dcc_mcp_core.SandboxContext:
    """Build a SandboxContext with optional allow/deny lists and an actor."""
    policy = dcc_mcp_core.SandboxPolicy()
    if allow is not None:
        policy.allow_actions(allow)
    if deny is not None:
        policy.deny_actions(deny)
    ctx = dcc_mcp_core.SandboxContext(policy)
    ctx.set_actor(actor)
    return ctx


# ---------------------------------------------------------------------------
# AuditEntry — deep field inspection
# ---------------------------------------------------------------------------


class TestAuditEntryFields:
    """Verify every public field of AuditEntry carries the expected value."""

    def test_actor_reflects_set_actor(self) -> None:
        """AuditEntry.actor should match the actor set via set_actor()."""
        ctx = _make_ctx(actor="studio-pipeline-bot")
        ctx.execute_json("list_objects", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "studio-pipeline-bot"

    def test_actor_default_when_not_set(self) -> None:
        """Without set_actor(), actor may be None or an empty string."""
        policy = dcc_mcp_core.SandboxPolicy()
        ctx = dcc_mcp_core.SandboxContext(policy)
        ctx.execute_json("list_objects", "{}")
        entry = ctx.audit_log.entries()[0]
        # actor is None when no actor has been set
        assert entry.actor is None or isinstance(entry.actor, str)

    def test_params_json_roundtrip(self) -> None:
        """AuditEntry.params_json should be the JSON string passed to execute_json."""
        ctx = _make_ctx()
        payload = json.dumps({"radius": 2.5, "name": "sphere_01"})
        ctx.execute_json("create_sphere", payload)
        entry = ctx.audit_log.entries()[0]
        assert entry.params_json is not None
        parsed = json.loads(entry.params_json)
        assert parsed["radius"] == 2.5
        assert parsed["name"] == "sphere_01"

    def test_params_json_empty_object(self) -> None:
        """execute_json('{}') stores '{}' (or equivalent) in params_json."""
        ctx = _make_ctx()
        ctx.execute_json("noop", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.params_json is not None
        parsed = json.loads(entry.params_json)
        assert parsed == {}

    def test_outcome_detail_is_none_on_success(self) -> None:
        """Successful executions should have outcome_detail == None."""
        ctx = _make_ctx()
        ctx.execute_json("safe_action", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.outcome == "success"
        assert entry.outcome_detail is None

    def test_outcome_detail_set_on_denial(self) -> None:
        """Denied executions should set outcome_detail to a non-empty string."""
        ctx = _make_ctx(deny=["dangerous"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("dangerous", "{}")
        denials = ctx.audit_log.denials()
        assert len(denials) == 1
        denial = denials[0]
        assert denial.outcome == "denied"
        assert denial.outcome_detail is not None
        assert len(denial.outcome_detail) > 0

    def test_outcome_detail_mentions_action_name(self) -> None:
        """The denial detail message should mention the denied action name."""
        ctx = _make_ctx(deny=["rm_rf_slash"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("rm_rf_slash", "{}")
        denial = ctx.audit_log.denials()[0]
        assert "rm_rf_slash" in denial.outcome_detail

    def test_action_field_matches_call(self) -> None:
        """AuditEntry.action should exactly match the action name used."""
        ctx = _make_ctx()
        ctx.execute_json("render_frame_42", "{}")
        entry = ctx.audit_log.entries()[0]
        assert entry.action == "render_frame_42"

    def test_duration_ms_is_int_or_float(self) -> None:
        """duration_ms should be a non-negative number."""
        ctx = _make_ctx()
        ctx.execute_json("act", "{}")
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.duration_ms, (int, float))
        assert entry.duration_ms >= 0

    def test_timestamp_ms_monotonically_increases(self) -> None:
        """Consecutive executions should have non-decreasing timestamp_ms."""
        ctx = _make_ctx()
        for _ in range(5):
            ctx.execute_json("act", "{}")
        entries = ctx.audit_log.entries()
        timestamps = [e.timestamp_ms for e in entries]
        for i in range(1, len(timestamps)):
            assert timestamps[i] >= timestamps[i - 1], "Timestamps should be non-decreasing"

    def test_repr_contains_outcome(self) -> None:
        """repr() of an AuditEntry should contain the outcome string."""
        ctx = _make_ctx()
        ctx.execute_json("act", "{}")
        entry = ctx.audit_log.entries()[0]
        assert "success" in repr(entry)

    def test_repr_contains_action(self) -> None:
        """repr() of an AuditEntry should contain the action name."""
        ctx = _make_ctx()
        ctx.execute_json("unique_action_xyz", "{}")
        entry = ctx.audit_log.entries()[0]
        assert "unique_action_xyz" in repr(entry)


# ---------------------------------------------------------------------------
# AuditLog — to_json deep validation
# ---------------------------------------------------------------------------


class TestAuditLogToJson:
    def test_to_json_contains_actor(self) -> None:
        """to_json() output should include the actor field."""
        ctx = _make_ctx(actor="test-agent")
        ctx.execute_json("act", "{}")
        records = json.loads(ctx.audit_log.to_json())
        assert records[0]["actor"] == "test-agent"

    def test_to_json_contains_action_name(self) -> None:
        """to_json() output should include the action name field."""
        ctx = _make_ctx()
        ctx.execute_json("export_fbx", "{}")
        records = json.loads(ctx.audit_log.to_json())
        assert records[0]["action"] == "export_fbx"

    def test_to_json_contains_outcome(self) -> None:
        """to_json() output should include the outcome field."""
        ctx = _make_ctx()
        ctx.execute_json("act", "{}")
        records = json.loads(ctx.audit_log.to_json())
        assert records[0]["outcome"] == "success"

    def test_to_json_contains_timestamp_ms(self) -> None:
        """to_json() output should include a positive timestamp_ms."""
        ctx = _make_ctx()
        ctx.execute_json("act", "{}")
        records = json.loads(ctx.audit_log.to_json())
        assert records[0]["timestamp_ms"] > 0

    def test_to_json_contains_params_json(self) -> None:
        """to_json() output should include params_json."""
        ctx = _make_ctx()
        ctx.execute_json("act", json.dumps({"key": "val"}))
        records = json.loads(ctx.audit_log.to_json())
        # params_json field should be present and contain the key
        params_raw = records[0].get("params_json", "{}")
        assert "key" in params_raw

    def test_to_json_empty_log_returns_empty_array(self) -> None:
        """to_json() on an empty log returns an empty JSON array."""
        policy = dcc_mcp_core.SandboxPolicy()
        ctx = dcc_mcp_core.SandboxContext(policy)
        records = json.loads(ctx.audit_log.to_json())
        assert records == []

    def test_to_json_multiple_entries(self) -> None:
        """to_json() with 5 executions should return an array of length 5."""
        ctx = _make_ctx()
        for i in range(5):
            ctx.execute_json(f"act_{i}", "{}")
        records = json.loads(ctx.audit_log.to_json())
        assert len(records) == 5


# ---------------------------------------------------------------------------
# DccCapabilities — field access
# ---------------------------------------------------------------------------


class TestDccCapabilities:
    def test_default_all_false(self) -> None:
        """Default DccCapabilities has all boolean fields False."""
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.file_operations is False
        assert dc.progress_reporting is False
        assert dc.scene_info is False
        assert dc.selection is False
        assert dc.snapshot is False
        assert dc.undo_redo is False

    def test_default_script_languages_empty(self) -> None:
        """Default script_languages is an empty list."""
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.script_languages == []

    def test_default_extensions_empty_dict(self) -> None:
        """Default extensions is an empty dict."""
        dc = dcc_mcp_core.DccCapabilities()
        assert dc.extensions == {}

    def test_set_file_operations(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.file_operations = True
        assert dc.file_operations is True

    def test_set_snapshot(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.snapshot = True
        assert dc.snapshot is True

    def test_set_scene_info(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.scene_info = True
        assert dc.scene_info is True

    def test_set_undo_redo(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.undo_redo = True
        assert dc.undo_redo is True

    def test_set_selection(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.selection = True
        assert dc.selection is True

    def test_set_progress_reporting(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        dc.progress_reporting = True
        assert dc.progress_reporting is True

    def test_repr_contains_class_name(self) -> None:
        dc = dcc_mcp_core.DccCapabilities()
        assert "DccCapabilities" in repr(dc)

    def test_repr_after_enabling_snapshot(self) -> None:
        """repr() should reflect snapshot=true after enabling it."""
        dc = dcc_mcp_core.DccCapabilities()
        dc.snapshot = True
        assert "true" in repr(dc).lower() or "snapshot" in repr(dc).lower()

    def test_set_multiple_fields_independent(self) -> None:
        """Setting one field does not affect others."""
        dc = dcc_mcp_core.DccCapabilities()
        dc.snapshot = True
        dc.file_operations = True
        assert dc.scene_info is False
        assert dc.selection is False


# ---------------------------------------------------------------------------
# ScriptLanguage enum
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    def test_python_variant_exists(self) -> None:
        sl = dcc_mcp_core.ScriptLanguage.PYTHON
        assert sl is not None

    def test_mel_variant_exists(self) -> None:
        sl = dcc_mcp_core.ScriptLanguage.MEL
        assert sl is not None

    def test_values_are_distinct(self) -> None:
        assert dcc_mcp_core.ScriptLanguage.PYTHON != dcc_mcp_core.ScriptLanguage.MEL

    def test_dcc_capabilities_accepts_script_language(self) -> None:
        """DccCapabilities.script_languages can be set using ScriptLanguage enum values."""
        dc = dcc_mcp_core.DccCapabilities()
        dc.script_languages = [dcc_mcp_core.ScriptLanguage.PYTHON]
        assert len(dc.script_languages) == 1

    def test_repr_is_string(self) -> None:
        sl = dcc_mcp_core.ScriptLanguage.PYTHON
        assert isinstance(repr(sl), str)


# ---------------------------------------------------------------------------
# SandboxPolicy — path restriction depth tests
# ---------------------------------------------------------------------------


class TestSandboxPolicyPaths:
    def test_allow_single_path(self, tmp_path) -> None:
        """A file inside an allowed directory should be permitted."""
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_paths([str(tmp_path)])
        ctx = dcc_mcp_core.SandboxContext(policy)
        child = str(tmp_path / "scene.mb")
        assert ctx.is_path_allowed(child) is True

    def test_disallow_path_outside_allowlist(self, tmp_path) -> None:
        """A path outside all allowed directories must be denied."""
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_paths([str(tmp_path)])
        ctx = dcc_mcp_core.SandboxContext(policy)
        assert ctx.is_path_allowed("/etc/shadow") is False

    def test_allow_multiple_paths(self, tmp_path) -> None:
        """All paths in the allowlist should be permitted."""
        dir_a = tmp_path / "a"
        dir_b = tmp_path / "b"
        dir_a.mkdir()
        dir_b.mkdir()
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_paths([str(dir_a), str(dir_b)])
        ctx = dcc_mcp_core.SandboxContext(policy)
        assert ctx.is_path_allowed(str(dir_a / "scene.mb")) is True
        assert ctx.is_path_allowed(str(dir_b / "assets.fbx")) is True
        assert ctx.is_path_allowed("/root/secret") is False

    def test_no_path_restriction_allows_all(self) -> None:
        """Without path restrictions, any path should be allowed."""
        policy = dcc_mcp_core.SandboxPolicy()
        ctx = dcc_mcp_core.SandboxContext(policy)
        assert ctx.is_path_allowed("/etc/passwd") is True
        assert ctx.is_path_allowed("/root") is True

    def test_empty_path_allowed_string(self, tmp_path) -> None:
        """is_path_allowed('') should return a bool without crashing."""
        policy = dcc_mcp_core.SandboxPolicy()
        policy.allow_paths([str(tmp_path)])
        ctx = dcc_mcp_core.SandboxContext(policy)
        result = ctx.is_path_allowed("")
        assert isinstance(result, bool)
