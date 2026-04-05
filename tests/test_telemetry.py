"""Tests for dcc-mcp-telemetry Python bindings.

Covers TelemetryConfig, ActionRecorder, ActionMetrics, RecordingGuard,
is_telemetry_initialized, and shutdown_telemetry.
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── TelemetryConfig ───────────────────────────────────────────────────────────


class TestTelemetryConfig:
    def test_constructor_sets_service_name(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("my-service")
        assert cfg.service_name == "my-service"

    def test_defaults_enable_metrics(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_metrics is True

    def test_defaults_enable_tracing(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        assert cfg.enable_tracing is True

    def test_with_noop_exporter_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_noop_exporter()
        assert returned is cfg

    def test_with_stdout_exporter_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_stdout_exporter()
        assert returned is cfg

    def test_with_json_logs_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_json_logs()
        assert returned is cfg

    def test_with_text_logs_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_text_logs()
        assert returned is cfg

    def test_with_attribute_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_attribute("dcc", "maya")
        assert returned is cfg

    def test_with_service_version_returns_self(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        returned = cfg.with_service_version("1.2.3")
        assert returned is cfg

    def test_set_enable_metrics_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg.set_enable_metrics(False)
        assert cfg.enable_metrics is False

    def test_set_enable_tracing_false(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("svc")
        cfg.set_enable_tracing(False)
        assert cfg.enable_tracing is False

    def test_repr_contains_service_name(self) -> None:
        cfg = dcc_mcp_core.TelemetryConfig("my-service")
        assert "my-service" in repr(cfg)

    def test_chaining_fluent_api(self) -> None:
        cfg = (
            dcc_mcp_core.TelemetryConfig("chained").with_noop_exporter().with_text_logs().with_attribute("env", "test")
        )
        assert cfg.service_name == "chained"


# ── ActionRecorder ────────────────────────────────────────────────────────────


class TestActionRecorder:
    def test_create_recorder(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("test-scope")
        assert recorder is not None

    def test_metrics_none_before_first_invocation(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-a")
        assert recorder.metrics("not_yet_run") is None

    def test_start_and_finish_success(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-b")
        guard = recorder.start("create_sphere", "maya")
        guard.finish(True)
        m = recorder.metrics("create_sphere")
        assert m is not None
        assert m.invocation_count == 1
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_start_and_finish_failure(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-c")
        guard = recorder.start("delete_all", "blender")
        guard.finish(False)
        m = recorder.metrics("delete_all")
        assert m is not None
        assert m.failure_count == 1
        assert m.success_count == 0

    def test_multiple_invocations_accumulate(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-d")
        for _ in range(4):
            recorder.start("render", "houdini").finish(True)
        recorder.start("render", "houdini").finish(False)
        m = recorder.metrics("render")
        assert m is not None
        assert m.invocation_count == 5
        assert m.success_count == 4
        assert m.failure_count == 1

    def test_success_rate(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-e")
        for _ in range(3):
            recorder.start("act", "maya").finish(True)
        recorder.start("act", "maya").finish(False)
        m = recorder.metrics("act")
        assert m is not None
        assert abs(m.success_rate() - 0.75) < 1e-6

    def test_all_metrics_returns_all(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-f")
        recorder.start("action1", "maya").finish(True)
        recorder.start("action2", "blender").finish(False)
        all_m = recorder.all_metrics()
        names = {m.action_name for m in all_m}
        assert "action1" in names
        assert "action2" in names

    def test_reset_clears_stats(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-g")
        recorder.start("act", "maya").finish(True)
        recorder.reset()
        assert recorder.metrics("act") is None

    def test_all_metrics_empty_initially(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-h")
        assert recorder.all_metrics() == []


# ── ActionMetrics ─────────────────────────────────────────────────────────────


class TestActionMetrics:
    def _make_metrics(self, successes: int = 3, failures: int = 1) -> dcc_mcp_core.ActionMetrics:
        recorder = dcc_mcp_core.ActionRecorder("scope-metrics")
        for _ in range(successes):
            recorder.start("act", "maya").finish(True)
        for _ in range(failures):
            recorder.start("act", "maya").finish(False)
        return recorder.metrics("act")

    def test_action_name(self) -> None:
        m = self._make_metrics()
        assert m.action_name == "act"

    def test_invocation_count(self) -> None:
        m = self._make_metrics(successes=2, failures=3)
        assert m.invocation_count == 5

    def test_success_count(self) -> None:
        m = self._make_metrics(successes=7, failures=0)
        assert m.success_count == 7

    def test_failure_count(self) -> None:
        m = self._make_metrics(successes=0, failures=5)
        assert m.failure_count == 5

    def test_avg_duration_ms_nonnegative(self) -> None:
        m = self._make_metrics()
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_nonnegative(self) -> None:
        m = self._make_metrics()
        assert m.p95_duration_ms >= 0.0

    def test_p99_duration_ms_nonnegative(self) -> None:
        m = self._make_metrics()
        assert m.p99_duration_ms >= 0.0

    def test_repr_contains_action_name(self) -> None:
        m = self._make_metrics()
        assert "act" in repr(m)


# ── RecordingGuard ────────────────────────────────────────────────────────────


class TestRecordingGuard:
    def test_context_manager_success(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-guard")
        with recorder.start("ctx_action", "maya") as _guard:
            pass
        m = recorder.metrics("ctx_action")
        assert m is not None
        assert m.success_count == 1

    def test_context_manager_records_failure_on_exception(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-guard-ex")
        with pytest.raises(ValueError), recorder.start("err_action", "maya"):
            raise ValueError("boom")
        m = recorder.metrics("err_action")
        assert m is not None
        assert m.failure_count == 1

    def test_finish_success_explicit(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-guard-ok")
        guard = recorder.start("explicit_ok", "blender")
        guard.finish(True)
        assert recorder.metrics("explicit_ok").success_count == 1

    def test_repr_contains_action_name(self) -> None:
        recorder = dcc_mcp_core.ActionRecorder("scope-repr")
        guard = recorder.start("my_action", "maya")
        r = repr(guard)
        assert "my_action" in r
        guard.finish(True)


# ── Module-level functions ────────────────────────────────────────────────────


class TestTelemetryFunctions:
    def test_is_telemetry_initialized_returns_bool(self) -> None:
        result = dcc_mcp_core.is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_shutdown_telemetry_safe_before_init(self) -> None:
        # Should not raise even if provider was never initialized or
        # already shut down.
        dcc_mcp_core.shutdown_telemetry()
