"""Deep tests for ToolRecorder/ToolMetrics/RecordingGuard/InputValidator/PyCrashRecoveryPolicy/PyProcessMonitor/PyProcessWatcher/DccInfo/DccCapabilities/SceneInfo/SceneStatistics/ScriptLanguage/PyDccLauncher.

All tests are pure-Python, no real DCC required.
"""

from __future__ import annotations

import json
import os
import sys
import time

import pytest

from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccInfo
from dcc_mcp_core import InputValidator
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import RecordingGuard
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ToolMetrics
from dcc_mcp_core import ToolRecorder

# ---------------------------------------------------------------------------
# TestActionRecorder
# ---------------------------------------------------------------------------


class TestActionRecorder:
    """Tests for ToolRecorder."""

    class TestHappyPath:
        def test_create_with_scope(self):
            rec = ToolRecorder("maya")
            assert rec is not None

        def test_all_metrics_empty_initially(self):
            rec = ToolRecorder("maya")
            assert rec.all_metrics() == []

        def test_start_returns_recording_guard(self):
            rec = ToolRecorder("maya")
            guard = rec.start("create_sphere", "maya")
            assert isinstance(guard, RecordingGuard)
            guard.finish(True)

        def test_finish_success_increments_success_count(self):
            rec = ToolRecorder("blender")
            guard = rec.start("add_mesh", "blender")
            guard.finish(True)
            m = rec.metrics("add_mesh")
            assert m is not None
            assert m.invocation_count == 1
            assert m.success_count == 1
            assert m.failure_count == 0

        def test_finish_failure_increments_failure_count(self):
            rec = ToolRecorder("houdini")
            guard = rec.start("render_frame", "houdini")
            guard.finish(False)
            m = rec.metrics("render_frame")
            assert m is not None
            assert m.invocation_count == 1
            assert m.success_count == 0
            assert m.failure_count == 1

        def test_multiple_recordings_aggregate(self):
            rec = ToolRecorder("maya")
            for _ in range(5):
                g = rec.start("create_sphere", "maya")
                g.finish(True)
            g = rec.start("create_sphere", "maya")
            g.finish(False)
            m = rec.metrics("create_sphere")
            assert m.invocation_count == 6
            assert m.success_count == 5
            assert m.failure_count == 1

        def test_all_metrics_returns_list_of_action_metrics(self):
            rec = ToolRecorder("maya")
            rec.start("action_a", "maya").finish(True)
            rec.start("action_b", "maya").finish(False)
            all_m = rec.all_metrics()
            assert isinstance(all_m, list)
            assert len(all_m) == 2
            names = {m.action_name for m in all_m}
            assert "action_a" in names
            assert "action_b" in names

        def test_metrics_returns_none_for_unknown_action(self):
            rec = ToolRecorder("maya")
            assert rec.metrics("nonexistent") is None

        def test_reset_clears_all_metrics(self):
            rec = ToolRecorder("maya")
            rec.start("action_a", "maya").finish(True)
            rec.start("action_b", "maya").finish(True)
            assert len(rec.all_metrics()) == 2
            rec.reset()
            assert rec.all_metrics() == []

        def test_multiple_actions_tracked_separately(self):
            rec = ToolRecorder("maya")
            for _ in range(3):
                rec.start("action_a", "maya").finish(True)
            for _ in range(2):
                rec.start("action_b", "maya").finish(False)
            assert rec.metrics("action_a").invocation_count == 3
            assert rec.metrics("action_b").invocation_count == 2

        def test_different_dcc_scopes(self):
            rec1 = ToolRecorder("maya")
            rec2 = ToolRecorder("blender")
            rec1.start("do_thing", "maya").finish(True)
            rec2.start("do_thing", "blender").finish(False)
            m1 = rec1.metrics("do_thing")
            m2 = rec2.metrics("do_thing")
            assert m1.success_count == 1
            assert m2.failure_count == 1

        def test_reset_allows_fresh_recording(self):
            rec = ToolRecorder("maya")
            rec.start("action", "maya").finish(True)
            rec.reset()
            rec.start("action", "maya").finish(False)
            m = rec.metrics("action")
            assert m.invocation_count == 1
            assert m.failure_count == 1


# ---------------------------------------------------------------------------
# TestActionMetrics
# ---------------------------------------------------------------------------


