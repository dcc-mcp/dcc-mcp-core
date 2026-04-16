"""Deep tests: ToolRecorder/ToolMetrics, TelemetryConfig, AuditLog/AuditEntry.

Also covers: SandboxContext.execute_json, VersionedRegistry full matrix.
Run #140: 12082 → ~12220 collected (+128 tests)
"""

from __future__ import annotations

import json
import time

import pytest

import dcc_mcp_core
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import SemVer
from dcc_mcp_core import TelemetryConfig
from dcc_mcp_core import ToolRecorder
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import is_telemetry_initialized
from dcc_mcp_core import shutdown_telemetry


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# ToolRecorder — basic creation
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestActionRecorderCreate:
    """ToolRecorder construction and basic API."""

    def test_create_with_scope(self):
        """ToolRecorder can be created with a scope name."""
        recorder = ToolRecorder("my-server")
        assert recorder is not None

    def test_metrics_none_before_recording(self):
        """metrics() returns None for unknown action before any recording."""
        recorder = ToolRecorder("test")
        assert recorder.metrics("unknown_action") is None

    def test_all_metrics_empty_before_recording(self):
        """all_metrics() returns empty list before any recording."""
        recorder = ToolRecorder("empty-scope")
        result = recorder.all_metrics()
        assert isinstance(result, list)
        assert len(result) == 0

    def test_reset_clears_empty_recorder(self):
        """reset() on fresh recorder is a no-op (no error)."""
        recorder = ToolRecorder("fresh")
        recorder.reset()
        assert recorder.all_metrics() == []


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# ToolRecorder — manual guard
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestActionRecorderManualGuard:
    """Manual start/finish guard pattern."""

    def test_single_success_invocation_count(self):
        """Single successful call sets invocation_count=1."""
        r = ToolRecorder("test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert m.invocation_count == 1

    def test_single_success_success_count(self):
        """Single successful call sets success_count=1."""
        r = ToolRecorder("test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert m.success_count == 1

    def test_single_success_failure_count_zero(self):
        """Single successful call leaves failure_count=0."""
        r = ToolRecorder("test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert m.failure_count == 0

    def test_single_success_rate_one(self):
        """Single successful call gives success_rate=1.0."""
        r = ToolRecorder("test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert m.success_rate() == pytest.approx(1.0)

    def test_single_failure_invocation_count(self):
        """Single failed call sets invocation_count=1."""
        r = ToolRecorder("test")
        g = r.start("fail_op", "blender")
        g.finish(success=False)
        m = r.metrics("fail_op")
        assert m.invocation_count == 1

    def test_single_failure_failure_count(self):
        """Single failed call sets failure_count=1."""
        r = ToolRecorder("test")
        g = r.start("fail_op", "blender")
        g.finish(success=False)
        m = r.metrics("fail_op")
        assert m.failure_count == 1

    def test_single_failure_success_count_zero(self):
        """Single failed call leaves success_count=0."""
        r = ToolRecorder("test")
        g = r.start("fail_op", "blender")
        g.finish(success=False)
        m = r.metrics("fail_op")
        assert m.success_count == 0

    def test_single_failure_success_rate_zero(self):
        """Single failed call gives success_rate=0.0."""
        r = ToolRecorder("test")
        g = r.start("fail_op", "blender")
        g.finish(success=False)
        m = r.metrics("fail_op")
        assert m.success_rate() == pytest.approx(0.0)

    def test_mixed_calls_invocation_count(self):
        """5 successes + 1 failure → invocation_count=6."""
        r = ToolRecorder("mixed")
        for _ in range(5):
            g = r.start("create_sphere", "maya")
            g.finish(success=True)
        g2 = r.start("create_sphere", "maya")
        g2.finish(success=False)
        assert r.metrics("create_sphere").invocation_count == 6

    def test_mixed_calls_success_count(self):
        """5 successes + 1 failure → success_count=5."""
        r = ToolRecorder("mixed2")
        for _ in range(5):
            g = r.start("a", "maya")
            g.finish(success=True)
        g2 = r.start("a", "maya")
        g2.finish(success=False)
        assert r.metrics("a").success_count == 5

    def test_mixed_calls_success_rate(self):
        """5 successes + 1 failure → success_rate≈0.833."""
        r = ToolRecorder("mixed3")
        for _ in range(5):
            g = r.start("a", "maya")
            g.finish(success=True)
        g2 = r.start("a", "maya")
        g2.finish(success=False)
        assert r.metrics("a").success_rate() == pytest.approx(5 / 6, abs=1e-9)

    def test_action_name_in_metrics(self):
        """metrics.action_name reflects the registered action."""
        r = ToolRecorder("name-test")
        g = r.start("unique_action_xyz", "maya")
        g.finish(success=True)
        m = r.metrics("unique_action_xyz")
        assert m.action_name == "unique_action_xyz"

    def test_avg_duration_ms_positive(self):
        """avg_duration_ms is a non-negative float after one call."""
        r = ToolRecorder("dur-test")
        g = r.start("slow_op", "houdini")
        time.sleep(0.005)
        g.finish(success=True)
        m = r.metrics("slow_op")
        assert isinstance(m.avg_duration_ms, float)
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_type(self):
        """p95_duration_ms is a float."""
        r = ToolRecorder("p95-test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert isinstance(m.p95_duration_ms, float)

    def test_p99_duration_ms_type(self):
        """p99_duration_ms is a float."""
        r = ToolRecorder("p99-test")
        g = r.start("op", "maya")
        g.finish(success=True)
        m = r.metrics("op")
        assert isinstance(m.p99_duration_ms, float)

    def test_p95_gte_avg(self):
        """p95_duration_ms >= avg_duration_ms."""
        r = ToolRecorder("p-compare")
        for _ in range(10):
            g = r.start("op", "maya")
            g.finish(success=True)
        m = r.metrics("op")
        assert m.p95_duration_ms >= m.avg_duration_ms - 1e-9

    def test_p99_gte_p95(self):
        """p99_duration_ms >= p95_duration_ms."""
        r = ToolRecorder("p-compare2")
        for _ in range(10):
            g = r.start("op", "maya")
            g.finish(success=True)
        m = r.metrics("op")
        assert m.p99_duration_ms >= m.p95_duration_ms - 1e-9


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# ToolRecorder — context manager
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestActionRecorderContextManager:
    """Context manager (with recorder.start(...) as guard) pattern."""

    def test_context_manager_success_increments(self):
        """Successful with block records success=True."""
        r = ToolRecorder("ctx")
        with r.start("batch_op", "blender"):
            pass
        m = r.metrics("batch_op")
        assert m.invocation_count == 1
        assert m.success_count == 1

    def test_context_manager_exception_records_failure(self):
        """Exception in with block records success=False."""
        r = ToolRecorder("ctx2")
        with pytest.raises(ValueError), r.start("risky_op", "maya"):
            raise ValueError("oops")
        m = r.metrics("risky_op")
        assert m.invocation_count == 1
        assert m.failure_count == 1

    def test_context_manager_multiple_calls(self):
        """Multiple context manager calls accumulate correctly."""
        r = ToolRecorder("ctx3")
        for _ in range(3):
            with r.start("op", "maya"):
                pass
        m = r.metrics("op")
        assert m.invocation_count == 3
        assert m.success_count == 3


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# ToolRecorder — all_metrics + reset
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestActionRecorderAllMetricsReset:
    """all_metrics() list and reset() semantics."""

    def test_all_metrics_contains_all_actions(self):
        """all_metrics() includes entries for each distinct action name."""
        r = ToolRecorder("multi")
        for name in ["a", "b", "c"]:
            g = r.start(name, "maya")
            g.finish(success=True)
        all_m = r.all_metrics()
        names = {m.action_name for m in all_m}
        assert {"a", "b", "c"} == names

    def test_all_metrics_length(self):
        """all_metrics() length equals distinct action count."""
        r = ToolRecorder("multi2")
        for name in ["x", "y"]:
            g = r.start(name, "maya")
            g.finish(success=True)
        assert len(r.all_metrics()) == 2

    def test_reset_clears_metrics(self):
        """reset() makes metrics() return None again."""
        r = ToolRecorder("reset-test")
        g = r.start("my_action", "maya")
        g.finish(success=True)
        assert r.metrics("my_action") is not None
        r.reset()
        assert r.metrics("my_action") is None

    def test_reset_clears_all_metrics(self):
        """reset() makes all_metrics() return empty list."""
        r = ToolRecorder("reset-all")
        g = r.start("my_action", "maya")
        g.finish(success=True)
        r.reset()
        assert r.all_metrics() == []

    def test_accumulate_after_reset(self):
        """Recording after reset accumulates from scratch."""
        r = ToolRecorder("post-reset")
        g = r.start("op", "maya")
        g.finish(success=True)
        r.reset()
        g2 = r.start("op", "maya")
        g2.finish(success=True)
        m = r.metrics("op")
        assert m.invocation_count == 1


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# TelemetryConfig — construction and chaining
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestTelemetryConfigConstruction:
    """TelemetryConfig constructor and method chaining."""

    def test_create_with_name(self):
        """TelemetryConfig can be constructed with a service name."""
        cfg = TelemetryConfig("my-service")
        assert cfg is not None

    def test_with_noop_exporter_returns_self(self):
        """with_noop_exporter() returns the same config object."""
        cfg = TelemetryConfig("svc")
        result = cfg.with_noop_exporter()
        assert result is cfg

    def test_with_attribute_returns_self(self):
        """with_attribute() returns the same config object for chaining."""
        cfg = TelemetryConfig("svc").with_noop_exporter()
        result = cfg.with_attribute("dcc.type", "maya")
        assert result is cfg

    def test_with_service_version_returns_self(self):
        """with_service_version() returns the same config object."""
        cfg = TelemetryConfig("svc").with_noop_exporter()
        result = cfg.with_service_version("1.2.3")
        assert result is cfg

    def test_set_enable_metrics_returns_self(self):
        """set_enable_metrics() returns the same config object."""
        cfg = TelemetryConfig("svc").with_noop_exporter()
        result = cfg.set_enable_metrics(True)
        assert result is cfg

    def test_set_enable_tracing_returns_self(self):
        """set_enable_tracing() returns the same config object."""
        cfg = TelemetryConfig("svc").with_noop_exporter()
        result = cfg.set_enable_tracing(False)
        assert result is cfg

    def test_chain_all_methods(self):
        """All builder methods can be chained together."""
        cfg = (
            TelemetryConfig("chain-test")
            .with_noop_exporter()
            .with_attribute("k", "v")
            .with_service_version("0.1.0")
            .set_enable_metrics(True)
            .set_enable_tracing(True)
        )
        assert cfg is not None

    def test_with_stdout_exporter_returns_self(self):
        """with_stdout_exporter() returns the same config object."""
        cfg = TelemetryConfig("svc")
        result = cfg.with_stdout_exporter()
        assert result is cfg

    def test_with_json_logs_returns_self(self):
        """with_json_logs() returns the same config object."""
        cfg = TelemetryConfig("svc")
        result = cfg.with_json_logs()
        assert result is cfg


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# TelemetryConfig — init/shutdown (process-level singleton)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestTelemetryConfigInitShutdown:
    """init() / is_telemetry_initialized() / shutdown_telemetry()."""

    def test_is_initialized_function_returns_bool(self):
        """is_telemetry_initialized() returns a bool."""
        result = is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_shutdown_telemetry_callable(self):
        """shutdown_telemetry() can be called without error."""
        shutdown_telemetry()

    def test_init_raises_if_tracer_already_set(self):
        """init() raises RuntimeError when global tracer dispatcher is already set.

        In pytest the ToolRecorder (used in other tests) triggers OTel initialisation
        the moment its Rust implementation creates a Meter, so the global dispatcher is
        always occupied by the time this test runs.  We therefore expect init() to fail.
        """
        cfg = TelemetryConfig("pytest-run").with_noop_exporter()
        with pytest.raises(RuntimeError):
            cfg.init()

    def test_shutdown_idempotent(self):
        """shutdown_telemetry() can be called multiple times without error."""
        shutdown_telemetry()
        shutdown_telemetry()


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# SandboxContext.execute_json + AuditLog
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestSandboxContextExecuteJson:
    """execute_json and resulting audit entries."""

    def _make_ctx(self, actions: list[str]) -> SandboxContext:
        policy = SandboxPolicy()
        policy.allow_actions(actions)
        ctx = SandboxContext(policy)
        ctx.set_actor("test-agent")
        return ctx

    def test_echo_returns_string(self):
        """execute_json('echo', ...) returns a JSON string."""
        ctx = self._make_ctx(["echo"])
        result = ctx.execute_json("echo", json.dumps({"x": 1}))
        assert isinstance(result, str)

    def test_echo_result_is_valid_json(self):
        """execute_json('echo', ...) return value is valid JSON."""
        ctx = self._make_ctx(["echo"])
        result = ctx.execute_json("echo", json.dumps({"msg": "hi"}))
        parsed = json.loads(result)
        # null or dict both acceptable
        assert parsed is None or isinstance(parsed, dict)

    def test_denied_action_raises_runtime_error(self):
        """execute_json on a denied action raises RuntimeError."""
        ctx = self._make_ctx(["echo"])
        with pytest.raises(RuntimeError):
            ctx.execute_json("delete_all", "{}")

    def test_denied_action_error_message(self):
        """Denial error message mentions the action name."""
        ctx = self._make_ctx(["echo"])
        with pytest.raises(RuntimeError, match="delete_all"):
            ctx.execute_json("delete_all", "{}")

    def test_action_count_increments_on_success(self):
        """action_count increments only on successful execute_json."""
        ctx = self._make_ctx(["echo"])
        assert ctx.action_count == 0
        ctx.execute_json("echo", "{}")
        assert ctx.action_count == 1
        ctx.execute_json("echo", "{}")
        assert ctx.action_count == 2

    def test_action_count_no_increment_on_denial(self):
        """action_count does not increment when action is denied."""
        ctx = self._make_ctx(["echo"])
        before = ctx.action_count
        with pytest.raises(RuntimeError):
            ctx.execute_json("banned_action", "{}")
        assert ctx.action_count == before

    def test_is_allowed_returns_true_for_allowed(self):
        """is_allowed returns True for an action in the allow list."""
        ctx = self._make_ctx(["create_sphere", "list_objects"])
        assert ctx.is_allowed("create_sphere") is True

    def test_is_allowed_returns_false_for_denied(self):
        """is_allowed returns False for an action not in the allow list."""
        ctx = self._make_ctx(["create_sphere"])
        assert ctx.is_allowed("delete_scene") is False

    def test_is_allowed_returns_bool(self):
        """is_allowed returns a bool type."""
        ctx = self._make_ctx(["echo"])
        result = ctx.is_allowed("echo")
        assert isinstance(result, bool)

    def test_is_path_allowed_returns_bool(self):
        """is_path_allowed returns a bool type."""
        ctx = self._make_ctx(["echo"])
        result = ctx.is_path_allowed("/some/path")
        assert isinstance(result, bool)


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# AuditLog entries
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestAuditLogEntries:
    """AuditLog and AuditEntry properties."""

    def _make_filled_ctx(self) -> SandboxContext:
        policy = SandboxPolicy()
        policy.allow_actions(["echo", "create_sphere"])
        ctx = SandboxContext(policy)
        ctx.set_actor("audit-agent")
        ctx.execute_json("echo", json.dumps({"val": 42}))
        with pytest.raises(RuntimeError):
            ctx.execute_json("denied_action", "{}")
        return ctx

    def test_audit_log_accessible(self):
        """ctx.audit_log returns an AuditLog object."""
        ctx = self._make_filled_ctx()
        assert ctx.audit_log is not None

    def test_audit_log_len(self):
        """len(audit_log) equals total entry count (success + denial)."""
        ctx = self._make_filled_ctx()
        assert len(ctx.audit_log) == 2

    def test_entries_returns_list(self):
        """audit_log.entries() returns a list."""
        ctx = self._make_filled_ctx()
        entries = ctx.audit_log.entries()
        assert isinstance(entries, list)

    def test_entries_count(self):
        """entries() count matches total executions (success + denied)."""
        ctx = self._make_filled_ctx()
        entries = ctx.audit_log.entries()
        assert len(entries) == 2

    def test_entry_action_name(self):
        """AuditEntry.action is the action name string."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.action, str)
        assert entry.action in ("echo", "denied_action")

    def test_entry_actor(self):
        """AuditEntry.actor matches the set_actor() value."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert entry.actor == "audit-agent"

    def test_entry_timestamp_ms_is_int(self):
        """AuditEntry.timestamp_ms is an int (Unix ms)."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.timestamp_ms, int)

    def test_entry_timestamp_ms_positive(self):
        """AuditEntry.timestamp_ms is a positive value."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert entry.timestamp_ms > 0

    def test_entry_params_json_is_string(self):
        """AuditEntry.params_json is a string."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.params_json, str)

    def test_entry_params_json_is_valid_json(self):
        """AuditEntry.params_json is valid JSON."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        parsed = json.loads(entry.params_json)
        assert parsed is not None

    def test_entry_duration_ms_is_int(self):
        """AuditEntry.duration_ms is an int."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.duration_ms, int)

    def test_entry_duration_ms_non_negative(self):
        """AuditEntry.duration_ms is non-negative."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert entry.duration_ms >= 0

    def test_entry_outcome_is_string(self):
        """AuditEntry.outcome is a string."""
        ctx = self._make_filled_ctx()
        entry = ctx.audit_log.entries()[0]
        assert isinstance(entry.outcome, str)

    def test_entry_outcome_values(self):
        """AuditEntry.outcome is one of the expected values."""
        ctx = self._make_filled_ctx()
        valid_outcomes = {"success", "denied", "error", "timeout"}
        for entry in ctx.audit_log.entries():
            assert entry.outcome in valid_outcomes

    def test_entry_outcome_detail_optional(self):
        """AuditEntry.outcome_detail is str or None."""
        ctx = self._make_filled_ctx()
        for entry in ctx.audit_log.entries():
            assert entry.outcome_detail is None or isinstance(entry.outcome_detail, str)


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# AuditLog filter methods
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestAuditLogFilters:
    """successes(), denials(), entries_for_action(), to_json()."""

    def _ctx_with_two_calls(self) -> SandboxContext:
        policy = SandboxPolicy()
        policy.allow_actions(["echo"])
        ctx = SandboxContext(policy)
        ctx.set_actor("filter-agent")
        ctx.execute_json("echo", "{}")
        with pytest.raises(RuntimeError):
            ctx.execute_json("forbidden_op", "{}")
        return ctx

    def test_successes_count(self):
        """successes() returns only successful entries."""
        ctx = self._ctx_with_two_calls()
        s = ctx.audit_log.successes()
        assert len(s) == 1

    def test_successes_outcome(self):
        """All entries from successes() have outcome='success'."""
        ctx = self._ctx_with_two_calls()
        for e in ctx.audit_log.successes():
            assert e.outcome == "success"

    def test_denials_count(self):
        """denials() returns only denied entries."""
        ctx = self._ctx_with_two_calls()
        d = ctx.audit_log.denials()
        assert len(d) == 1

    def test_denials_outcome(self):
        """All entries from denials() have outcome='denied'."""
        ctx = self._ctx_with_two_calls()
        for e in ctx.audit_log.denials():
            assert e.outcome == "denied"

    def test_entries_for_action_echo(self):
        """entries_for_action('echo') returns echo entries."""
        ctx = self._ctx_with_two_calls()
        echo_entries = ctx.audit_log.entries_for_action("echo")
        assert len(echo_entries) == 1
        assert echo_entries[0].action == "echo"

    def test_entries_for_action_denied(self):
        """entries_for_action('forbidden_op') returns denial entries."""
        ctx = self._ctx_with_two_calls()
        denied_entries = ctx.audit_log.entries_for_action("forbidden_op")
        assert len(denied_entries) == 1
        assert denied_entries[0].action == "forbidden_op"

    def test_entries_for_action_unknown_empty(self):
        """entries_for_action for unknown action returns empty list."""
        ctx = self._ctx_with_two_calls()
        entries = ctx.audit_log.entries_for_action("no_such_action")
        assert entries == []

    def test_to_json_is_string(self):
        """to_json() returns a string."""
        ctx = self._ctx_with_two_calls()
        result = ctx.audit_log.to_json()
        assert isinstance(result, str)

    def test_to_json_parses_as_json(self):
        """to_json() returns parseable JSON."""
        ctx = self._ctx_with_two_calls()
        parsed = json.loads(ctx.audit_log.to_json())
        assert parsed is not None

    def test_to_json_is_array(self):
        """to_json() returns a JSON array."""
        ctx = self._ctx_with_two_calls()
        parsed = json.loads(ctx.audit_log.to_json())
        assert isinstance(parsed, list)

    def test_to_json_length(self):
        """to_json() array has same length as entries()."""
        ctx = self._ctx_with_two_calls()
        parsed = json.loads(ctx.audit_log.to_json())
        assert len(parsed) == len(ctx.audit_log.entries())


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# VersionedRegistry — create and register
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestVersionedRegistryCreate:
    """VersionedRegistry construction and basic registration."""

    def test_create(self):
        """VersionedRegistry can be instantiated."""
        vr = VersionedRegistry()
        assert vr is not None

    def test_total_entries_empty(self):
        """total_entries() is 0 on a fresh registry."""
        vr = VersionedRegistry()
        assert vr.total_entries() == 0

    def test_keys_empty(self):
        """keys() is an empty list on a fresh registry."""
        vr = VersionedRegistry()
        assert vr.keys() == []

    def test_register_one_entry(self):
        """Registering one versioned action sets total_entries to 1."""
        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        assert vr.total_entries() == 1

    def test_register_multiple_versions(self):
        """Registering 3 versions for same action gives total_entries=3."""
        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "1.5.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")
        assert vr.total_entries() == 3

    def test_register_different_dccs(self):
        """Same action for different DCCs are distinct entries."""
        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "blender", "1.0.0")
        assert vr.total_entries() == 2

    def test_keys_after_registration(self):
        """keys() returns (name, dcc) tuples after registration."""
        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("delete_mesh", "maya", "1.0.0")
        keys = vr.keys()
        assert len(keys) == 2

    def test_versions_sorted(self):
        """versions() returns sorted version list."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "2.0.0")
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "1.5.0")
        versions = vr.versions("op", "maya")
        assert versions == ["1.0.0", "1.5.0", "2.0.0"]

    def test_latest_version(self):
        """latest_version() returns the highest semantic version."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "2.0.0")
        vr.register_versioned("op", "maya", "1.9.0")
        assert vr.latest_version("op", "maya") == "2.0.0"


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# VersionedRegistry — resolve
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestVersionedRegistryResolve:
    """resolve() — best match within constraint."""

    def _registry(self) -> VersionedRegistry:
        vr = VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0", description="v1")
        vr.register_versioned("create_sphere", "maya", "1.5.0", description="v1.5")
        vr.register_versioned("create_sphere", "maya", "2.0.0", description="v2")
        return vr

    def test_resolve_caret_returns_dict(self):
        """resolve() returns a dict."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "^1.0.0")
        assert isinstance(result, dict)

    def test_resolve_caret_best_match(self):
        """^1.0.0 resolves to highest 1.x version (1.5.0)."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "^1.0.0")
        assert result["version"] == "1.5.0"

    def test_resolve_star_returns_latest(self):
        """* resolves to latest version."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "*")
        assert result["version"] == "2.0.0"

    def test_resolve_gte_constraint(self):
        """>=2.0.0 resolves to 2.0.0."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", ">=2.0.0")
        assert result["version"] == "2.0.0"

    def test_resolve_exact_version(self):
        """=1.0.0 or 1.0.0 resolves to exactly 1.0.0."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "1.0.0")
        assert result["version"] == "1.0.0"

    def test_resolve_contains_name(self):
        """Resolved dict contains 'name' key."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "*")
        assert "name" in result

    def test_resolve_contains_dcc(self):
        """Resolved dict contains 'dcc' key."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", "*")
        assert "dcc" in result

    def test_resolve_no_match_returns_none(self):
        """resolve() returns None when no version satisfies constraint."""
        vr = self._registry()
        result = vr.resolve("create_sphere", "maya", ">=99.0.0")
        assert result is None

    def test_resolve_unknown_action_returns_none(self):
        """resolve() returns None for unknown action name."""
        vr = self._registry()
        result = vr.resolve("no_such_action", "maya", "*")
        assert result is None


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# VersionedRegistry — resolve_all
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestVersionedRegistryResolveAll:
    """resolve_all() — all matches."""

    def _registry(self) -> VersionedRegistry:
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "1.5.0")
        vr.register_versioned("op", "maya", "2.0.0")
        return vr

    def test_resolve_all_star_count(self):
        """resolve_all with '*' returns all 3 versions."""
        vr = self._registry()
        all_v = vr.resolve_all("op", "maya", "*")
        assert len(all_v) == 3

    def test_resolve_all_caret_count(self):
        """resolve_all with '^1.0.0' returns versions 1.x only (2 entries)."""
        vr = self._registry()
        all_v = vr.resolve_all("op", "maya", "^1.0.0")
        assert len(all_v) == 2

    def test_resolve_all_caret_versions(self):
        """resolve_all '^1.0.0' versions are 1.0.0 and 1.5.0."""
        vr = self._registry()
        all_v = vr.resolve_all("op", "maya", "^1.0.0")
        versions = [r["version"] for r in all_v]
        assert set(versions) == {"1.0.0", "1.5.0"}

    def test_resolve_all_no_match_empty(self):
        """resolve_all with unsatisfiable constraint returns empty list."""
        vr = self._registry()
        all_v = vr.resolve_all("op", "maya", ">=99.0.0")
        assert all_v == []

    def test_resolve_all_returns_list(self):
        """resolve_all returns a list."""
        vr = self._registry()
        result = vr.resolve_all("op", "maya", "*")
        assert isinstance(result, list)


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# VersionedRegistry — remove
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestVersionedRegistryRemove:
    """remove() semantics."""

    def test_remove_caret_count(self):
        """remove('^1.0.0') removes 2 out of 3 versions, returns 2."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "1.5.0")
        vr.register_versioned("op", "maya", "2.0.0")
        removed = vr.remove("op", "maya", "^1.0.0")
        assert removed == 2

    def test_remove_caret_leaves_remaining(self):
        """After remove('^1.0.0'), only 2.0.0 remains."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "1.5.0")
        vr.register_versioned("op", "maya", "2.0.0")
        vr.remove("op", "maya", "^1.0.0")
        remaining = vr.versions("op", "maya")
        assert remaining == ["2.0.0"]

    def test_remove_star_all(self):
        """remove('*') removes all versions."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "2.0.0")
        removed = vr.remove("op", "maya", "*")
        assert removed == 2

    def test_remove_star_total_entries_decrements(self):
        """After remove('*'), total_entries decreases by removed count."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        vr.register_versioned("op", "maya", "2.0.0")
        vr.register_versioned("other", "blender", "1.0.0")
        vr.remove("op", "maya", "*")
        assert vr.total_entries() == 1

    def test_remove_no_match_returns_zero(self):
        """Remove with unsatisfiable constraint returns 0."""
        vr = VersionedRegistry()
        vr.register_versioned("op", "maya", "1.0.0")
        removed = vr.remove("op", "maya", ">=99.0.0")
        assert removed == 0

    def test_remove_unknown_action_returns_zero(self):
        """Remove for unknown action returns 0."""
        vr = VersionedRegistry()
        removed = vr.remove("no_such_action", "maya", "*")
        assert removed == 0


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# SemVer
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestSemVer:
    """SemVer construction, comparison, and parsing."""

    def test_create_str(self):
        """SemVer(1,2,3) str is '1.2.3'."""
        v = SemVer(1, 2, 3)
        assert str(v) == "1.2.3"

    def test_major_attr(self):
        """SemVer.major is correct."""
        v = SemVer(3, 0, 1)
        assert v.major == 3

    def test_minor_attr(self):
        """SemVer.minor is correct."""
        v = SemVer(1, 7, 0)
        assert v.minor == 7

    def test_patch_attr(self):
        """SemVer.patch is correct."""
        v = SemVer(1, 2, 9)
        assert v.patch == 9

    def test_parse_plain(self):
        """SemVer.parse('2.0.0') produces correct object."""
        v = SemVer.parse("2.0.0")
        assert v.major == 2
        assert v.minor == 0
        assert v.patch == 0

    def test_parse_v_prefix(self):
        """SemVer.parse('v1.5.0-alpha') strips v prefix and pre-release."""
        v = SemVer.parse("v1.5.0-alpha")
        assert v.major == 1
        assert v.minor == 5
        assert v.patch == 0

    def test_gt_comparison(self):
        """SemVer comparison: 2.0.0 > 1.2.3."""
        assert SemVer.parse("2.0.0") > SemVer(1, 2, 3)

    def test_lt_comparison(self):
        """SemVer comparison: 1.0.0 < 1.5.0."""
        assert SemVer(1, 0, 0) < SemVer(1, 5, 0)

    def test_eq_comparison(self):
        """SemVer comparison: 1.0.0 == 1.0.0."""
        assert SemVer(1, 0, 0) == SemVer(1, 0, 0)


# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# VersionConstraint
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
class TestVersionConstraint:
    """VersionConstraint parsing and matching."""

    def test_caret_matches_same_major(self):
        """^1.0.0 matches 1.5.0 (same major)."""
        c = VersionConstraint.parse("^1.0.0")
        assert c.matches(SemVer(1, 5, 0)) is True

    def test_caret_does_not_match_higher_major(self):
        """^1.0.0 does not match 2.0.0."""
        c = VersionConstraint.parse("^1.0.0")
        assert c.matches(SemVer(2, 0, 0)) is False

    def test_gte_matches_equal(self):
        """>=1.2.0 matches 1.2.0."""
        c = VersionConstraint.parse(">=1.2.0")
        assert c.matches(SemVer(1, 2, 0)) is True

    def test_gte_matches_higher(self):
        """>=1.2.0 matches 2.0.0."""
        c = VersionConstraint.parse(">=1.2.0")
        assert c.matches(SemVer(2, 0, 0)) is True

    def test_gte_does_not_match_lower(self):
        """>=1.2.0 does not match 1.1.9."""
        c = VersionConstraint.parse(">=1.2.0")
        assert c.matches(SemVer(1, 1, 9)) is False

    def test_tilde_matches_same_minor(self):
        """~1.5.0 matches 1.5.5 (same major.minor)."""
        c = VersionConstraint.parse("~1.5.0")
        assert c.matches(SemVer(1, 5, 5)) is True

    def test_tilde_does_not_match_higher_minor(self):
        """~1.5.0 does not match 1.6.0."""
        c = VersionConstraint.parse("~1.5.0")
        assert c.matches(SemVer(1, 6, 0)) is False

    def test_star_matches_any(self):
        """* matches any version."""
        c = VersionConstraint.parse("*")
        assert c.matches(SemVer(99, 99, 99)) is True

    def test_gt_strict(self):
        """>1.0.0 does not match 1.0.0 (strict)."""
        c = VersionConstraint.parse(">1.0.0")
        assert c.matches(SemVer(1, 0, 0)) is False

    def test_gt_matches_higher(self):
        """>1.0.0 matches 1.0.1."""
        c = VersionConstraint.parse(">1.0.0")
        assert c.matches(SemVer(1, 0, 1)) is True

    def test_lt_strict(self):
        """<2.0.0 matches 1.9.9 but not 2.0.0."""
        c = VersionConstraint.parse("<2.0.0")
        assert c.matches(SemVer(1, 9, 9)) is True
        assert c.matches(SemVer(2, 0, 0)) is False

    def test_lte_matches_equal(self):
        """<=2.0.0 matches 2.0.0."""
        c = VersionConstraint.parse("<=2.0.0")
        assert c.matches(SemVer(2, 0, 0)) is True
