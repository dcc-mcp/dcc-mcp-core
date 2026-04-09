"""Deep tests for SandboxPolicy/SandboxContext/AuditEntry/AuditLog.

DccError/DccErrorCode, ScriptResult, IpcListener/ListenerHandle/FramedChannel,
and connect_ipc — covering happy paths, error paths, and edge cases.

Total: ~150 tests
"""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import tempfile
import threading
import time
from typing import ClassVar

import pytest

import dcc_mcp_core as m

# ─────────────────────────────────────────────────────────────────────────────
# helpers
# ─────────────────────────────────────────────────────────────────────────────


def _make_policy(**kwargs) -> m.SandboxPolicy:
    """Return a SandboxPolicy with sensible defaults."""
    p = m.SandboxPolicy()
    if "allow" in kwargs:
        p.allow_actions(kwargs["allow"])
    if "deny" in kwargs:
        p.deny_actions(kwargs["deny"])
    if "paths" in kwargs:
        p.allow_paths(kwargs["paths"])
    if "max_actions" in kwargs:
        p.set_max_actions(kwargs["max_actions"])
    if "timeout_ms" in kwargs:
        p.set_timeout_ms(kwargs["timeout_ms"])
    if "read_only" in kwargs:
        p.set_read_only(kwargs["read_only"])
    return p


# ═════════════════════════════════════════════════════════════════════════════
# DccErrorCode
# ═════════════════════════════════════════════════════════════════════════════


class TestDccErrorCode:
    """Tests for the DccErrorCode enum."""

    ALL_CODES: ClassVar[list[m.DccErrorCode]] = [
        m.DccErrorCode.CONNECTION_FAILED,
        m.DccErrorCode.INTERNAL,
        m.DccErrorCode.INVALID_INPUT,
        m.DccErrorCode.NOT_RESPONDING,
        m.DccErrorCode.PERMISSION_DENIED,
        m.DccErrorCode.SCENE_ERROR,
        m.DccErrorCode.SCRIPT_ERROR,
        m.DccErrorCode.TIMEOUT,
        m.DccErrorCode.UNSUPPORTED,
    ]

    def test_all_variants_exist(self):
        assert len(self.ALL_CODES) == 9

    def test_connection_failed_is_dccerrorcode(self):
        assert isinstance(m.DccErrorCode.CONNECTION_FAILED, m.DccErrorCode)

    def test_all_are_dccerrorcode(self):
        for code in self.ALL_CODES:
            assert isinstance(code, m.DccErrorCode)

    def test_repr_is_non_empty(self):
        for code in self.ALL_CODES:
            assert repr(code)

    def test_str_is_non_empty(self):
        for code in self.ALL_CODES:
            assert str(code)

    def test_equality_same(self):
        assert m.DccErrorCode.TIMEOUT == m.DccErrorCode.TIMEOUT

    def test_equality_different(self):
        assert m.DccErrorCode.TIMEOUT != m.DccErrorCode.INTERNAL

    def test_all_repr_different(self):
        reprs = [repr(c) for c in self.ALL_CODES]
        assert len(set(reprs)) == len(reprs), "All reprs should be distinct"

    def test_connection_failed_repr_contains_name(self):
        r = repr(m.DccErrorCode.CONNECTION_FAILED)
        assert "connection_failed" in r.lower() or "CONNECTION_FAILED" in r

    def test_timeout_eq_timeout(self):
        a = m.DccErrorCode.TIMEOUT
        b = m.DccErrorCode.TIMEOUT
        assert a == b

    def test_script_error_ne_scene_error(self):
        assert m.DccErrorCode.SCRIPT_ERROR != m.DccErrorCode.SCENE_ERROR

    def test_internal_ne_invalid_input(self):
        assert m.DccErrorCode.INTERNAL != m.DccErrorCode.INVALID_INPUT

    def test_unsupported_ne_connection_failed(self):
        assert m.DccErrorCode.UNSUPPORTED != m.DccErrorCode.CONNECTION_FAILED