class TestActionMetrics:
    """Tests for ToolMetrics returned by ToolRecorder."""

    class TestHappyPath:
        def _make_metrics(self, successes: int, failures: int) -> ToolMetrics:
            rec = ToolRecorder("test_scope")
            for _ in range(successes):
                rec.start("measured_action", "maya").finish(True)
            for _ in range(failures):
                rec.start("measured_action", "maya").finish(False)
            return rec.metrics("measured_action")

        def test_action_name_correct(self):
            m = self._make_metrics(1, 0)
            assert m.action_name == "measured_action"

        def test_invocation_count(self):
            m = self._make_metrics(3, 2)
            assert m.invocation_count == 5

        def test_success_count(self):
            m = self._make_metrics(4, 1)
            assert m.success_count == 4

        def test_failure_count(self):
            m = self._make_metrics(2, 3)
            assert m.failure_count == 3

        def test_success_rate_all_success(self):
            m = self._make_metrics(5, 0)
            rate = m.success_rate()
            assert abs(rate - 1.0) < 1e-6

        def test_success_rate_all_failure(self):
            m = self._make_metrics(0, 4)
            rate = m.success_rate()
            assert abs(rate - 0.0) < 1e-6

        def test_success_rate_mixed(self):
            m = self._make_metrics(3, 1)
            rate = m.success_rate()
            assert abs(rate - 0.75) < 1e-6

        def test_avg_duration_ms_is_positive(self):
            m = self._make_metrics(3, 0)
            avg = m.avg_duration_ms
            assert isinstance(avg, float)
            assert avg >= 0.0

        def test_p95_duration_ms(self):
            m = self._make_metrics(10, 0)
            p95 = m.p95_duration_ms
            assert isinstance(p95, float)
            assert p95 >= 0.0

        def test_p99_duration_ms(self):
            m = self._make_metrics(10, 0)
            p99 = m.p99_duration_ms
            assert isinstance(p99, float)
            assert p99 >= 0.0

        def test_p99_ge_p95_ge_avg(self):
            m = self._make_metrics(20, 0)
            assert m.p99_duration_ms >= m.p95_duration_ms >= 0.0

        def test_repr_contains_action_name(self):
            m = self._make_metrics(2, 1)
            r = repr(m)
            assert "measured_action" in r

        def test_all_metrics_returns_action_metrics_instances(self):
            rec = ToolRecorder("maya")
            rec.start("act1", "maya").finish(True)
            rec.start("act2", "maya").finish(False)
            for m in rec.all_metrics():
                assert isinstance(m, ToolMetrics)
                assert m.invocation_count >= 1


# ---------------------------------------------------------------------------
# TestRecordingGuard
# ---------------------------------------------------------------------------


class TestRecordingGuard:
    """Tests for RecordingGuard returned by ToolRecorder.start()."""

    class TestHappyPath:
        def test_finish_true_records_success(self):
            rec = ToolRecorder("maya")
            guard = rec.start("act", "maya")
            guard.finish(True)
            assert rec.metrics("act").success_count == 1

        def test_finish_false_records_failure(self):
            rec = ToolRecorder("maya")
            guard = rec.start("act", "maya")
            guard.finish(False)
            assert rec.metrics("act").failure_count == 1

        def test_guard_is_different_per_start(self):
            rec = ToolRecorder("maya")
            g1 = rec.start("act", "maya")
            g2 = rec.start("act", "maya")
            assert g1 is not g2
            g1.finish(True)
            g2.finish(False)
            m = rec.metrics("act")
            assert m.success_count == 1
            assert m.failure_count == 1


# ---------------------------------------------------------------------------
# TestInputValidator
# ---------------------------------------------------------------------------


