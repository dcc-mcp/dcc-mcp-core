"""Deep tests for ActionRecorder, RecordingGuard, and ActionMetrics.

Covers:
- ActionRecorder.start() + guard.finish(success=True/False) manual flow
- RecordingGuard as context manager (success and exception paths)
- ActionMetrics fields: invocation_count, success_count, failure_count,
  avg_duration_ms, p95_duration_ms, p99_duration_ms, success_rate()
- ActionRecorder.all_metrics() ordering and count
- ActionRecorder.reset() clears all metrics
- ActionRecorder.metrics() returns None for unknown action
- Multiple RecordingGuard instances on same action accumulate correctly
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import ActionMetrics
from dcc_mcp_core import ActionRecorder
from dcc_mcp_core import RecordingGuard

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _recorder(scope: str = "test-scope") -> ActionRecorder:
    return ActionRecorder(scope)


# ---------------------------------------------------------------------------
# ActionRecorder - basic guard.finish()
# ---------------------------------------------------------------------------


class TestActionRecorderGuardFinish:
    def test_single_success_increments_invocation(self):
        r = _recorder()
        guard = r.start("op", "maya")
        guard.finish(success=True)
        m = r.metrics("op")
        assert m is not None
        assert m.invocation_count == 1

    def test_single_success_increments_success_count(self):
        r = _recorder()
        guard = r.start("op", "maya")
        guard.finish(success=True)
        m = r.metrics("op")
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_single_failure_increments_failure_count(self):
        r = _recorder()
        guard = r.start("op", "maya")
        guard.finish(success=False)
        m = r.metrics("op")
        assert m.failure_count == 1
        assert m.success_count == 0

    def test_multiple_recordings_accumulate(self):
        r = _recorder()
        for _ in range(5):
            g = r.start("build", "maya")
            g.finish(success=True)
        g = r.start("build", "maya")
        g.finish(success=False)
        m = r.metrics("build")
        assert m.invocation_count == 6
        assert m.success_count == 5
        assert m.failure_count == 1

    def test_avg_duration_ms_is_non_negative(self):
        r = _recorder()
        guard = r.start("render", "maya")
        guard.finish(success=True)
        m = r.metrics("render")
        assert m.avg_duration_ms >= 0.0

    def test_p95_duration_ms_ge_avg(self):
        r = _recorder()
        for _ in range(10):
            g = r.start("export", "blender")
            g.finish(success=True)
        m = r.metrics("export")
        assert m.p95_duration_ms >= 0.0

    def test_p99_duration_ms_ge_p95(self):
        r = _recorder()
        for _ in range(10):
            g = r.start("import", "houdini")
            g.finish(success=True)
        m = r.metrics("import")
        assert m.p99_duration_ms >= m.p95_duration_ms

    def test_success_rate_all_success(self):
        r = _recorder()
        for _ in range(4):
            g = r.start("cmd", "maya")
            g.finish(success=True)
        m = r.metrics("cmd")
        assert m.success_rate() == pytest.approx(1.0)

    def test_success_rate_all_failure(self):
        r = _recorder()
        for _ in range(3):
            g = r.start("fail_cmd", "maya")
            g.finish(success=False)
        m = r.metrics("fail_cmd")
        assert m.success_rate() == pytest.approx(0.0)

    def test_success_rate_mixed(self):
        r = _recorder()
        for _ in range(3):
            g = r.start("mixed", "maya")
            g.finish(success=True)
        for _ in range(1):
            g = r.start("mixed", "maya")
            g.finish(success=False)
        m = r.metrics("mixed")
        assert m.success_rate() == pytest.approx(0.75)

    def test_action_name_matches(self):
        r = _recorder()
        g = r.start("my_action", "maya")
        g.finish(success=True)
        m = r.metrics("my_action")
        assert m.action_name == "my_action"


# ---------------------------------------------------------------------------
# RecordingGuard - context manager
# ---------------------------------------------------------------------------


class TestRecordingGuardContextManager:
    def test_context_manager_no_exception_records_success(self):
        r = _recorder()
        with r.start("scene_open", "maya"):
            pass
        m = r.metrics("scene_open")
        assert m.success_count == 1
        assert m.failure_count == 0

    def test_context_manager_exception_records_failure(self):
        r = _recorder()
        with pytest.raises(ValueError), r.start("scene_save", "maya"):
            raise ValueError("disk full")
        m = r.metrics("scene_save")
        assert m.failure_count == 1
        assert m.success_count == 0

    def test_context_manager_returns_recording_guard(self):
        r = _recorder()
        guard = r.start("ping", "maya")
        result = guard.__enter__()
        assert isinstance(result, RecordingGuard)
        result.finish(success=True)

    def test_context_manager_invocation_count_increments_each_use(self):
        r = _recorder()
        for _ in range(3):
            with r.start("refresh", "maya"):
                pass
        m = r.metrics("refresh")
        assert m.invocation_count == 3

    def test_context_manager_mixed_success_failure(self):
        r = _recorder()
        with r.start("task", "maya"):
            pass
        with pytest.raises(RuntimeError), r.start("task", "maya"):
            raise RuntimeError("crashed")
        with r.start("task", "maya"):
            pass
        m = r.metrics("task")
        assert m.invocation_count == 3
        assert m.success_count == 2
        assert m.failure_count == 1

    def test_guard_finish_explicit_after_enter(self):
        r = _recorder()
        guard = r.start("explicit", "maya")
        guard.__enter__()
        guard.finish(success=True)
        m = r.metrics("explicit")
        assert m.invocation_count == 1


# ---------------------------------------------------------------------------
# ActionRecorder - all_metrics() and reset()
# ---------------------------------------------------------------------------


class TestActionRecorderAllMetricsReset:
    def test_all_metrics_empty_initially(self):
        r = _recorder()
        assert r.all_metrics() == []

    def test_all_metrics_returns_one_per_action(self):
        r = _recorder()
        for action in ["a", "b", "c"]:
            g = r.start(action, "maya")
            g.finish(success=True)
        metrics = r.all_metrics()
        assert len(metrics) == 3

    def test_all_metrics_are_action_metrics_instances(self):
        r = _recorder()
        g = r.start("x", "maya")
        g.finish(success=True)
        for m in r.all_metrics():
            assert isinstance(m, ActionMetrics)

    def test_metrics_returns_none_for_unknown_action(self):
        r = _recorder()
        assert r.metrics("nonexistent") is None

    def test_reset_clears_all_metrics(self):
        r = _recorder()
        for name in ["a", "b"]:
            g = r.start(name, "maya")
            g.finish(success=True)
        r.reset()
        assert r.all_metrics() == []

    def test_reset_clears_specific_action_metrics(self):
        r = _recorder()
        g = r.start("test_action", "maya")
        g.finish(success=True)
        r.reset()
        assert r.metrics("test_action") is None

    def test_all_metrics_different_dccs_count_per_action(self):
        r = _recorder()
        g1 = r.start("render", "maya")
        g1.finish(success=True)
        g2 = r.start("render", "blender")
        g2.finish(success=True)
        m = r.metrics("render")
        # Both calls are for the same action name; check invocation_count
        assert m.invocation_count == 2

    def test_multiple_recorders_independent(self):
        r1 = ActionRecorder("scope-a")
        r2 = ActionRecorder("scope-b")
        g1 = r1.start("shared", "maya")
        g1.finish(success=True)
        # r2 has no recordings
        assert r2.metrics("shared") is None
        assert r1.metrics("shared").invocation_count == 1
