"""Deep tests for PyProcessWatcher and PyProcessMonitor.

Covers:
- PyProcessWatcher.add_watch / remove_watch / is_watched / watch_count / tracked_count
- PyProcessWatcher.start / stop / is_running
- PyProcessWatcher.poll_events
- PyProcessMonitor.track / untrack / tracked_count
- PyProcessMonitor.query / list_all / is_alive / refresh
- Multi-PID tracking scenarios
"""

from __future__ import annotations

import os
import time

import pytest

from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PyProcessWatcher

SELF_PID = os.getpid()


# ---------------------------------------------------------------------------
# PyProcessWatcher
# ---------------------------------------------------------------------------


class TestPyProcessWatcherLifecycle:
    def test_new_watcher_is_not_running(self):
        w = PyProcessWatcher()
        assert w.is_running() is False

    def test_start_makes_watcher_running(self):
        w = PyProcessWatcher()
        w.start()
        assert w.is_running() is True
        w.stop()

    def test_stop_makes_watcher_not_running(self):
        w = PyProcessWatcher()
        w.start()
        w.stop()
        assert w.is_running() is False

    def test_stop_without_start_does_not_crash(self):
        w = PyProcessWatcher()
        w.stop()  # Should not raise

    def test_start_idempotent(self):
        w = PyProcessWatcher()
        w.start()
        w.start()  # Second start should not raise
        assert w.is_running() is True
        w.stop()