class TestInputValidator:
    """Tests for InputValidator."""

    class TestHappyPath:
        def test_empty_validator_accepts_anything(self):
            iv = InputValidator()
            ok, err = iv.validate(json.dumps({"x": 1}))
            assert ok is True
            assert err is None

        def test_require_string_accepts_valid_string(self):
            iv = InputValidator()
            iv.require_string("name", 256, 1)
            ok, err = iv.validate(json.dumps({"name": "sphere"}))
            assert ok is True
            assert err is None

        def test_require_number_accepts_valid_float(self):
            iv = InputValidator()
            iv.require_number("radius", 0.0, 1000.0)
            ok, _err = iv.validate(json.dumps({"radius": 5.0}))
            assert ok is True

        def test_require_number_accepts_int(self):
            iv = InputValidator()
            iv.require_number("count", 0.0, 100.0)
            ok, _err = iv.validate(json.dumps({"count": 10}))
            assert ok is True

        def test_forbid_substrings_accepts_clean_string(self):
            iv = InputValidator()
            iv.require_string("path", 512, 1)
            iv.forbid_substrings("path", ["../", "//"])
            ok, _err = iv.validate(json.dumps({"path": "/valid/path"}))
            assert ok is True

        def test_multiple_fields_all_valid(self):
            iv = InputValidator()
            iv.require_string("name", 128, 1)
            iv.require_number("radius", 0.0, 100.0)
            ok, _err = iv.validate(json.dumps({"name": "sphere", "radius": 2.5}))
            assert ok is True

    class TestErrorPath:
        def test_forbid_substring_rejects_double_dot(self):
            iv = InputValidator()
            iv.require_string("path", 512, 1)
            iv.forbid_substrings("path", [".."])
            ok, err = iv.validate(json.dumps({"path": "../evil"}))
            assert ok is False
            assert err is not None
            assert ".." in err

        def test_forbid_substring_rejects_double_slash(self):
            iv = InputValidator()
            iv.require_string("url", 512, 1)
            iv.forbid_substrings("url", ["//"])
            ok, _err = iv.validate(json.dumps({"url": "http//evil"}))
            assert ok is False

        def test_require_string_rejects_wrong_type(self):
            iv = InputValidator()
            iv.require_string("name", 256, 1)
            ok, err = iv.validate(json.dumps({"name": 123}))
            assert ok is False
            assert err is not None

        def test_require_number_rejects_string(self):
            iv = InputValidator()
            iv.require_number("radius", 0.0, 100.0)
            ok, _err = iv.validate(json.dumps({"radius": "not_a_number"}))
            assert ok is False

        def test_max_length_exceeded(self):
            iv = InputValidator()
            iv.require_string("name", 5, 1)
            ok, _err = iv.validate(json.dumps({"name": "toolongstring"}))
            assert ok is False

        def test_min_length_not_met(self):
            iv = InputValidator()
            iv.require_string("name", 256, 3)
            ok, _err = iv.validate(json.dumps({"name": "ab"}))
            assert ok is False

        def test_out_of_range_number_rejected(self):
            iv = InputValidator()
            iv.require_number("val", 0.0, 10.0)
            ok, _err = iv.validate(json.dumps({"val": 100.0}))
            assert ok is False

        def test_invalid_json_raises(self):
            iv = InputValidator()
            with pytest.raises((RuntimeError, ValueError)):
                iv.validate("not valid json {{")

        def test_validate_returns_tuple(self):
            iv = InputValidator()
            result = iv.validate(json.dumps({}))
            assert isinstance(result, tuple)
            assert len(result) == 2

        def test_forbidden_substring_in_multiple_fields(self):
            iv = InputValidator()
            iv.require_string("a", 256, 1)
            iv.require_string("b", 256, 1)
            iv.forbid_substrings("a", ["BAD"])
            iv.forbid_substrings("b", ["EVIL"])
            # a field ok, b field has forbidden
            ok, _err = iv.validate(json.dumps({"a": "good", "b": "EVIL_value"}))
            assert ok is False


# ---------------------------------------------------------------------------
# TestPyCrashRecoveryPolicy
# ---------------------------------------------------------------------------


