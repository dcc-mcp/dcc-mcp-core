"""Deep tests for VtValue type factories and ActionRecorder/RecordingGuard.

Covers:
- VtValue.from_bool / from_int / from_float / from_string / from_token / from_asset
- VtValue.from_vec3f and to_python() returns tuple-like
- VtValue.type_name for each factory
- VtValue.to_python() returns correct Python primitives
- ActionRecorder.start() returns RecordingGuard
- RecordingGuard.finish(success=True/False) updates metrics
- RecordingGuard as context manager: success on no exception
- RecordingGuard as context manager: failure on exception
- ActionRecorder.metrics() returns ActionMetrics
- ActionMetrics.invocation_count / success_count / failure_count
- ActionMetrics.avg_duration_ms >= 0
- ActionMetrics.success_rate() in [0.0, 1.0]
- ActionRecorder.all_metrics() list length
- ActionRecorder.reset() clears all metrics
- ScriptResult: all fields accessible
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import ActionMetrics
from dcc_mcp_core import ActionRecorder
from dcc_mcp_core import RecordingGuard
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# VtValue factories and to_python()
# ---------------------------------------------------------------------------


class TestVtValueBool:
    def test_from_bool_true(self):
        v = VtValue.from_bool(True)
        assert v.to_python() is True

    def test_from_bool_false(self):
        v = VtValue.from_bool(False)
        assert v.to_python() is False

    def test_type_name_bool(self):
        v = VtValue.from_bool(True)
        assert "bool" in v.type_name.lower()


class TestVtValueInt:
    def test_from_int_positive(self):
        v = VtValue.from_int(42)
        result = v.to_python()
        assert result is not None
        assert int(result) == 42

    def test_from_int_zero(self):
        v = VtValue.from_int(0)
        assert int(v.to_python()) == 0

    def test_from_int_negative(self):
        v = VtValue.from_int(-100)
        assert int(v.to_python()) == -100

    def test_type_name_int(self):
        v = VtValue.from_int(1)
        assert "int" in v.type_name.lower()


class TestVtValueFloat:
    def test_from_float_positive(self):
        v = VtValue.from_float(3.14)
        result = v.to_python()
        assert abs(float(result) - 3.14) < 1e-5

    def test_from_float_zero(self):
        v = VtValue.from_float(0.0)
        assert abs(float(v.to_python())) < 1e-10

    def test_from_float_negative(self):
        v = VtValue.from_float(-1.5)
        assert abs(float(v.to_python()) - (-1.5)) < 1e-6

    def test_type_name_float(self):
        v = VtValue.from_float(1.0)
        assert "float" in v.type_name.lower() or "double" in v.type_name.lower()


class TestVtValueString:
    def test_from_string_simple(self):
        v = VtValue.from_string("hello")
        assert v.to_python() == "hello"

    def test_from_string_empty(self):
        v = VtValue.from_string("")
        assert v.to_python() == ""

    def test_from_string_with_spaces(self):
        v = VtValue.from_string("hello world")
        assert v.to_python() == "hello world"

    def test_type_name_string(self):
        v = VtValue.from_string("x")
        assert "string" in v.type_name.lower()


class TestVtValueToken:
    def test_from_token_simple(self):
        v = VtValue.from_token("Y")
        result = v.to_python()
        assert result == "Y"

    def test_from_token_up_axis(self):
        v = VtValue.from_token("Z")
        result = v.to_python()
        assert result == "Z"

    def test_type_name_token(self):
        v = VtValue.from_token("Y")
        assert "token" in v.type_name.lower()


class TestVtValueAsset:
    def test_from_asset_path(self):
        v = VtValue.from_asset("/path/to/scene.usda")
        result = v.to_python()
        # asset path returns string representation
        assert result is not None
        assert "scene.usda" in str(result)

    def test_type_name_asset(self):
        v = VtValue.from_asset("/path.usda")
        assert "asset" in v.type_name.lower()


class TestVtValueVec3f:
    def test_from_vec3f_basic(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        result = v.to_python()
        assert result is not None

    def test_from_vec3f_components(self):
        v = VtValue.from_vec3f(10.0, 20.0, 30.0)
        result = v.to_python()
        # Result should be tuple or list with 3 elements
        assert len(result) == 3
        assert abs(float(result[0]) - 10.0) < 1e-5
        assert abs(float(result[1]) - 20.0) < 1e-5
        assert abs(float(result[2]) - 30.0) < 1e-5

    def test_from_vec3f_zero(self):
        v = VtValue.from_vec3f(0.0, 0.0, 0.0)
        result = v.to_python()
        assert len(result) == 3
        for val in result:
            assert abs(float(val)) < 1e-10

    def test_type_name_vec3(self):
        v = VtValue.from_vec3f(1, 2, 3)
        assert "float3" in v.type_name.lower() or "vec" in v.type_name.lower()

    def test_repr_non_empty(self):
        v = VtValue.from_vec3f(1, 2, 3)
        r = repr(v)
        assert len(r) > 0


# ---------------------------------------------------------------------------
# ActionRecorder and RecordingGuard
# ---------------------------------------------------------------------------


class TestActionRecorder:
    def test_start_returns_recording_guard(self):
        rec = ActionRecorder("test-scope")
        guard = rec.start("create_sphere", "maya")
        assert isinstance(guard, RecordingGuard)
        guard.finish(success=True)

    def test_metrics_none_before_any_recording(self):
        rec = ActionRecorder("test-scope")
        assert rec.metrics("nonexistent") is None

    def test_metrics_after_one_success(self):
        rec = ActionRecorder("scope")
        guard = rec.start("render", "maya")
        guard.finish(success=True)

        m = rec.metrics("render")
        assert m is not None
        assert isinstance(m, ActionMetrics)

    def test_invocation_count_after_one(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m.invocation_count == 1

    def test_success_count_after_success(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_failure_count_after_failure(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=False)
        m = rec.metrics("op")
        assert m.failure_count == 1
        assert m.success_count == 0

    def test_mixed_success_failure_counts(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=True)
        rec.start("op", "maya").finish(success=True)
        rec.start("op", "maya").finish(success=False)
        m = rec.metrics("op")
        assert m.invocation_count == 3
        assert m.success_count == 2
        assert m.failure_count == 1

    def test_success_rate_all_success(self):
        rec = ActionRecorder("scope")
        for _ in range(4):
            rec.start("x", "maya").finish(success=True)
        m = rec.metrics("x")
        assert abs(m.success_rate() - 1.0) < 1e-6

    def test_success_rate_all_failure(self):
        rec = ActionRecorder("scope")
        for _ in range(3):
            rec.start("x", "maya").finish(success=False)
        m = rec.metrics("x")
        assert abs(m.success_rate() - 0.0) < 1e-6

    def test_success_rate_half(self):
        rec = ActionRecorder("scope")
        rec.start("x", "maya").finish(success=True)
        rec.start("x", "maya").finish(success=False)
        m = rec.metrics("x")
        assert abs(m.success_rate() - 0.5) < 1e-6

    def test_avg_duration_ms_non_negative(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_non_negative(self):
        rec = ActionRecorder("scope")
        for _ in range(10):
            rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m.p95_duration_ms >= 0.0

    def test_p99_duration_ms_non_negative(self):
        rec = ActionRecorder("scope")
        for _ in range(10):
            rec.start("op", "maya").finish(success=True)
        m = rec.metrics("op")
        assert m.p99_duration_ms >= 0.0

    def test_all_metrics_empty_initially(self):
        rec = ActionRecorder("scope")
        assert rec.all_metrics() == []

    def test_all_metrics_returns_list(self):
        rec = ActionRecorder("scope")
        rec.start("a", "maya").finish(success=True)
        rec.start("b", "maya").finish(success=True)
        all_m = rec.all_metrics()
        assert isinstance(all_m, list)
        assert len(all_m) == 2

    def test_reset_clears_all_metrics(self):
        rec = ActionRecorder("scope")
        rec.start("op", "maya").finish(success=True)
        assert len(rec.all_metrics()) == 1
        rec.reset()
        assert len(rec.all_metrics()) == 0
        assert rec.metrics("op") is None

    def test_multiple_actions_tracked_separately(self):
        rec = ActionRecorder("scope")
        rec.start("create", "maya").finish(success=True)
        rec.start("delete", "maya").finish(success=False)
        rec.start("create", "maya").finish(success=True)

        create_m = rec.metrics("create")
        delete_m = rec.metrics("delete")
        assert create_m.invocation_count == 2
        assert delete_m.invocation_count == 1


# ---------------------------------------------------------------------------
# RecordingGuard as context manager
# ---------------------------------------------------------------------------


class TestRecordingGuardContextManager:
    def test_context_manager_success_on_no_exception(self):
        rec = ActionRecorder("scope")
        with rec.start("op", "maya"):
            pass
        m = rec.metrics("op")
        assert m is not None
        assert m.invocation_count == 1
        assert m.success_count == 1

    def test_context_manager_failure_on_exception(self):
        rec = ActionRecorder("scope")
        with pytest.raises(RuntimeError), rec.start("op", "maya"):
            raise RuntimeError("test error")

        m = rec.metrics("op")
        assert m is not None
        assert m.invocation_count == 1
        assert m.failure_count == 1

    def test_context_manager_returns_guard(self):
        rec = ActionRecorder("scope")
        guard = rec.start("op", "maya")
        with guard as g:
            assert g is guard

    def test_context_manager_repr(self):
        rec = ActionRecorder("scope")
        guard = rec.start("op", "maya")
        r = repr(guard)
        assert len(r) > 0
        guard.finish(success=True)

    def test_finish_outside_context(self):
        rec = ActionRecorder("scope")
        guard = rec.start("direct", "maya")
        guard.finish(success=True)
        m = rec.metrics("direct")
        assert m.success_count == 1


# ---------------------------------------------------------------------------
# ActionMetrics properties
# ---------------------------------------------------------------------------


class TestActionMetricsProperties:
    def _make_metrics_for(self, action: str, count: int, success: bool = True) -> ActionMetrics:
        rec = ActionRecorder("scope")
        for _ in range(count):
            rec.start(action, "maya").finish(success=success)
        return rec.metrics(action)

    def test_action_name_property(self):
        m = self._make_metrics_for("my_action", 1)
        assert m.action_name == "my_action"

    def test_invocation_count_correct(self):
        m = self._make_metrics_for("op", 7)
        assert m.invocation_count == 7

    def test_success_count_property(self):
        m = self._make_metrics_for("op", 5, success=True)
        assert m.success_count == 5

    def test_failure_count_property(self):
        m = self._make_metrics_for("op", 3, success=False)
        assert m.failure_count == 3

    def test_repr_non_empty(self):
        m = self._make_metrics_for("op", 1)
        r = repr(m)
        assert len(r) > 0


# ---------------------------------------------------------------------------
# ScriptResult deep
# ---------------------------------------------------------------------------


class TestScriptResult:
    def _success_result(self) -> ScriptResult:
        return ScriptResult(
            success=True,
            execution_time_ms=42,
            output="Sphere created: sphere1",
            context={"object_name": "sphere1"},
        )

    def _error_result(self) -> ScriptResult:
        return ScriptResult(
            success=False,
            execution_time_ms=100,
            error="RuntimeError: invalid argument",
        )

    def test_success_flag_true(self):
        sr = self._success_result()
        assert sr.success is True

    def test_success_flag_false(self):
        sr = self._error_result()
        assert sr.success is False

    def test_output_field(self):
        sr = self._success_result()
        assert "sphere1" in sr.output

    def test_error_none_on_success(self):
        sr = self._success_result()
        assert sr.error is None

    def test_error_field_on_failure(self):
        sr = self._error_result()
        assert "RuntimeError" in sr.error

    def test_execution_time_ms_positive(self):
        sr = self._success_result()
        assert sr.execution_time_ms == 42

    def test_context_field(self):
        sr = self._success_result()
        assert sr.context.get("object_name") == "sphere1"

    def test_context_empty_by_default(self):
        sr = ScriptResult(success=True, execution_time_ms=0)
        assert sr.context == {}

    def test_output_none_by_default(self):
        sr = ScriptResult(success=True, execution_time_ms=0)
        assert sr.output is None

    def test_to_dict_has_success(self):
        sr = self._success_result()
        d = sr.to_dict()
        assert "success" in d
        assert d["success"] is True

    def test_to_dict_has_execution_time(self):
        sr = self._success_result()
        d = sr.to_dict()
        assert "execution_time_ms" in d
        assert d["execution_time_ms"] == 42

    def test_repr_non_empty(self):
        sr = self._success_result()
        r = repr(sr)
        assert len(r) > 0

    def test_minimal_construction(self):
        sr = ScriptResult(success=True, execution_time_ms=0)
        assert sr.success is True
        assert sr.execution_time_ms == 0