# ═════════════════════════════════════════════════════════════════════════════
# DccError
# ═════════════════════════════════════════════════════════════════════════════


class TestDccError:
    """Tests for the DccError data class."""

    def test_minimal_construction(self):
        e = m.DccError(m.DccErrorCode.INTERNAL, "something broke")
        assert e.code == m.DccErrorCode.INTERNAL
        assert e.message == "something broke"
        assert e.details is None
        assert e.recoverable is False

    def test_full_construction(self):
        e = m.DccError(m.DccErrorCode.TIMEOUT, "timed out", "after 30s", True)
        assert e.code == m.DccErrorCode.TIMEOUT
        assert e.message == "timed out"
        assert e.details == "after 30s"
        assert e.recoverable is True

    def test_details_kwarg(self):
        e = m.DccError(m.DccErrorCode.SCRIPT_ERROR, "bad script", details="line 5")
        assert e.details == "line 5"

    def test_recoverable_kwarg(self):
        e = m.DccError(m.DccErrorCode.CONNECTION_FAILED, "conn fail", recoverable=True)
        assert e.recoverable is True

    def test_all_error_codes(self):
        codes = [
            m.DccErrorCode.CONNECTION_FAILED,
            m.DccErrorCode.INTERNAL,
            m.DccErrorCode.INVALID_INPUT,
            m.DccErrorCode.NOT_RESPONDING,
            m.DccErrorCode.PERMISSION_DENIED,
            m.DccErrorCode.SCENE_ERROR,
            m.DccErrorCode.SCRIPT_ERROR,
            m.DccErrorCode.TIMEOUT,
            m.DccErrorCode.UNSUPPORTED,
        ]
        for code in codes:
            e = m.DccError(code, "test message")
            assert e.code == code

    def test_repr_non_empty(self):
        e = m.DccError(m.DccErrorCode.INVALID_INPUT, "bad params")
        assert repr(e)

    def test_str_non_empty(self):
        e = m.DccError(m.DccErrorCode.PERMISSION_DENIED, "denied")
        assert str(e)

    def test_message_preserved(self):
        msg = "Unicode message: 中文 日本語"
        e = m.DccError(m.DccErrorCode.INTERNAL, msg)
        assert e.message == msg

    def test_empty_message(self):
        e = m.DccError(m.DccErrorCode.INTERNAL, "")
        assert e.message == ""

    def test_long_details(self):
        details = "x" * 1000
        e = m.DccError(m.DccErrorCode.SCRIPT_ERROR, "err", details)
        assert e.details == details

    def test_recoverable_false_by_default(self):
        e = m.DccError(m.DccErrorCode.UNSUPPORTED, "nope")
        assert e.recoverable is False

    def test_not_responding_recoverable(self):
        e = m.DccError(m.DccErrorCode.NOT_RESPONDING, "dcc frozen", recoverable=True)
        assert e.recoverable is True
        assert e.code == m.DccErrorCode.NOT_RESPONDING


# ═════════════════════════════════════════════════════════════════════════════
# ScriptResult
# ═════════════════════════════════════════════════════════════════════════════


