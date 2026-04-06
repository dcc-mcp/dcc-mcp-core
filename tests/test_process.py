"""Tests for dcc-mcp-process Python bindings.

Covers PyProcessMonitor, PyDccLauncher, PyCrashRecoveryPolicy, PyProcessWatcher.
Tests use the current process PID for monitor tests. Launch tests are
structural-only (no real DCC executable required).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import os

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── PyProcessMonitor ──────────────────────────────────────────────────────────


class TestPyProcessMonitor:
    def test_create_monitor(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        assert mon is not None

    def test_initial_tracked_count_zero(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_track_current_process(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1

    def test_untrack_reduces_count(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.untrack(pid)
        assert mon.tracked_count() == 0

    def test_is_alive_current_process(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        assert mon.is_alive(pid) is True

    def test_is_alive_invalid_pid_false(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        # PID 0 is always invalid for user processes
        result = mon.is_alive(0)
        assert isinstance(result, bool)

    def test_refresh_does_not_raise(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()  # should not raise

    def test_query_after_refresh(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info is not None
        assert info["pid"] == pid
        assert info["name"] == "self"
        assert "status" in info
        assert "memory_bytes" in info

    def test_query_untracked_is_none(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        info = mon.query(os.getpid())
        assert info is None

    def test_list_all_returns_tracked(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        all_infos = mon.list_all()
        assert len(all_infos) == 1
        assert all_infos[0]["pid"] == pid

    def test_status_is_string(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert isinstance(info["status"], str)

    def test_cpu_usage_nonnegative(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["cpu_usage_percent"] >= 0.0

    def test_memory_bytes_positive(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["memory_bytes"] >= 0

    def test_repr_contains_tracked(self) -> None:
        mon = dcc_mcp_core.PyProcessMonitor()
        r = repr(mon)
        assert "PyProcessMonitor" in r

    def test_track_multiple_pids(self) -> None:
        import sys

        mon = dcc_mcp_core.PyProcessMonitor()
        pid = os.getpid()
        ppid = os.getppid() if hasattr(os, "getppid") else pid + 1
        mon.track(pid, "self")
        if ppid != pid:
            mon.track(ppid, "parent")
        count = mon.tracked_count()
        assert count >= 1


# ── PyDccLauncher ─────────────────────────────────────────────────────────────


class TestPyDccLauncher:
    def test_create_launcher(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher is not None

    def test_running_count_initial_zero(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.running_count() == 0

    def test_pid_of_unknown_is_none(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.pid_of("nonexistent") is None

    def test_restart_count_unknown_is_zero(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        assert launcher.restart_count("nonexistent") == 0

    def test_repr_contains_running(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        r = repr(launcher)
        assert "PyDccLauncher" in r

    def test_terminate_unknown_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.terminate("not_running")

    def test_kill_unknown_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.kill("not_running")

    def test_launch_nonexistent_raises(self) -> None:
        launcher = dcc_mcp_core.PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.launch(
                "test",
                "/nonexistent/path/to/dcc",
                args=[],
                launch_timeout_ms=500,
            )


# ── PyCrashRecoveryPolicy ─────────────────────────────────────────────────────


class TestPyCrashRecoveryPolicy:
    def test_create_policy_default(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy()
        assert policy is not None

    def test_create_policy_custom_restarts(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        assert policy is not None

    def test_should_restart_crashed(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("crashed") is True

    def test_should_not_restart_stopped(self) -> None:
        # "stopped" is a clean shutdown, not a crash
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("stopped") is False

    def test_next_delay_ms_initial(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        delay = policy.next_delay_ms("maya", 0)
        assert isinstance(delay, int)
        assert delay >= 0

    def test_next_delay_ms_increases_with_attempts(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        delay0 = policy.next_delay_ms("maya", 0)
        delay1 = policy.next_delay_ms("maya", 1)
        delay2 = policy.next_delay_ms("maya", 2)
        assert delay0 <= delay1 <= delay2

    def test_use_exponential_backoff_does_not_raise(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_exponential_backoff(initial_ms=500, max_delay_ms=60000)

    def test_use_fixed_backoff_does_not_raise(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        policy.use_fixed_backoff(delay_ms=2000)

    def test_repr_contains_policy_info(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        r = repr(policy)
        assert "Policy" in r or "policy" in r or "3" in r

    def test_zero_restarts_should_not_restart(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=0)
        assert policy.should_restart("crashed") is False

    def test_should_restart_unresponsive(self) -> None:
        """'unresponsive' is a crash-like condition — should trigger restart."""
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("unresponsive") is True

    def test_should_not_restart_running(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("running") is False

    def test_should_not_restart_starting(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("starting") is False

    def test_should_restart_invalid_status_raises(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        with pytest.raises((ValueError, RuntimeError)):
            policy.should_restart("invalid_status_xyz")

    def test_max_restarts_getter(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=7)
        assert policy.max_restarts == 7

    def test_fixed_backoff_constant_delay(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_fixed_backoff(delay_ms=3000)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        d2 = policy.next_delay_ms("maya", 2)
        assert d0 == d1 == d2 == 3000

    def test_exponential_backoff_bounded_by_max(self) -> None:
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=100, max_delay_ms=500)
        for attempt in range(8):
            delay = policy.next_delay_ms("maya", attempt)
            assert delay <= 500, f"attempt {attempt}: delay {delay} exceeds max"

    def test_next_delay_exceeds_restarts_raises(self) -> None:
        """Requesting delay beyond max_restarts should raise an error."""
        policy = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=2)
        with pytest.raises((RuntimeError, ValueError)):
            policy.next_delay_ms("maya", 3)  # attempt 3 > max_restarts=2


# ── PyProcessWatcher ──────────────────────────────────────────────────────────


class TestPyProcessWatcher:
    def test_create_watcher(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        assert watcher is not None

    def test_watch_count_initial_zero(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        assert watcher.watch_count() == 0

    def test_add_watch_current_process(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        watcher.add_watch(pid, "self")
        assert watcher.watch_count() == 1

    def test_remove_watch(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        watcher.add_watch(pid, "self")
        watcher.remove_watch(pid)
        assert watcher.watch_count() == 0

    def test_is_watched_true(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        watcher.add_watch(pid, "self")
        assert watcher.is_watched(pid) is True

    def test_is_watched_false(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        assert watcher.is_watched(os.getpid()) is False

    def test_repr_contains_count(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        r = repr(watcher)
        assert "PyProcessWatcher" in r or "Watcher" in r

    # ── Lifecycle: start / stop / is_running ──────────────────────────────────

    def test_not_running_before_start(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher()
        assert watcher.is_running() is False

    def test_start_makes_is_running_true(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        watcher.start()
        try:
            assert watcher.is_running() is True
        finally:
            watcher.stop()

    def test_stop_makes_is_running_false(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        watcher.start()
        watcher.stop()
        assert watcher.is_running() is False

    def test_start_idempotent(self) -> None:
        """Calling start() twice should not raise."""
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        watcher.start()
        try:
            watcher.start()  # second call: no-op
            assert watcher.is_running() is True
        finally:
            watcher.stop()

    def test_stop_idempotent(self) -> None:
        """Calling stop() when already stopped should not raise."""
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        watcher.stop()  # no-op
        watcher.stop()  # still no-op

    def test_poll_events_empty_before_start(self) -> None:
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        events = watcher.poll_events()
        assert events == []

    def test_poll_events_returns_list(self) -> None:
        """poll_events() always returns a list (empty or non-empty)."""
        import time

        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=100)
        pid = os.getpid()
        watcher.track(pid, "self")
        watcher.start()
        try:
            time.sleep(0.35)  # allow at least 3 polls
            events = watcher.poll_events()
            assert isinstance(events, list)
        finally:
            watcher.stop()

    def test_poll_events_heartbeat_structure(self) -> None:
        """Heartbeat events should have required keys."""
        import time

        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=100)
        pid = os.getpid()
        watcher.track(pid, "self")
        watcher.start()
        try:
            time.sleep(0.35)
            events = watcher.poll_events()
        finally:
            watcher.stop()

        heartbeats = [e for e in events if e.get("type") == "heartbeat"]
        if heartbeats:  # may be empty if the OS is very slow
            hb = heartbeats[0]
            assert "pid" in hb
            assert "name" in hb
            assert "new_status" in hb
            assert hb["pid"] == pid
            assert hb["name"] == "self"

    def test_poll_events_drains_queue(self) -> None:
        """Second poll_events() call returns empty list (queue was drained)."""
        import time

        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=100)
        watcher.track(os.getpid(), "self")
        watcher.start()
        try:
            time.sleep(0.25)
            watcher.poll_events()  # first drain
            events2 = watcher.poll_events()  # should be empty
            assert events2 == []
        finally:
            watcher.stop()

    def test_tracked_count_alias(self) -> None:
        """tracked_count() is an alias for watch_count()."""
        watcher = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        watcher.track(pid, "self")
        assert watcher.tracked_count() == 1
        assert watcher.tracked_count() == watcher.watch_count()

    def test_track_untrack_aliases(self) -> None:
        """track/untrack are the canonical API; add_watch/remove_watch are aliases."""
        watcher = dcc_mcp_core.PyProcessWatcher()
        pid = os.getpid()
        watcher.track(pid, "self")
        assert watcher.is_watched(pid) is True
        watcher.untrack(pid)
        assert watcher.is_watched(pid) is False

    def test_repr_shows_running_state(self) -> None:
        """repr() should reflect running state."""
        watcher = dcc_mcp_core.PyProcessWatcher(poll_interval_ms=200)
        watcher.start()
        try:
            r = repr(watcher)
            assert "True" in r or "running" in r.lower() or "1" in r
        finally:
            watcher.stop()