class TestPyCrashRecoveryPolicy:
    """Tests for PyCrashRecoveryPolicy."""

    class TestHappyPath:
        def test_default_max_restarts(self):
            p = PyCrashRecoveryPolicy()
            assert p.max_restarts == 3

        def test_custom_max_restarts(self):
            p = PyCrashRecoveryPolicy(max_restarts=5)
            assert p.max_restarts == 5

        def test_should_restart_crashed(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            assert p.should_restart("crashed") is True

        def test_should_restart_unresponsive(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            assert p.should_restart("unresponsive") is True

        def test_should_not_restart_running(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            assert p.should_restart("running") is False

        def test_should_not_restart_stopped(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            assert p.should_restart("stopped") is False

        def test_should_not_restart_when_max_restarts_zero(self):
            p = PyCrashRecoveryPolicy(max_restarts=0)
            assert p.should_restart("crashed") is False

        def test_fixed_backoff_constant_delay(self):
            p = PyCrashRecoveryPolicy(max_restarts=5)
            p.use_fixed_backoff(500)
            assert p.next_delay_ms("maya", 0) == 500
            assert p.next_delay_ms("maya", 2) == 500
            assert p.next_delay_ms("maya", 4) == 500

        def test_exponential_backoff_grows(self):
            p = PyCrashRecoveryPolicy(max_restarts=5)
            p.use_exponential_backoff(1000, 30000)
            d0 = p.next_delay_ms("blender", 0)
            d1 = p.next_delay_ms("blender", 1)
            d2 = p.next_delay_ms("blender", 2)
            assert d0 == 1000
            assert d1 > d0
            assert d2 > d1

        def test_exponential_backoff_capped_at_max(self):
            p = PyCrashRecoveryPolicy(max_restarts=10)
            p.use_exponential_backoff(1000, 3000)
            d_large = p.next_delay_ms("houdini", 9)
            assert d_large <= 3000

        def test_repr_contains_max_restarts(self):
            p = PyCrashRecoveryPolicy(max_restarts=7)
            assert "7" in repr(p)

        def test_switch_from_exponential_to_fixed(self):
            p = PyCrashRecoveryPolicy(max_restarts=5)
            p.use_exponential_backoff(1000, 30000)
            p.use_fixed_backoff(200)
            assert p.next_delay_ms("maya", 1) == 200

    class TestErrorPath:
        def test_next_delay_ms_exceeds_max_raises(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            p.use_fixed_backoff(100)
            with pytest.raises(RuntimeError, match="exceeded max restarts"):
                p.next_delay_ms("maya", 10)

        def test_invalid_status_raises_value_error(self):
            p = PyCrashRecoveryPolicy(max_restarts=3)
            with pytest.raises(ValueError):
                p.should_restart("invalid_status")

        def test_max_restarts_zero_next_delay_raises(self):
            p = PyCrashRecoveryPolicy(max_restarts=0)
            p.use_fixed_backoff(100)
            with pytest.raises(RuntimeError):
                p.next_delay_ms("maya", 0)


# ---------------------------------------------------------------------------
# TestPyProcessMonitor
# ---------------------------------------------------------------------------


class TestPyProcessMonitor:
    """Tests for PyProcessMonitor."""

    class TestHappyPath:
        def test_create(self):
            pm = PyProcessMonitor()
            assert pm is not None

        def test_tracked_count_initially_zero(self):
            pm = PyProcessMonitor()
            assert pm.tracked_count() == 0

        def test_list_all_initially_empty(self):
            pm = PyProcessMonitor()
            assert pm.list_all() == []

        def test_track_current_process(self):
            pm = PyProcessMonitor()
            pm.track(os.getpid(), "self")
            assert pm.tracked_count() == 1

        def test_is_alive_current_process(self):
            pm = PyProcessMonitor()
            assert pm.is_alive(os.getpid()) is True

        def test_is_alive_nonexistent_pid(self):
            pm = PyProcessMonitor()
            assert pm.is_alive(99999999) is False

        def test_refresh_then_list_all_has_entry(self):
            pm = PyProcessMonitor()
            pid = os.getpid()
            pm.track(pid, "self")
            pm.refresh()
            entries = pm.list_all()
            assert len(entries) == 1
            entry = entries[0]
            assert entry["pid"] == pid
            assert entry["name"] == "self"
            assert "status" in entry
            assert "memory_bytes" in entry

        def test_query_before_refresh_returns_none(self):
            pm = PyProcessMonitor()
            pid = os.getpid()
            pm.track(pid, "self")
            # query without refresh returns None
            result = pm.query(pid)
            assert result is None

        def test_query_after_refresh_returns_dict(self):
            pm = PyProcessMonitor()
            pid = os.getpid()
            pm.track(pid, "self")
            pm.refresh()
            result = pm.query(pid)
            assert result is not None
            assert isinstance(result, dict)
            assert result["pid"] == pid

        def test_untrack_removes_from_tracked(self):
            pm = PyProcessMonitor()
            pid = os.getpid()
            pm.track(pid, "self")
            assert pm.tracked_count() == 1
            pm.untrack(pid)
            assert pm.tracked_count() == 0

        def test_is_alive_works_without_track(self):
            pm = PyProcessMonitor()
            # is_alive performs a fresh OS query - no track required
            assert pm.is_alive(os.getpid()) is True

        def test_track_multiple_processes(self):
            pm = PyProcessMonitor()
            pm.track(os.getpid(), "self")
            pm.track(1, "init")  # PID 1 exists on Linux, may not on Windows
            # Just verify tracked_count >= 1
            assert pm.tracked_count() >= 1
            pm.untrack(os.getpid())

        def test_list_all_entry_keys(self):
            pm = PyProcessMonitor()
            pm.track(os.getpid(), "self")
            pm.refresh()
            entry = pm.list_all()[0]
            for key in ["pid", "name", "status", "memory_bytes", "restart_count"]:
                assert key in entry

        def test_memory_bytes_positive(self):
            pm = PyProcessMonitor()
            pm.track(os.getpid(), "self")
            pm.refresh()
            entry = pm.list_all()[0]
            assert entry["memory_bytes"] > 0

        def test_restart_count_zero_on_fresh_track(self):
            pm = PyProcessMonitor()
            pm.track(os.getpid(), "self")
            pm.refresh()
            entry = pm.list_all()[0]
            assert entry["restart_count"] == 0


# ---------------------------------------------------------------------------
# TestPyProcessWatcher
# ---------------------------------------------------------------------------


class TestPyProcessWatcher:
    """Tests for PyProcessWatcher (background polling watcher)."""

    class TestHappyPath:
        def test_create_default(self):
            w = PyProcessWatcher()
            assert w is not None

        def test_create_custom_interval(self):
            w = PyProcessWatcher(poll_interval_ms=200)
            assert w is not None

        def test_not_running_initially(self):
            w = PyProcessWatcher()
            assert w.is_running() is False

        def test_tracked_count_zero_initially(self):
            w = PyProcessWatcher()
            assert w.tracked_count() == 0

        def test_watch_count_alias(self):
            w = PyProcessWatcher()
            assert w.watch_count() == 0

        def test_track_increments_count(self):
            w = PyProcessWatcher()
            w.track(os.getpid(), "self")
            assert w.tracked_count() == 1
            w.untrack(os.getpid())

        def test_add_watch_alias(self):
            w = PyProcessWatcher()
            w.add_watch(os.getpid(), "self")
            assert w.tracked_count() == 1
            w.remove_watch(os.getpid())

        def test_is_watched_after_track(self):
            w = PyProcessWatcher()
            pid = os.getpid()
            w.track(pid, "self")
            assert w.is_watched(pid) is True
            w.untrack(pid)

        def test_is_not_watched_before_track(self):
            w = PyProcessWatcher()
            assert w.is_watched(os.getpid()) is False

        def test_start_sets_is_running(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            assert w.is_running() is True
            w.stop()

        def test_stop_clears_is_running(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            w.stop()
            assert w.is_running() is False

        def test_poll_events_returns_list(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            time.sleep(0.4)
            events = w.poll_events()
            w.stop()
            assert isinstance(events, list)

        def test_poll_events_has_heartbeat_type(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            time.sleep(0.5)
            events = w.poll_events()
            w.stop()
            types = {e.get("type") for e in events}
            assert "heartbeat" in types

        def test_start_is_idempotent(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            w.start()  # should be no-op
            assert w.is_running() is True
            w.stop()

        def test_stop_is_idempotent(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.stop()  # no-op if not running
            w.stop()  # again no-op
            assert w.is_running() is False

        def test_repr_includes_class_name(self):
            w = PyProcessWatcher()
            assert "ProcessWatcher" in repr(w) or "Watcher" in repr(w)

        def test_untrack_decrements_count(self):
            w = PyProcessWatcher()
            pid = os.getpid()
            w.track(pid, "self")
            assert w.tracked_count() == 1
            w.untrack(pid)
            assert w.tracked_count() == 0

        def test_poll_events_drains_queue(self):
            w = PyProcessWatcher(poll_interval_ms=100)
            w.track(os.getpid(), "self")
            w.start()
            time.sleep(0.4)
            events1 = w.poll_events()
            events2 = w.poll_events()
            w.stop()
            assert len(events1) > 0
            # second drain may be empty or small
            assert isinstance(events2, list)


# ---------------------------------------------------------------------------
# TestDccInfo
# ---------------------------------------------------------------------------


class TestDccInfo:
    """Tests for DccInfo."""

    class TestHappyPath:
        def test_minimal_construction(self):
            info = DccInfo(
                dcc_type="maya",
                version="2024.2",
                platform="windows",
                pid=1234,
            )
            assert info.dcc_type == "maya"
            assert info.version == "2024.2"
            assert info.platform == "windows"
            assert info.pid == 1234

        def test_python_version_optional(self):
            info = DccInfo(
                dcc_type="blender",
                version="4.1",
                platform="linux",
                pid=5678,
                python_version="3.11.0",
            )
            assert info.python_version == "3.11.0"

        def test_python_version_default_none(self):
            info = DccInfo(
                dcc_type="maya",
                version="2024",
                platform="windows",
                pid=100,
            )
            assert info.python_version is None

        def test_metadata_optional_dict(self):
            meta = {"build": "release", "patch": "3"}
            info = DccInfo(
                dcc_type="houdini",
                version="20.5",
                platform="linux",
                pid=9000,
                metadata=meta,
            )
            assert info.metadata == meta

        def test_metadata_default_none(self):
            info = DccInfo(
                dcc_type="maya",
                version="2024",
                platform="windows",
                pid=100,
            )
            # metadata defaults to empty dict when not provided
            assert info.metadata is None or info.metadata == {}

        def test_to_dict_returns_dict(self):
            info = DccInfo(
                dcc_type="maya",
                version="2024.2",
                platform="windows",
                pid=1234,
            )
            d = info.to_dict()
            assert isinstance(d, dict)
            assert d["dcc_type"] == "maya"
            assert d["version"] == "2024.2"
            assert d["platform"] == "windows"
            assert d["pid"] == 1234

        def test_repr_contains_dcc_type(self):
            info = DccInfo(
                dcc_type="unreal",
                version="5.3",
                platform="windows",
                pid=42,
            )
            assert "unreal" in repr(info)

        def test_various_dcc_types(self):
            for dcc in ["maya", "blender", "houdini", "unreal", "unity", "3dsmax"]:
                info = DccInfo(dcc_type=dcc, version="1.0", platform="windows", pid=1)
                assert info.dcc_type == dcc

        def test_to_dict_with_metadata(self):
            meta = {"key": "value"}
            info = DccInfo(
                dcc_type="maya",
                version="2024",
                platform="windows",
                pid=10,
                metadata=meta,
            )
            d = info.to_dict()
            assert "metadata" in d


# ---------------------------------------------------------------------------
# TestDccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilities:
    """Tests for DccCapabilities."""

    class TestHappyPath:
        def test_default_construction(self):
            cap = DccCapabilities()
            assert cap is not None

        def test_all_false_by_default(self):
            cap = DccCapabilities()
            assert cap.scene_info is False
            assert cap.snapshot is False
            assert cap.undo_redo is False
            assert cap.progress_reporting is False
            assert cap.file_operations is False
            assert cap.selection is False

        def test_explicit_true_fields(self):
            cap = DccCapabilities(
                scene_info=True,
                snapshot=True,
                undo_redo=True,
            )
            assert cap.scene_info is True
            assert cap.snapshot is True
            assert cap.undo_redo is True

        def test_script_languages_default(self):
            cap = DccCapabilities()
            # default is Ellipsis (not set) - check it exists without error
            _ = cap.script_languages

        def test_script_languages_with_list(self):
            cap = DccCapabilities(script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL])
            langs = cap.script_languages
            assert langs is not None

        def test_extensions_default_none(self):
            cap = DccCapabilities()
            # extensions defaults to None or empty when not provided
            assert cap.extensions is None or cap.extensions == {}

        def test_extensions_with_dict(self):
            cap = DccCapabilities(extensions={"ext_a": True, "ext_b": False})
            assert cap.extensions is not None

        def test_repr_works(self):
            cap = DccCapabilities(scene_info=True)
            r = repr(cap)
            assert isinstance(r, str)

        def test_file_operations_flag(self):
            cap = DccCapabilities(file_operations=True)
            assert cap.file_operations is True

        def test_selection_flag(self):
            cap = DccCapabilities(selection=True)
            assert cap.selection is True

        def test_progress_reporting_flag(self):
            cap = DccCapabilities(progress_reporting=True)
            assert cap.progress_reporting is True


# ---------------------------------------------------------------------------
# TestScriptLanguage
# ---------------------------------------------------------------------------


class TestScriptLanguage:
    """Tests for ScriptLanguage enum."""

    class TestHappyPath:
        def test_python_variant(self):
            lang = ScriptLanguage.PYTHON
            assert lang is not None

        def test_mel_variant(self):
            lang = ScriptLanguage.MEL
            assert lang is not None

        def test_hscript_variant(self):
            assert ScriptLanguage.HSCRIPT is not None

        def test_maxscript_variant(self):
            assert ScriptLanguage.MAXSCRIPT is not None

        def test_lua_variant(self):
            assert ScriptLanguage.LUA is not None

        def test_blueprint_variant(self):
            assert ScriptLanguage.BLUEPRINT is not None

        def test_csharp_variant(self):
            assert ScriptLanguage.CSHARP is not None

        def test_vex_variant(self):
            assert ScriptLanguage.VEX is not None

        def test_all_variants_distinct(self):
            variants = [
                ScriptLanguage.PYTHON,
                ScriptLanguage.MEL,
                ScriptLanguage.HSCRIPT,
                ScriptLanguage.MAXSCRIPT,
                ScriptLanguage.LUA,
                ScriptLanguage.BLUEPRINT,
                ScriptLanguage.CSHARP,
                ScriptLanguage.VEX,
            ]
            # All should have distinct repr strings
            reprs = [repr(v) for v in variants]
            assert len(set(reprs)) == len(reprs)

        def test_can_put_in_list(self):
            langs = [ScriptLanguage.PYTHON, ScriptLanguage.MEL]
            cap = DccCapabilities(script_languages=langs)
            _ = cap.script_languages


# ---------------------------------------------------------------------------
# TestSceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatistics:
    """Tests for SceneStatistics."""

    class TestHappyPath:
        def test_default_construction(self):
            stats = SceneStatistics()
            assert stats is not None

        def test_all_fields_default_zero(self):
            stats = SceneStatistics()
            for field in [
                "object_count",
                "polygon_count",
                "vertex_count",
                "material_count",
                "texture_count",
                "light_count",
                "camera_count",
            ]:
                val = getattr(stats, field)
                assert val == 0 or val is None

        def test_explicit_values(self):
            stats = SceneStatistics(
                object_count=100,
                polygon_count=50000,
                vertex_count=60000,
                material_count=20,
                texture_count=40,
                light_count=5,
                camera_count=3,
            )
            assert stats.object_count == 100
            assert stats.polygon_count == 50000
            assert stats.vertex_count == 60000
            assert stats.material_count == 20
            assert stats.texture_count == 40
            assert stats.light_count == 5
            assert stats.camera_count == 3

        def test_repr_works(self):
            stats = SceneStatistics(object_count=10)
            r = repr(stats)
            assert isinstance(r, str)


# ---------------------------------------------------------------------------
# TestSceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfo:
    """Tests for SceneInfo."""

    class TestHappyPath:
        def test_default_construction(self):
            info = SceneInfo()
            assert info is not None

        def test_modified_defaults_false(self):
            info = SceneInfo()
            assert info.modified is False

        def test_explicit_file_path(self):
            info = SceneInfo(file_path="/project/scene.ma")
            assert info.file_path == "/project/scene.ma"

        def test_explicit_name(self):
            info = SceneInfo(name="my_scene")
            assert info.name == "my_scene"

        def test_explicit_fps(self):
            info = SceneInfo(fps=24.0)
            assert info.fps == 24.0

        def test_explicit_frame_range(self):
            info = SceneInfo(frame_range=(1, 250))
            assert info.frame_range == (1, 250)

        def test_explicit_current_frame(self):
            info = SceneInfo(current_frame=100)
            assert info.current_frame == 100

        def test_explicit_up_axis(self):
            info = SceneInfo(up_axis="Y")
            assert info.up_axis == "Y"

        def test_explicit_units(self):
            info = SceneInfo(units="cm")
            assert info.units == "cm"

        def test_explicit_format(self):
            info = SceneInfo(format="maya_ascii")
            assert info.format == "maya_ascii"

        def test_modified_flag(self):
            info = SceneInfo(modified=True)
            assert info.modified is True

        def test_statistics_field(self):
            stats = SceneStatistics(object_count=5)
            info = SceneInfo(statistics=stats)
            assert info.statistics is not None
            assert info.statistics.object_count == 5

        def test_metadata_field(self):
            info = SceneInfo(metadata={"renderer": "arnold"})
            assert info.metadata == {"renderer": "arnold"}

        def test_repr_works(self):
            info = SceneInfo(name="test_scene")
            r = repr(info)
            assert isinstance(r, str)

        def test_empty_construction(self):
            info = SceneInfo()
            # All optional fields should be accessible without error
            _ = info.file_path
            _ = info.name
            _ = info.fps
            _ = info.current_frame
            _ = info.frame_range
            _ = info.up_axis
            _ = info.units
            _ = info.format
            _ = info.statistics
            _ = info.metadata


# ---------------------------------------------------------------------------
# TestPyDccLauncher
# ---------------------------------------------------------------------------


class TestPyDccLauncher:
    """Tests for PyDccLauncher."""

    class TestHappyPath:
        def test_create(self):
            launcher = PyDccLauncher()
            assert launcher is not None

        def test_running_count_initially_zero(self):
            launcher = PyDccLauncher()
            assert launcher.running_count() == 0

        def test_launch_short_process(self):
            launcher = PyDccLauncher()
            result = launcher.launch(
                name="test_proc",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(5)"],
            )
            assert isinstance(result, dict)
            assert "pid" in result
            assert result["name"] == "test_proc"
            assert result["pid"] > 0
            # Cleanup
            launcher.kill("test_proc")

        def test_running_count_after_launch(self):
            launcher = PyDccLauncher()
            launcher.launch(
                name="count_test",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(5)"],
            )
            assert launcher.running_count() == 1
            launcher.kill("count_test")

        def test_pid_of_launched_process(self):
            launcher = PyDccLauncher()
            result = launcher.launch(
                name="pid_test",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(5)"],
            )
            pid = launcher.pid_of("pid_test")
            assert pid == result["pid"]
            launcher.kill("pid_test")

        def test_restart_count_initially_zero(self):
            launcher = PyDccLauncher()
            launcher.launch(
                name="restart_test",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(5)"],
            )
            assert launcher.restart_count("restart_test") == 0
            launcher.kill("restart_test")

        def test_terminate_stops_process(self):
            launcher = PyDccLauncher()
            launcher.launch(
                name="term_test",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(10)"],
            )
            launcher.terminate("term_test")
            time.sleep(0.3)
            assert launcher.running_count() == 0

        def test_kill_stops_process(self):
            launcher = PyDccLauncher()
            launcher.launch(
                name="kill_test",
                executable=sys.executable,
                args=["-c", "import time; time.sleep(10)"],
            )
            launcher.kill("kill_test")
            time.sleep(0.3)
            assert launcher.running_count() == 0

        def test_launch_with_custom_args(self):
            launcher = PyDccLauncher()
            result = launcher.launch(
                name="args_test",
                executable=sys.executable,
                args=["-c", "print('hello'); import time; time.sleep(5)"],
            )
            assert result["pid"] > 0
            launcher.kill("args_test")

        def test_launch_no_args(self):
            launcher = PyDccLauncher()
            result = launcher.launch(
                name="noargs_test",
                executable=sys.executable,
            )
            # Python with no script exits quickly
            time.sleep(0.5)
            # Don't assert running because it may have exited already
            assert result["name"] == "noargs_test"

    class TestErrorPath:
        def test_kill_nonexistent_raises(self):
            launcher = PyDccLauncher()
            with pytest.raises(RuntimeError, match="not running"):
                launcher.kill("ghost")

        def test_terminate_nonexistent_raises(self):
            launcher = PyDccLauncher()
            with pytest.raises(RuntimeError, match="not running"):
                launcher.terminate("ghost")

        def test_pid_of_nonexistent_returns_none(self):
            launcher = PyDccLauncher()
            result = launcher.pid_of("ghost")
            assert result is None

        def test_restart_count_nonexistent_returns_zero(self):
            launcher = PyDccLauncher()
            result = launcher.restart_count("ghost")
            assert result == 0

        def test_launch_invalid_executable_raises(self):
            launcher = PyDccLauncher()
            with pytest.raises((OSError, RuntimeError)):
                launcher.launch(
                    name="invalid_exe",
                    executable="/nonexistent/path/to/executable",
                    launch_timeout_ms=500,
                )