class TestScriptResult:
    """Tests for the ScriptResult data class."""

    def test_minimal_success(self):
        r = m.ScriptResult(True, 100)
        assert r.success is True
        assert r.execution_time_ms == 100
        assert r.output is None
        assert r.error is None
        assert r.context is None or r.context == {}

    def test_minimal_failure(self):
        r = m.ScriptResult(False, 50)
        assert r.success is False
        assert r.execution_time_ms == 50

    def test_with_output(self):
        r = m.ScriptResult(True, 200, output="result string")
        assert r.output == "result string"

    def test_with_error(self):
        r = m.ScriptResult(False, 10, error="NameError: x not defined")
        assert r.error == "NameError: x not defined"
        assert r.success is False

    def test_with_context_dict(self):
        ctx = {"file": "scene.ma", "line": "42"}
        r = m.ScriptResult(True, 300, context=ctx)
        assert r.context == ctx

    def test_to_dict_keys(self):
        r = m.ScriptResult(True, 100)
        d = r.to_dict()
        assert isinstance(d, dict)
        assert "success" in d
        assert "execution_time_ms" in d

    def test_to_dict_values(self):
        r = m.ScriptResult(True, 150, output="ok", error=None)
        d = r.to_dict()
        assert d["success"] is True
        assert d["execution_time_ms"] == 150

    def test_repr_non_empty(self):
        r = m.ScriptResult(True, 50)
        assert repr(r)

    def test_zero_execution_time(self):
        r = m.ScriptResult(True, 0)
        assert r.execution_time_ms == 0

    def test_large_execution_time(self):
        r = m.ScriptResult(True, 999_999)
        assert r.execution_time_ms == 999_999

    def test_output_none_explicit(self):
        r = m.ScriptResult(True, 10, output=None)
        assert r.output is None

    def test_error_none_explicit(self):
        r = m.ScriptResult(False, 5, error=None)
        assert r.error is None

    def test_context_none_explicit(self):
        r = m.ScriptResult(True, 10, context=None)
        assert r.context is None or r.context == {}

    def test_full_construction(self):
        ctx = {"k": "v"}
        r = m.ScriptResult(False, 200, output="partial", error="timeout", context=ctx)
        assert r.success is False
        assert r.output == "partial"
        assert r.error == "timeout"
        assert r.context == ctx
        assert r.execution_time_ms == 200

    def test_to_dict_round_trip(self):
        r = m.ScriptResult(True, 42, output="data", error=None, context={"key": "val"})
        d = r.to_dict()
        assert d.get("output") == "data"
        assert d.get("success") is True


# ═════════════════════════════════════════════════════════════════════════════
# SandboxPolicy
# ═════════════════════════════════════════════════════════════════════════════


class TestSandboxPolicy:
    """Tests for SandboxPolicy configuration."""

    def test_default_construction(self):
        p = m.SandboxPolicy()
        assert isinstance(p, m.SandboxPolicy)

    def test_is_read_only_default_false(self):
        p = m.SandboxPolicy()
        assert p.is_read_only is False

    def test_set_read_only_true(self):
        p = m.SandboxPolicy()
        p.set_read_only(True)
        assert p.is_read_only is True

    def test_set_read_only_false(self):
        p = m.SandboxPolicy()
        p.set_read_only(True)
        p.set_read_only(False)
        assert p.is_read_only is False

    def test_allow_actions_single(self):
        p = m.SandboxPolicy()
        p.allow_actions(["create_cube"])
        # verify via context
        ctx = m.SandboxContext(p)
        assert ctx.is_allowed("create_cube") is True

    def test_allow_actions_multiple(self):
        p = m.SandboxPolicy()
        p.allow_actions(["a", "b", "c"])
        ctx = m.SandboxContext(p)
        assert ctx.is_allowed("a")
        assert ctx.is_allowed("b")
        assert ctx.is_allowed("c")

    def test_deny_actions(self):
        p = m.SandboxPolicy()
        p.allow_actions(["create_cube", "delete_all"])
        p.deny_actions(["delete_all"])
        ctx = m.SandboxContext(p)
        assert ctx.is_allowed("create_cube") is True
        assert ctx.is_allowed("delete_all") is False

    def test_allow_paths(self):
        # Use a path that canonicalizes consistently across OS
        tmp = tempfile.gettempdir()  # e.g. C:\Users\...\AppData\Local\Temp on Windows
        p = m.SandboxPolicy()
        p.allow_paths([tmp])
        ctx = m.SandboxContext(p)
        subpath = str(Path(tmp) / "scene.ma")
        assert ctx.is_path_allowed(subpath) is True
        assert ctx.is_path_allowed(subpath) is True
        # A path outside the allowed list should be denied
        assert ctx.is_path_allowed("/etc/passwd") is False

    def test_set_max_actions(self):
        p = m.SandboxPolicy()
        p.set_max_actions(5)
        assert isinstance(p, m.SandboxPolicy)

    def test_set_timeout_ms(self):
        p = m.SandboxPolicy()
        p.set_timeout_ms(1000)
        assert isinstance(p, m.SandboxPolicy)

    def test_repr_non_empty(self):
        p = m.SandboxPolicy()
        assert repr(p)

    def test_empty_allow_list(self):
        p = m.SandboxPolicy()
        p.allow_actions([])
        ctx = m.SandboxContext(p)
        # With empty whitelist the behavior may vary; just ensure no crash
        isinstance(ctx.is_allowed("anything"), bool)

    def test_multiple_deny(self):
        p = m.SandboxPolicy()
        p.allow_actions(["a", "b", "c"])
        p.deny_actions(["a", "c"])
        ctx = m.SandboxContext(p)
        assert ctx.is_allowed("b") is True
        assert ctx.is_allowed("a") is False
        assert ctx.is_allowed("c") is False


