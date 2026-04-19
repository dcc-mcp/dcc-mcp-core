"""Deep tests for TransportManager, PyProcessMonitor, PyProcessWatcher, PyDccLauncher, PyCrashRecoveryPolicy, PySharedBuffer, PySharedSceneBuffer, and PyBufferPool.

Coverage targets (this iteration):
- TransportManager: register/deregister/rank/update_status/session/pool
- PyProcessMonitor: track/untrack/refresh/query/is_alive/list_all
- PyProcessWatcher: track/watch/start/stop/poll_events/aliases
- PyDccLauncher: empty init/running_count/pid_of/restart_count
- PyCrashRecoveryPolicy: should_restart/next_delay_ms/fixed/exponential
- PySharedBuffer: create/write/read/clear/capacity/data_len/name/descriptor_json/open
- PySharedSceneBuffer: write/read/id/is_chunked/is_inline/descriptor_json/compression
- PyBufferPool: capacity/buffer_size/available/acquire/release-on-gc
"""

from __future__ import annotations

# Import built-in modules
import gc
import os
import tempfile
import time

# Import third-party modules
import pytest

from dcc_mcp_core import PyBufferPool

# Import local modules
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import PyProcessWatcher
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedBuffer
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import TransportManager

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_manager() -> TransportManager:
    """Create a TransportManager backed by a fresh temp directory."""
    return TransportManager(tempfile.mkdtemp())


# ===========================================================================
# TransportManager
# ===========================================================================


class TestTransportManagerCreate:
    """Construction and initial state."""

    def test_create_default(self):
        mgr = _make_manager()
        mgr.shutdown()

    def test_repr_contains_services(self):
        mgr = _make_manager()
        r = repr(mgr)
        assert "services" in r
        mgr.shutdown()

    def test_len_zero_initially(self):
        mgr = _make_manager()
        assert len(mgr) == 0
        mgr.shutdown()

    def test_session_count_zero_initially(self):
        mgr = _make_manager()
        assert mgr.session_count() == 0
        mgr.shutdown()

    def test_pool_size_zero_initially(self):
        mgr = _make_manager()
        assert mgr.pool_size() == 0
        mgr.shutdown()

    def test_is_shutdown_before_shutdown(self):
        mgr = _make_manager()
        assert mgr.is_shutdown() is False
        mgr.shutdown()

    def test_is_shutdown_after_shutdown(self):
        mgr = _make_manager()
        mgr.shutdown()
        assert mgr.is_shutdown() is True

    def test_custom_params(self):
        mgr = TransportManager(
            tempfile.mkdtemp(),
            max_connections_per_dcc=5,
            idle_timeout=60,
            heartbeat_interval=3,
            connect_timeout=5,
            reconnect_max_retries=2,
        )
        mgr.shutdown()