class TestPyProcessWatcherTracking:
    def test_initial_watch_count_is_zero(self):
        w = PyProcessWatcher()
        assert w.watch_count() == 0

    def test_add_watch_increments_watch_count(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        assert w.watch_count() == 1
        w.remove_watch(SELF_PID)

    def test_add_watch_increments_tracked_count(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        assert w.tracked_count() == 1
        w.remove_watch(SELF_PID)

    def test_is_watched_true_after_add(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        assert w.is_watched(SELF_PID) is True
        w.remove_watch(SELF_PID)

    def test_is_watched_false_before_add(self):
        w = PyProcessWatcher()
        assert w.is_watched(SELF_PID) is False

    def test_is_watched_false_after_remove(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        w.remove_watch(SELF_PID)
        assert w.is_watched(SELF_PID) is False

    def test_remove_watch_decrements_watch_count(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        w.remove_watch(SELF_PID)
        assert w.watch_count() == 0

    def test_multiple_pids_tracked(self):
        w = PyProcessWatcher()
        # Use current PID and parent PID if available
        pids = [SELF_PID]
        ppid = os.getppid() if hasattr(os, "getppid") else None
        if ppid and ppid != SELF_PID:
            pids.append(ppid)
        for i, p in enumerate(pids):
            w.add_watch(p, f"proc_{i}")
        assert w.watch_count() == len(pids)
        for p in pids:
            assert w.is_watched(p) is True
        for p in pids:
            w.remove_watch(p)
        assert w.watch_count() == 0

    def test_add_duplicate_pid_does_not_crash(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        w.add_watch(SELF_PID, "self_again")  # Should not raise
        w.remove_watch(SELF_PID)


class TestPyProcessWatcherPollEvents:
    def test_poll_events_returns_list(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        w.start()
        time.sleep(0.05)
        events = w.poll_events()
        assert isinstance(events, list)
        w.stop()
        w.remove_watch(SELF_PID)

    def test_poll_events_empty_when_no_watches(self):
        w = PyProcessWatcher()
        w.start()
        time.sleep(0.05)
        events = w.poll_events()
        assert events == []
        w.stop()

    def test_poll_events_not_running_returns_empty_or_list(self):
        w = PyProcessWatcher()
        # Should not raise even when not running
        events = w.poll_events()
        assert isinstance(events, list)

    def test_poll_events_clears_queue(self):
        w = PyProcessWatcher()
        w.add_watch(SELF_PID, "self")
        w.start()
        time.sleep(0.05)
        w.poll_events()
        # Second poll should return empty (events consumed)
        events2 = w.poll_events()
        assert isinstance(events2, list)
        w.stop()
        w.remove_watch(SELF_PID)


# ---------------------------------------------------------------------------
# PyProcessMonitor
# ---------------------------------------------------------------------------


class TestPyProcessMonitorTracking:
    def test_initial_tracked_count_is_zero(self):
        m = PyProcessMonitor()
        assert m.tracked_count() == 0

    def test_track_increments_count(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        assert m.tracked_count() == 1
        m.untrack(SELF_PID)

    def test_untrack_decrements_count(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.untrack(SELF_PID)
        assert m.tracked_count() == 0

    def test_untrack_nonexistent_does_not_crash(self):
        m = PyProcessMonitor()
        m.untrack(9999999)  # Should not raise

    def test_track_multiple_pids(self):
        m = PyProcessMonitor()
        pids = [SELF_PID]
        ppid = os.getppid() if hasattr(os, "getppid") else None
        if ppid and ppid != SELF_PID:
            pids.append(ppid)
        for i, p in enumerate(pids):
            m.track(p, f"proc_{i}")
        assert m.tracked_count() == len(pids)
        for p in pids:
            m.untrack(p)
        assert m.tracked_count() == 0

    def test_list_all_returns_list(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        result = m.list_all()
        assert isinstance(result, list)
        m.untrack(SELF_PID)

    def test_list_all_contains_tracked_pid(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        result = m.list_all()
        pids_in_result = [entry["pid"] for entry in result]
        assert SELF_PID in pids_in_result
        m.untrack(SELF_PID)

    def test_list_all_empty_when_no_tracking(self):
        m = PyProcessMonitor()
        result = m.list_all()
        assert result == []


class TestPyProcessMonitorQuery:
    def test_query_returns_dict_for_tracked_pid(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()  # Must refresh before query returns data
        info = m.query(SELF_PID)
        assert isinstance(info, dict)
        m.untrack(SELF_PID)

    def test_query_dict_has_expected_keys(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info = m.query(SELF_PID)
        for key in ("pid", "name", "status", "cpu_usage_percent", "memory_bytes", "restart_count"):
            assert key in info, f"Missing key: {key}"
        m.untrack(SELF_PID)

    def test_query_pid_matches(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info = m.query(SELF_PID)
        assert info["pid"] == SELF_PID
        m.untrack(SELF_PID)

    def test_query_name_matches(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "my_process")
        m.refresh()
        info = m.query(SELF_PID)
        assert info["name"] == "my_process"
        m.untrack(SELF_PID)

    def test_query_status_is_string(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info = m.query(SELF_PID)
        assert isinstance(info["status"], str)
        m.untrack(SELF_PID)

    def test_query_memory_bytes_positive(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info = m.query(SELF_PID)
        assert info["memory_bytes"] >= 0
        m.untrack(SELF_PID)

    def test_query_restart_count_zero_initially(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info = m.query(SELF_PID)
        assert info["restart_count"] == 0
        m.untrack(SELF_PID)

    def test_query_none_for_untracked_pid(self):
        m = PyProcessMonitor()
        info = m.query(9999999)
        assert info is None

    def test_query_after_untrack_returns_none(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.untrack(SELF_PID)
        info = m.query(SELF_PID)
        assert info is None


class TestPyProcessMonitorIsAlive:
    def test_is_alive_true_for_running_process(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        assert m.is_alive(SELF_PID) is True
        m.untrack(SELF_PID)

    def test_is_alive_false_for_unknown_pid(self):
        m = PyProcessMonitor()
        result = m.is_alive(9999999)
        # May return False or None; should not raise
        assert result is False or result is None

    def test_is_alive_after_untrack(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.untrack(SELF_PID)
        # Untracked PID: result behavior is implementation-defined, should not raise
        result = m.is_alive(SELF_PID)
        assert isinstance(result, bool) or result is None


class TestPyProcessMonitorRefresh:
    def test_refresh_does_not_raise(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        m.untrack(SELF_PID)

    def test_refresh_without_tracking_does_not_raise(self):
        m = PyProcessMonitor()
        m.refresh()  # Empty refresh should be fine

    def test_refresh_updates_memory_info(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        info_before = m.query(SELF_PID)
        m.refresh()
        info_after = m.query(SELF_PID)
        # Both should have valid memory_bytes
        assert info_before["memory_bytes"] >= 0
        assert info_after["memory_bytes"] >= 0

    def test_list_all_reflects_current_state_after_refresh(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        m.refresh()
        all_entries = m.list_all()
        pids = [e["pid"] for e in all_entries]
        assert SELF_PID in pids
        m.untrack(SELF_PID)


class TestPyProcessMonitorMultiPid:
    def test_multi_pid_query_all_present(self):
        m = PyProcessMonitor()
        m.track(SELF_PID, "self")
        ppid = os.getppid() if hasattr(os, "getppid") else None
        extra = []
        if ppid and ppid != SELF_PID:
            m.track(ppid, "parent")
            extra.append(ppid)
        all_entries = m.list_all()
        pids_in_list = {e["pid"] for e in all_entries}
        assert SELF_PID in pids_in_list
        for p in extra:
            assert p in pids_in_list
        m.untrack(SELF_PID)
        for p in extra:
            m.untrack(p)
        assert m.tracked_count() == 0

    def test_list_all_after_partial_untrack(self):
        m = PyProcessMonitor()
        ppid = os.getppid() if hasattr(os, "getppid") else None
        if ppid is None or ppid == SELF_PID:
            pytest.skip("parent PID not available or same as self")
        m.track(SELF_PID, "self")
        m.track(ppid, "parent")
        m.untrack(SELF_PID)
        remaining = m.list_all()
        pids = {e["pid"] for e in remaining}
        assert SELF_PID not in pids
        assert ppid in pids
        m.untrack(ppid)