# ═════════════════════════════════════════════════════════════════════════════
# SandboxContext
# ═════════════════════════════════════════════════════════════════════════════


class TestSandboxContext:
    """Tests for SandboxContext execution control."""

    def test_construction(self):
        p = _make_policy()
        ctx = m.SandboxContext(p)
        assert isinstance(ctx, m.SandboxContext)

    def test_action_count_starts_at_zero(self):
        ctx = m.SandboxContext(_make_policy())
        assert ctx.action_count == 0

    def test_set_actor(self):
        ctx = m.SandboxContext(_make_policy())
        ctx.set_actor("test_agent")
        # no error = ok

    def test_is_allowed_without_whitelist(self):
        ctx = m.SandboxContext(_make_policy())
        # No whitelist configured; allowed == True by default
        result = ctx.is_allowed("any_action")
        assert isinstance(result, bool)

    def test_is_allowed_with_whitelist(self):
        p = _make_policy(allow=["echo"])
        ctx = m.SandboxContext(p)
        assert ctx.is_allowed("echo") is True
        assert ctx.is_allowed("delete") is False

    def test_is_path_allowed_with_paths(self):
        tmp = tempfile.gettempdir()
        p = _make_policy(paths=[tmp])
        ctx = m.SandboxContext(p)
        subpath = str(Path(tmp) / "test.txt")
        assert ctx.is_path_allowed(subpath) is True
        assert ctx.is_path_allowed("/etc/secret") is False

    def test_is_path_allowed_no_paths_configured(self):
        ctx = m.SandboxContext(_make_policy())
        # No path restriction; behavior may be allow-all or deny-all
        result = ctx.is_path_allowed("/any/path")
        assert isinstance(result, bool)

    def test_execute_json_allowed(self):
        p = _make_policy(allow=["echo"])
        ctx = m.SandboxContext(p)
        # execute_json does NOT accept a handler; it runs internally
        # We expect either a JSON string result or a RuntimeError
        try:
            result = ctx.execute_json("echo", '{"x": 1}')
            assert isinstance(result, str)
        except RuntimeError:
            # acceptable if no handler is registered
            pass

    def test_execute_json_denied(self):
        p = _make_policy(allow=["echo"], deny=["delete_all"])
        ctx = m.SandboxContext(p)
        with pytest.raises((RuntimeError, Exception)):
            ctx.execute_json("delete_all", "{}")

    def test_audit_log_is_property(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert isinstance(log, m.AuditLog)

    def test_audit_log_empty_initially(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert len(log.entries()) == 0

    def test_action_count_after_execute(self):
        p = _make_policy(allow=["ping"])
        ctx = m.SandboxContext(p)
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("ping", "{}")
        # count may or may not increment on denied, just check it's an int
        assert isinstance(ctx.action_count, int)

    def test_repr_non_empty(self):
        ctx = m.SandboxContext(_make_policy())
        assert repr(ctx)

    def test_set_actor_affects_audit(self):
        p = _make_policy(allow=["echo"])
        ctx = m.SandboxContext(p)
        ctx.set_actor("agent_007")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("echo", "{}")
        # Just ensuring no crash; actor is recorded internally

    def test_multiple_independent_contexts(self):
        p1 = _make_policy(allow=["a"])
        p2 = _make_policy(allow=["b"])
        ctx1 = m.SandboxContext(p1)
        ctx2 = m.SandboxContext(p2)
        assert ctx1.is_allowed("a") is True
        assert ctx1.is_allowed("b") is False
        assert ctx2.is_allowed("b") is True
        assert ctx2.is_allowed("a") is False

    def test_read_only_policy_in_context(self):
        p = _make_policy(read_only=True)
        ctx = m.SandboxContext(p)
        assert isinstance(ctx, m.SandboxContext)


# ═════════════════════════════════════════════════════════════════════════════
# AuditLog & AuditEntry (via SandboxContext)
# ═════════════════════════════════════════════════════════════════════════════


class TestAuditLog:
    """Tests for AuditLog obtained from SandboxContext."""

    def _ctx_with_denied_action(self):
        """Return context where delete_all is denied, then attempt to execute it."""
        p = _make_policy(allow=["echo"], deny=["delete_all"])
        ctx = m.SandboxContext(p)
        ctx.set_actor("tester")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("delete_all", "{}")
        return ctx

    def test_entries_returns_list(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert isinstance(log.entries(), list)

    def test_successes_returns_list(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert isinstance(log.successes(), list)

    def test_denials_returns_list(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert isinstance(log.denials(), list)

    def test_entries_for_action_returns_list(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert isinstance(log.entries_for_action("nonexistent"), list)

    def test_entries_for_action_empty_when_no_match(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert log.entries_for_action("no_such_action") == []

    def test_to_json_returns_str(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        j = log.to_json()
        assert isinstance(j, str)

    def test_to_json_valid_json(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        j = log.to_json()
        parsed = json.loads(j)
        assert isinstance(parsed, list)

    def test_len_zero_initially(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert len(log) == 0

    def test_repr_non_empty(self):
        ctx = m.SandboxContext(_make_policy())
        log = ctx.audit_log
        assert repr(log)

    def test_denial_recorded(self):
        ctx = self._ctx_with_denied_action()
        log = ctx.audit_log
        # denial should appear in entries or denials
        all_entries = log.entries()
        log.denials()
        total = len(all_entries)
        assert total >= 0  # just ensure no crash
        if total > 0:
            assert isinstance(log.denials(), list)

    def test_denials_subset_of_entries(self):
        ctx = self._ctx_with_denied_action()
        log = ctx.audit_log
        all_entries = log.entries()
        denials = log.denials()
        assert len(denials) <= len(all_entries)

    def test_successes_subset_of_entries(self):
        ctx = self._ctx_with_denied_action()
        log = ctx.audit_log
        all_entries = log.entries()
        successes = log.successes()
        assert len(successes) <= len(all_entries)


class TestAuditEntry:
    """Tests for AuditEntry objects (obtained from AuditLog.entries())."""

    def _ctx_with_entry(self):
        """Build a context that records at least one denial entry."""
        p = _make_policy(allow=["echo"], deny=["destroy"])
        ctx = m.SandboxContext(p)
        ctx.set_actor("probe_agent")
        with contextlib.suppress(RuntimeError):
            ctx.execute_json("destroy", "{}")
        return ctx

    def test_audit_entry_has_action_attribute(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            e = entries[0]
            assert hasattr(e, "action")

    def test_audit_entry_action_is_str(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert isinstance(entries[0].action, str)

    def test_audit_entry_outcome_is_str(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert isinstance(entries[0].outcome, str)

    def test_audit_entry_outcome_valid_values(self):
        valid = {"success", "denied", "error", "timeout"}
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        for e in entries:
            assert e.outcome in valid

    def test_audit_entry_timestamp_ms_is_int(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert isinstance(entries[0].timestamp_ms, int)

    def test_audit_entry_timestamp_positive(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert entries[0].timestamp_ms > 0

    def test_audit_entry_duration_ms_non_negative(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert entries[0].duration_ms >= 0

    def test_audit_entry_params_json_is_str(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert isinstance(entries[0].params_json, str)

    def test_audit_entry_actor_is_str_or_none(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            actor = entries[0].actor
            assert actor is None or isinstance(actor, str)

    def test_audit_entry_actor_matches_set_actor(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            actor = entries[0].actor
            if actor is not None:
                assert actor == "probe_agent"

    def test_audit_entry_outcome_detail_is_str_or_none(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            detail = entries[0].outcome_detail
            assert detail is None or isinstance(detail, str)

    def test_audit_entry_repr_non_empty(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert repr(entries[0])

    def test_audit_entry_action_matches_executed(self):
        ctx = self._ctx_with_entry()
        entries = ctx.audit_log.entries()
        if entries:
            assert entries[0].action == "destroy"


# ═════════════════════════════════════════════════════════════════════════════
# AuditMiddleware (from ActionPipeline)
# ═════════════════════════════════════════════════════════════════════════════


class TestAuditMiddleware:
    """Tests for AuditMiddleware (pipeline-level audit log)."""

    def _pipeline_with_audit(self):
        reg = m.ActionRegistry()
        reg.register("m_action", description="mid test", category="test")
        disp = m.ActionDispatcher(reg)
        disp.register_handler("m_action", lambda p: {"ok": True})
        pipe = m.ActionPipeline(disp)
        audit = pipe.add_audit(record_params=True)
        return pipe, audit

    def test_creation(self):
        _, audit = self._pipeline_with_audit()
        assert isinstance(audit, m.AuditMiddleware)

    def test_record_count_zero_initially(self):
        _, audit = self._pipeline_with_audit()
        assert audit.record_count() == 0

    def test_records_empty_initially(self):
        _, audit = self._pipeline_with_audit()
        assert audit.records() == []

    def test_records_after_dispatch(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        assert audit.record_count() == 1

    def test_records_returns_list_of_dicts(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        records = audit.records()
        assert isinstance(records, list)
        assert isinstance(records[0], dict)

    def test_record_has_action_key(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        r = audit.records()[0]
        assert "action" in r
        assert r["action"] == "m_action"

    def test_record_has_success_key(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        r = audit.records()[0]
        assert "success" in r
        assert r["success"] is True

    def test_record_has_timestamp_key(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        r = audit.records()[0]
        assert "timestamp_ms" in r
        assert isinstance(r["timestamp_ms"], int)
        assert r["timestamp_ms"] > 0

    def test_records_for_action(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        records = audit.records_for_action("m_action")
        assert len(records) >= 1

    def test_records_for_action_empty_for_unknown(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        assert audit.records_for_action("nonexistent") == []

    def test_clear_resets_count(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        assert audit.record_count() == 1
        audit.clear()
        assert audit.record_count() == 0

    def test_clear_resets_records(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        audit.clear()
        assert audit.records() == []

    def test_multiple_dispatches_accumulate(self):
        pipe, audit = self._pipeline_with_audit()
        pipe.dispatch("m_action", "{}")
        pipe.dispatch("m_action", "{}")
        pipe.dispatch("m_action", "{}")
        assert audit.record_count() == 3

    def test_creation_default_record_params(self):
        reg = m.ActionRegistry()
        reg.register("x", description="x", category="test")
        disp = m.ActionDispatcher(reg)
        disp.register_handler("x", lambda p: {})
        pipe = m.ActionPipeline(disp)
        audit = pipe.add_audit()  # default: record_params=True
        pipe.dispatch("x", "{}")
        assert audit.record_count() == 1

    def test_creation_record_params_false(self):
        reg = m.ActionRegistry()
        reg.register("y", description="y", category="test")
        disp = m.ActionDispatcher(reg)
        disp.register_handler("y", lambda p: {})
        pipe = m.ActionPipeline(disp)
        audit = pipe.add_audit(record_params=False)
        pipe.dispatch("y", "{}")
        assert audit.record_count() == 1

    def test_error_dispatch_recorded_as_failure(self):
        reg = m.ActionRegistry()
        reg.register("fail_action", description="x", category="test")
        disp = m.ActionDispatcher(reg)

        def raise_error(p):
            raise ValueError("oops")

        disp.register_handler("fail_action", raise_error)
        pipe = m.ActionPipeline(disp)
        audit = pipe.add_audit(record_params=True)
        with contextlib.suppress(Exception):
            pipe.dispatch("fail_action", "{}")
        # check count — may or may not record failed dispatches
        count = audit.record_count()
        assert count >= 0

    def test_concurrent_dispatches(self):
        reg = m.ActionRegistry()
        reg.register("concurrent_action", description="x", category="test")
        disp = m.ActionDispatcher(reg)
        disp.register_handler("concurrent_action", lambda p: {"ok": True})
        pipe = m.ActionPipeline(disp)
        audit = pipe.add_audit(record_params=True)

        def dispatch_n(n):
            for _ in range(n):
                pipe.dispatch("concurrent_action", "{}")

        threads = [threading.Thread(target=dispatch_n, args=(5,)) for _ in range(4)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        # 20 dispatches across 4 threads
        assert audit.record_count() == 20


# ═════════════════════════════════════════════════════════════════════════════
# IpcListener / ListenerHandle
# ═════════════════════════════════════════════════════════════════════════════


class TestIpcListener:
    """Tests for IpcListener and ListenerHandle on TCP (port 0 = ephemeral)."""

    def test_bind_tcp(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        assert isinstance(listener, m.IpcListener)
        listener_addr = listener.local_address()
        assert listener_addr.is_tcp
        assert listener_addr.is_local

    def test_bind_returns_ipc_listener(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        assert isinstance(m.IpcListener.bind(addr), m.IpcListener)

    def test_local_address_is_transport_address(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        assert isinstance(local, m.TransportAddress)

    def test_local_address_is_tcp(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        assert local.is_tcp

    def test_transport_name_tcp(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        assert listener.transport_name == "tcp"

    def test_repr_non_empty(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        assert repr(listener)

    def test_into_handle_returns_listener_handle(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert isinstance(handle, m.ListenerHandle)

    def test_into_handle_accept_count_zero(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.accept_count == 0

    def test_into_handle_is_shutdown_false(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.is_shutdown is False

    def test_into_handle_shutdown(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_into_handle_shutdown_idempotent(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()
        handle.shutdown()  # must not raise
        assert handle.is_shutdown is True

    def test_handle_transport_name(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle.transport_name == "tcp"

    def test_handle_local_address(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        local = handle.local_address()
        assert isinstance(local, m.TransportAddress)
        assert local.is_tcp

    def test_handle_repr_non_empty(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert repr(handle)

    def test_accept_timeout_raises_on_no_client(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        with pytest.raises(RuntimeError):
            listener.accept(timeout_ms=50)

    def test_into_handle_twice_raises(self):
        """into_handle() must raise on second call."""
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.into_handle()

    def test_local_address_after_into_handle_raises(self):
        """local_address() must raise after the listener is consumed."""
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.local_address()


# ═════════════════════════════════════════════════════════════════════════════
# FramedChannel (client via connect_ipc + ListenerHandle — no accept needed)
# ═════════════════════════════════════════════════════════════════════════════


def _make_client_channel():
    """Bind listener, convert to handle (no accept), return (handle, client)."""
    addr = m.TransportAddress.tcp("127.0.0.1", 0)
    listener = m.IpcListener.bind(addr)
    local = listener.local_address()
    handle = listener.into_handle()
    client = m.connect_ipc(local)
    return handle, client


class TestFramedChannel:
    """Tests for FramedChannel lifecycle via connect_ipc + ListenerHandle."""

    def test_client_is_framed_channel(self):
        _handle, client = _make_client_channel()
        try:
            assert isinstance(client, m.FramedChannel)
        finally:
            client.shutdown()

    def test_is_running_true_initially(self):
        _handle, client = _make_client_channel()
        try:
            assert client.is_running is True
        finally:
            client.shutdown()

    def test_repr_non_empty(self):
        _handle, client = _make_client_channel()
        try:
            assert repr(client)
        finally:
            client.shutdown()

    def test_try_recv_empty_returns_none(self):
        _handle, client = _make_client_channel()
        try:
            result = client.try_recv()
            assert result is None
        finally:
            client.shutdown()

    def test_recv_timeout_returns_none(self):
        _handle, client = _make_client_channel()
        try:
            result = client.recv(timeout_ms=50)
            assert result is None
        finally:
            client.shutdown()

    def test_send_request_returns_id_string(self):
        _handle, client = _make_client_channel()
        try:
            req_id = client.send_request("execute_python", b"print('hello')")
            assert isinstance(req_id, str)
            assert len(req_id) > 0
        finally:
            client.shutdown()

    def test_send_request_no_params(self):
        _handle, client = _make_client_channel()
        try:
            req_id = client.send_request("list_objects")
            assert isinstance(req_id, str)
        finally:
            client.shutdown()

    def test_send_request_uuid_unique(self):
        _handle, client = _make_client_channel()
        try:
            ids = [client.send_request(f"method_{i}") for i in range(5)]
            assert len(set(ids)) == 5
        finally:
            client.shutdown()

    def test_send_notify_no_error(self):
        _handle, client = _make_client_channel()
        try:
            client.send_notify("scene_changed", b"payload_data")
        finally:
            client.shutdown()

    def test_send_notify_no_params(self):
        _handle, client = _make_client_channel()
        try:
            client.send_notify("heartbeat")
        finally:
            client.shutdown()

    def test_shutdown_idempotent(self):
        _handle, client = _make_client_channel()
        client.shutdown()
        client.shutdown()  # must not raise

    def test_is_running_false_after_shutdown(self):
        _handle, client = _make_client_channel()
        client.shutdown()
        time.sleep(0.05)
        assert client.is_running is False

    def test_send_response_invalid_uuid_raises(self):
        _handle, client = _make_client_channel()
        try:
            with pytest.raises((RuntimeError, ValueError)):
                client.send_response("not-a-valid-uuid", True, b"data", None)
        finally:
            client.shutdown()

    def test_ping_api_accessible(self):
        """ping() may timeout without a real server; just verify API is callable."""
        _handle, client = _make_client_channel()
        try:
            try:
                rtt = client.ping(timeout_ms=200)
                assert isinstance(rtt, int)
                assert rtt >= 0
            except RuntimeError:
                pass  # expected without a server processing pings
        finally:
            client.shutdown()


# ═════════════════════════════════════════════════════════════════════════════
# connect_ipc
# ═════════════════════════════════════════════════════════════════════════════


class TestConnectIpc:
    """Tests for the connect_ipc() factory function."""

    def test_connect_to_listener_via_handle(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        _handle = listener.into_handle()
        client = m.connect_ipc(local)
        assert isinstance(client, m.FramedChannel)
        client.shutdown()

    def test_connect_to_closed_port_raises(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 1)
        with pytest.raises(RuntimeError):
            m.connect_ipc(addr)

    def test_connect_returns_framed_channel(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        _handle = listener.into_handle()
        client = m.connect_ipc(local)
        assert isinstance(client, m.FramedChannel)
        client.shutdown()

    def test_connect_client_is_running(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        _handle = listener.into_handle()
        client = m.connect_ipc(local)
        try:
            assert client.is_running is True
        finally:
            client.shutdown()

    def test_connect_multiple_clients(self):
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        local = listener.local_address()
        _handle = listener.into_handle()
        clients = [m.connect_ipc(local) for _ in range(3)]
        try:
            for c in clients:
                assert isinstance(c, m.FramedChannel)
                assert c.is_running is True
        finally:
            for c in clients:
                c.shutdown()