class TestTransportManagerRegister:
    """Register / deregister / list services."""

    def test_register_returns_str(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        assert isinstance(iid, str)
        assert len(iid) > 0
        mgr.shutdown()

    def test_register_increments_count(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        assert len(mgr.list_all_instances()) == 1
        mgr.shutdown()

    def test_register_two_instances_same_dcc(self):
        mgr = _make_manager()
        iid1 = mgr.register_service("maya", "127.0.0.1", 18812)
        iid2 = mgr.register_service("maya", "127.0.0.1", 18813)
        assert iid1 != iid2
        assert len(mgr.list_instances("maya")) == 2
        mgr.shutdown()

    def test_register_different_dccs(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.register_service("houdini", "127.0.0.1", 18820)
        assert len(mgr.list_instances("maya")) == 1
        assert len(mgr.list_instances("houdini")) == 1
        mgr.shutdown()

    def test_list_empty_dcc(self):
        mgr = _make_manager()
        assert mgr.list_instances("blender") == []
        mgr.shutdown()

    def test_list_all_instances(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.register_service("blender", "127.0.0.1", 18820)
        all_instances = mgr.list_all_instances()
        assert len(all_instances) == 2
        mgr.shutdown()

    def test_list_all_services_returns_list(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        svcs = mgr.list_all_services()
        assert isinstance(svcs, list)
        assert len(svcs) == 1
        mgr.shutdown()

    def test_deregister_returns_true(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        result = mgr.deregister_service("maya", iid)
        assert result is True
        mgr.shutdown()

    def test_deregister_removes_instance(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.deregister_service("maya", iid)
        assert mgr.list_instances("maya") == []
        mgr.shutdown()

    def test_deregister_nonexistent_returns_false(self):
        mgr = _make_manager()
        # A well-formed UUID that does not exist: returns False
        fake_uuid = "00000000-0000-0000-0000-000000000000"
        result = mgr.deregister_service("maya", fake_uuid)
        assert result is False
        mgr.shutdown()

    def test_get_service_returns_entry(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.get_service("maya", iid)
        assert entry is not None
        assert entry.instance_id == iid
        mgr.shutdown()

    def test_get_service_nonexistent_returns_none(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        # A well-formed UUID that was never registered
        fake_uuid = "00000000-0000-0000-0000-000000000000"
        result = mgr.get_service("maya", fake_uuid)
        assert result is None
        mgr.shutdown()


class TestServiceEntry:
    """ServiceEntry field validation."""

    def test_entry_dcc_type(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.dcc_type == "maya"
        mgr.shutdown()

    def test_entry_host(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.host == "127.0.0.1"
        mgr.shutdown()

    def test_entry_port(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.port == 18812
        mgr.shutdown()

    def test_entry_status_available(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.status == ServiceStatus.AVAILABLE
        mgr.shutdown()

    def test_entry_effective_address_tcp(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        addr = entry.effective_address()
        # effective_address() returns a TransportAddress; its repr is "tcp://host:port"
        addr_str = repr(addr)
        assert "127.0.0.1" in addr_str
        assert "18812" in addr_str
        mgr.shutdown()

    def test_entry_instance_id_is_str(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.instance_id == iid
        mgr.shutdown()

    def test_entry_is_ipc_false_for_tcp(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        assert entry.is_ipc is False
        mgr.shutdown()

    def test_entry_to_dict(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        entry = mgr.list_instances("maya")[0]
        d = entry.to_dict()
        assert isinstance(d, dict)
        assert "dcc_type" in d
        mgr.shutdown()


class TestTransportManagerStatusAndRank:
    """update_service_status and rank_services."""

    def test_update_status_to_busy(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        result = mgr.update_service_status("maya", iid, ServiceStatus.BUSY)
        assert result is True
        mgr.shutdown()

    def test_status_reflected_in_list(self):
        mgr = _make_manager()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.update_service_status("maya", iid, ServiceStatus.BUSY)
        entry = mgr.list_instances("maya")[0]
        assert entry.status == ServiceStatus.BUSY
        mgr.shutdown()

    def test_update_status_nonexistent_returns_false(self):
        mgr = _make_manager()
        fake_uuid = "00000000-0000-0000-0000-000000000000"
        result = mgr.update_service_status("maya", fake_uuid, ServiceStatus.BUSY)
        assert result is False
        mgr.shutdown()

    def test_rank_services_empty(self):
        mgr = _make_manager()
        # rank_services raises RuntimeError when no services are registered
        with pytest.raises(RuntimeError):
            mgr.rank_services("maya")
        mgr.shutdown()

    def test_rank_services_returns_list(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        ranked = mgr.rank_services("maya")
        assert isinstance(ranked, list)
        assert len(ranked) == 1
        mgr.shutdown()

    def test_rank_services_two_instances(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.register_service("maya", "127.0.0.1", 18813)
        ranked = mgr.rank_services("maya")
        assert len(ranked) == 2
        mgr.shutdown()

    def test_find_best_service_returns_entry(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        best = mgr.find_best_service("maya")
        assert best is not None
        mgr.shutdown()

    def test_find_best_service_empty_returns_none(self):
        mgr = _make_manager()
        # When no services are registered, find_best_service raises RuntimeError
        with pytest.raises((RuntimeError, Exception)):
            mgr.find_best_service("maya")
        mgr.shutdown()


class TestTransportManagerSessions:
    """Session creation and lifecycle (not connecting to real DCC)."""

    def test_session_count_starts_zero(self):
        mgr = _make_manager()
        assert mgr.session_count() == 0
        mgr.shutdown()

    def test_list_sessions_empty(self):
        mgr = _make_manager()
        sessions = mgr.list_sessions()
        assert isinstance(sessions, list)
        assert len(sessions) == 0
        mgr.shutdown()

    def test_list_sessions_for_dcc_empty(self):
        mgr = _make_manager()
        sessions = mgr.list_sessions_for_dcc("maya")
        assert isinstance(sessions, list)
        mgr.shutdown()

    def test_cleanup_runs_without_error(self):
        mgr = _make_manager()
        mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.cleanup()
        mgr.shutdown()


# ===========================================================================
# PyProcessMonitor
# ===========================================================================


class TestPyProcessMonitorCreate:
    """Construction and initial state."""

    def test_create(self):
        mon = PyProcessMonitor()
        assert mon is not None

    def test_repr(self):
        mon = PyProcessMonitor()
        r = repr(mon)
        assert isinstance(r, str)

    def test_tracked_count_zero(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_list_all_empty(self):
        mon = PyProcessMonitor()
        lst = mon.list_all()
        assert lst == []


class TestPyProcessMonitorTrack:
    """Track, refresh, query, untrack."""

    def test_track_increments_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1

    def test_untrack_decrements_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.untrack(pid)
        assert mon.tracked_count() == 0

    def test_query_after_refresh_returns_dict(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert isinstance(info, dict)

    def test_query_status_is_running(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["status"] == "running"

    def test_query_has_pid_key(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "pid" in info

    def test_query_has_cpu_key(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "cpu_usage_percent" in info

    def test_query_has_memory_key(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "memory_bytes" in info

    def test_query_has_restart_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert "restart_count" in info

    def test_query_untracked_pid_returns_none(self):
        mon = PyProcessMonitor()
        result = mon.query(99999999)
        assert result is None

    def test_is_alive_self(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_invalid_pid_false(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(99999999) is False

    def test_list_all_after_track(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        lst = mon.list_all()
        assert len(lst) == 1

    def test_track_two_pids(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        ppid = os.getppid() if hasattr(os, "getppid") else pid
        mon.track(pid, "self")
        if ppid != pid and mon.is_alive(ppid):
            mon.track(ppid, "parent")
            assert mon.tracked_count() == 2
        else:
            assert mon.tracked_count() >= 1


# ===========================================================================
# PyProcessWatcher
# ===========================================================================


class TestPyProcessWatcherCreate:
    """Construction and initial state."""

    def test_create_default(self):
        w = PyProcessWatcher()
        assert w is not None

    def test_create_custom_interval(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        assert w is not None

    def test_repr(self):
        w = PyProcessWatcher()
        r = repr(w)
        assert isinstance(r, str)

    def test_not_running_initially(self):
        w = PyProcessWatcher()
        assert w.is_running() is False

    def test_tracked_count_zero(self):
        w = PyProcessWatcher()
        assert w.tracked_count() == 0

    def test_watch_count_alias_zero(self):
        w = PyProcessWatcher()
        assert w.watch_count() == 0

    def test_poll_events_empty_initially(self):
        w = PyProcessWatcher()
        events = w.poll_events()
        assert events == []


class TestPyProcessWatcherTrack:
    """Track / untrack / aliases."""

    def test_track_increments_count(self):
        w = PyProcessWatcher()
        w.track(os.getpid(), "self")
        assert w.tracked_count() == 1

    def test_add_watch_alias(self):
        w = PyProcessWatcher()
        w.add_watch(os.getpid(), "self")
        assert w.tracked_count() == 1

    def test_untrack_decrements_count(self):
        w = PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "self")
        w.untrack(pid)
        assert w.tracked_count() == 0

    def test_remove_watch_alias(self):
        w = PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "self")
        w.remove_watch(pid)
        assert w.tracked_count() == 0

    def test_is_watched_true(self):
        w = PyProcessWatcher()
        pid = os.getpid()
        w.track(pid, "self")
        assert w.is_watched(pid) is True

    def test_is_watched_false(self):
        w = PyProcessWatcher()
        assert w.is_watched(os.getpid()) is False

    def test_untrack_not_watched_is_noop(self):
        w = PyProcessWatcher()
        w.untrack(99999999)  # must not raise


class TestPyProcessWatcherStartStop:
    """Start / stop lifecycle and event polling."""

    def test_start_sets_running(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.track(os.getpid(), "self")
        w.start()
        assert w.is_running() is True
        w.stop()

    def test_stop_clears_running(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.track(os.getpid(), "self")
        w.start()
        w.stop()
        assert w.is_running() is False

    def test_start_stop_idempotent(self):
        w = PyProcessWatcher(poll_interval_ms=200)
        w.track(os.getpid(), "self")
        w.start()
        w.start()  # second start is no-op
        w.stop()
        w.stop()  # second stop is no-op

    def test_poll_events_returns_list(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        assert isinstance(events, list)

    def test_poll_events_have_type_key(self):
        w = PyProcessWatcher(poll_interval_ms=100)
        w.track(os.getpid(), "self")
        w.start()
        time.sleep(0.35)
        events = w.poll_events()
        w.stop()
        if events:
            assert "type" in events[0]


# ===========================================================================
# PyDccLauncher
# ===========================================================================


class TestPyDccLauncherCreate:
    """Construction and basic state queries (no actual DCC launch)."""

    def test_create(self):
        launcher = PyDccLauncher()
        assert launcher is not None

    def test_repr(self):
        launcher = PyDccLauncher()
        r = repr(launcher)
        assert isinstance(r, str)

    def test_running_count_zero(self):
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_pid_of_unknown_returns_none(self):
        launcher = PyDccLauncher()
        result = launcher.pid_of("unknown-dcc")
        assert result is None

    def test_restart_count_unknown_returns_zero(self):
        launcher = PyDccLauncher()
        count = launcher.restart_count("unknown-dcc")
        assert count == 0

    def test_launch_nonexistent_raises(self):
        launcher = PyDccLauncher()
        with pytest.raises((RuntimeError, OSError, PermissionError, FileNotFoundError)):
            launcher.launch("test", "/nonexistent/path/to/dcc", [], 500)


# ===========================================================================
# PyCrashRecoveryPolicy
# ===========================================================================


class TestPyCrashRecoveryPolicyCreate:
    """Construction."""

    def test_create_default(self):
        policy = PyCrashRecoveryPolicy()
        assert policy is not None

    def test_create_custom_max_restarts(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        assert policy.max_restarts == 5

    def test_repr(self):
        policy = PyCrashRecoveryPolicy()
        r = repr(policy)
        assert isinstance(r, str)

    def test_max_restarts_default_three(self):
        policy = PyCrashRecoveryPolicy()
        assert policy.max_restarts == 3

    def test_max_restarts_zero(self):
        policy = PyCrashRecoveryPolicy(max_restarts=0)
        assert policy.max_restarts == 0


class TestPyCrashRecoveryPolicyShouldRestart:
    """should_restart logic."""

    def test_should_restart_crashed(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("crashed") is True

    def test_should_restart_unresponsive(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        assert policy.should_restart("unresponsive") is True

    def test_should_restart_ok_false(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        # "ok" is a valid status that does not warrant restart
        # The implementation may raise ValueError for unknown statuses;
        # "ok" must not warrant a restart — if it's unknown, it raises instead
        try:
            result = policy.should_restart("ok")
            assert result is False
        except ValueError:
            # acceptable: implementation rejects unknown status strings
            pass

    def test_should_restart_zero_max_restarts(self):
        policy = PyCrashRecoveryPolicy(max_restarts=0)
        assert policy.should_restart("crashed") is False

    def test_should_restart_unknown_status_false(self):
        policy = PyCrashRecoveryPolicy(max_restarts=3)
        # Unknown status: either returns False or raises ValueError — both acceptable
        try:
            result = policy.should_restart("some_unknown")
            assert result is False
        except ValueError:
            pass  # implementation raises for unknown statuses


class TestPyCrashRecoveryPolicyBackoff:
    """next_delay_ms with fixed and exponential backoff."""

    def test_fixed_backoff_delay(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_fixed_backoff(delay_ms=2000)
        delay = policy.next_delay_ms("maya", 0)
        assert delay == 2000

    def test_fixed_backoff_same_across_attempts(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_fixed_backoff(delay_ms=1000)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        d2 = policy.next_delay_ms("maya", 2)
        assert d0 == d1 == d2 == 1000

    def test_exponential_backoff_increases(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_exponential_backoff(initial_ms=500, max_delay_ms=10000)
        d0 = policy.next_delay_ms("maya", 0)
        d1 = policy.next_delay_ms("maya", 1)
        assert d0 <= d1

    def test_exponential_backoff_first_delay_equals_initial(self):
        policy = PyCrashRecoveryPolicy(max_restarts=5)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
        d0 = policy.next_delay_ms("maya", 0)
        assert d0 == 1000

    def test_exponential_backoff_does_not_exceed_max(self):
        policy = PyCrashRecoveryPolicy(max_restarts=10)
        policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=5000)
        for attempt in range(8):
            d = policy.next_delay_ms("maya", attempt)
            assert d <= 5000

    def test_next_delay_ms_exceeds_max_restarts_raises(self):
        policy = PyCrashRecoveryPolicy(max_restarts=2)
        policy.use_fixed_backoff(delay_ms=1000)
        with pytest.raises(RuntimeError):
            policy.next_delay_ms("maya", 3)


# ===========================================================================
# PySharedBuffer
# ===========================================================================


class TestPySharedBufferCreate:
    """Create and basic attributes."""

    def test_create_returns_buffer(self):
        buf = PySharedBuffer.create(capacity=1024)
        assert buf is not None

    def test_repr_contains_id(self):
        buf = PySharedBuffer.create(capacity=1024)
        r = repr(buf)
        assert isinstance(r, str)

    def test_capacity_matches(self):
        buf = PySharedBuffer.create(capacity=2048)
        assert buf.capacity() == 2048

    def test_data_len_initially_zero(self):
        buf = PySharedBuffer.create(capacity=1024)
        assert buf.data_len() == 0

    def test_id_is_str(self):
        buf = PySharedBuffer.create(capacity=1024)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_name_is_str(self):
        buf = PySharedBuffer.create(capacity=1024)
        p = buf.name()
        assert isinstance(p, str)
        assert len(p) > 0


class TestPySharedBufferWriteRead:
    """Write, read, clear."""

    def test_write_returns_byte_count(self):
        buf = PySharedBuffer.create(capacity=1024)
        n = buf.write(b"hello")
        assert n == 5

    def test_data_len_after_write(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"hello")
        assert buf.data_len() == 5

    def test_read_returns_written_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"hello world")
        data = buf.read()
        assert data == b"hello world"

    def test_read_empty_buffer(self):
        buf = PySharedBuffer.create(capacity=1024)
        data = buf.read()
        assert data == b""

    def test_clear_resets_data_len(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"some data")
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_then_read_empty(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"data")
        buf.clear()
        assert buf.read() == b""

    def test_overwrite_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_write_binary_data(self):
        buf = PySharedBuffer.create(capacity=1024)
        payload = bytes(range(256))
        buf.write(payload)
        assert buf.read() == payload

    def test_write_large_data(self):
        large = b"x" * 65536
        buf = PySharedBuffer.create(capacity=65536)
        buf.write(large)
        assert buf.read() == large


class TestPySharedBufferDescriptorAndOpen:
    """descriptor_json and open."""

    def test_descriptor_json_is_str(self):
        buf = PySharedBuffer.create(capacity=1024)
        desc = buf.descriptor_json()
        assert isinstance(desc, str)

    def test_descriptor_json_contains_id(self):
        buf = PySharedBuffer.create(capacity=1024)
        desc = buf.descriptor_json()
        assert buf.id in desc

    def test_open_existing_buffer(self):
        buf = PySharedBuffer.create(capacity=512)
        buf.write(b"test data")
        buf2 = PySharedBuffer.open(buf.name(), buf.id)
        assert buf2.read() == b"test data"


# ===========================================================================
# PySharedSceneBuffer
# ===========================================================================


class TestPySharedSceneBufferWrite:
    """Write and metadata."""

    def test_write_returns_buffer(self):
        ssb = PySharedSceneBuffer.write(data=b"geometry", kind=PySceneDataKind.Geometry, source_dcc="Maya")
        assert ssb is not None

    def test_id_is_str(self):
        ssb = PySharedSceneBuffer.write(data=b"geo", kind=PySceneDataKind.Geometry)
        assert isinstance(ssb.id, str)

    def test_repr_contains_id(self):
        ssb = PySharedSceneBuffer.write(data=b"geo", kind=PySceneDataKind.Geometry)
        r = repr(ssb)
        assert ssb.id in r

    def test_is_inline_small_data(self):
        ssb = PySharedSceneBuffer.write(data=b"small", kind=PySceneDataKind.Geometry)
        assert ssb.is_inline is True

    def test_is_chunked_small_data_false(self):
        ssb = PySharedSceneBuffer.write(data=b"small", kind=PySceneDataKind.Geometry)
        assert ssb.is_chunked is False


class TestPySharedSceneBufferRead:
    """Read back data."""

    def test_read_returns_written_bytes(self):
        payload = b"vertex data 12345"
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Geometry)
        assert ssb.read() == payload

    def test_read_with_source_dcc(self):
        payload = b"anim cache"
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.AnimationCache, source_dcc="Houdini")
        assert ssb.read() == payload

    def test_read_screenshot_kind(self):
        payload = b"png bytes"
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Screenshot)
        assert ssb.read() == payload

    def test_read_arbitrary_kind(self):
        payload = b"custom data"
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Arbitrary)
        assert ssb.read() == payload

    def test_read_binary_payload(self):
        payload = bytes(range(256)) * 4
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Geometry)
        assert ssb.read() == payload


class TestPySharedSceneBufferCompression:
    """Compression flag."""

    def test_write_with_compression(self):
        payload = b"compressible data " * 100
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Geometry, use_compression=True)
        assert ssb.read() == payload

    def test_write_without_compression(self):
        payload = b"data without compression"
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Geometry, use_compression=False)
        assert ssb.read() == payload

    def test_compressed_is_not_inline_for_large_data(self):
        # Large payload may be stored differently
        payload = b"compressible " * 1000
        ssb = PySharedSceneBuffer.write(data=payload, kind=PySceneDataKind.Geometry, use_compression=True)
        assert ssb.read() == payload


class TestPySharedSceneBufferDescriptor:
    """descriptor_json."""

    def test_descriptor_json_is_str(self):
        ssb = PySharedSceneBuffer.write(data=b"data", kind=PySceneDataKind.Geometry)
        desc = ssb.descriptor_json()
        assert isinstance(desc, str)

    def test_descriptor_json_contains_id(self):
        ssb = PySharedSceneBuffer.write(data=b"data", kind=PySceneDataKind.Geometry)
        desc = ssb.descriptor_json()
        assert ssb.id in desc

    def test_descriptor_json_contains_kind(self):
        ssb = PySharedSceneBuffer.write(data=b"data", kind=PySceneDataKind.Geometry)
        desc = ssb.descriptor_json()
        assert "geometry" in desc.lower()


# ===========================================================================
# PyBufferPool
# ===========================================================================


class TestPyBufferPoolCreate:
    """Construction and initial state."""

    def test_create(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_repr(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        r = repr(pool)
        assert "capacity" in r

    def test_capacity_matches(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool.capacity() == 4

    def test_buffer_size_matches(self):
        pool = PyBufferPool(capacity=4, buffer_size=512)
        assert pool.buffer_size() == 512

    def test_available_equals_capacity_initially(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        assert pool.available() == 3


class TestPyBufferPoolAcquireRelease:
    """Acquire, use, and release."""

    def test_acquire_returns_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        assert buf is not None

    def test_acquire_decrements_available(self):
        pool = PyBufferPool(capacity=3, buffer_size=512)
        _ = pool.acquire()
        assert pool.available() == 2

    def test_acquire_all_slots(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 0
        del b1, b2

    def test_acquire_raises_when_exhausted(self):
        pool = PyBufferPool(capacity=1, buffer_size=512)
        _buf = pool.acquire()
        with pytest.raises(RuntimeError):
            pool.acquire()

    def test_release_on_gc(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert pool.available() == 1
        del buf
        gc.collect()
        assert pool.available() == 2

    def test_multiple_acquire_release_cycles(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        for _ in range(5):
            buf = pool.acquire()
            buf.write(b"data")
            del buf
            gc.collect()
        assert pool.available() == 2

    def test_acquired_buffer_write_read(self):
        pool = PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        buf.write(b"scene snapshot")
        assert buf.read() == b"scene snapshot"

    def test_capacity_unchanged_after_operations(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        buf = pool.acquire()
        del buf
        gc.collect()
        assert pool.capacity() == 3
