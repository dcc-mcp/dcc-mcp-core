"""Deep tests for TransportManager session/pool operations, ListenerHandle, PyProcessMonitor.

Also covers PyCrashRecoveryPolicy, SceneInfo/SceneStatistics,
ScriptResult, DccError/DccErrorCode, and RoutingStrategy.

Tests are grouped by class; each class covers happy-path and error-path
scenarios for one API surface.
"""

from __future__ import annotations

import os
import tempfile

import pytest

import dcc_mcp_core as m

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_manager() -> m.TransportManager:
    """Return a TransportManager backed by a temporary directory."""
    tmpdir = tempfile.mkdtemp()
    return m.TransportManager(tmpdir)


# ===========================================================================
# TransportManager - Session Operations
# ===========================================================================


class TestTransportManagerSessionOps:
    """Session operations: get_or_create_session, get_session, record_success/error.

    Also covers close_session, list_sessions, session_count.
    """

    def test_get_or_create_session_returns_string(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        assert isinstance(sid, str)

    def test_get_or_create_session_idempotent(self):
        """Same (dcc, instance) returns the same session id."""
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid1 = mgr.get_or_create_session("maya", iid)
        sid2 = mgr.get_or_create_session("maya", iid)
        assert sid1 == sid2

    def test_get_or_create_session_no_instance(self):
        """get_or_create_session with instance_id=None creates a session."""
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya")
        assert isinstance(sid, str) and len(sid) > 0

    def test_get_session_returns_dict(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("blender")
        sid = mgr.get_or_create_session("blender", iid)
        info = mgr.get_session(sid)
        assert isinstance(info, dict)

    def test_get_session_has_session_id_key(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("blender")
        sid = mgr.get_or_create_session("blender", iid)
        info = mgr.get_session(sid)
        assert "session_id" in info or "id" in info or sid in str(info)

    def test_get_session_unknown_returns_none_or_raises(self):
        mgr = _make_manager()
        # Implementation may return None or raise RuntimeError/ValueError for unknown/invalid session id
        try:
            result = mgr.get_session("does-not-exist-uuid")
            assert result is None
        except (RuntimeError, ValueError):
            pass  # Also acceptable behaviour

    def test_record_success_does_not_raise(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        mgr.record_success(sid, latency_ms=10)  # should not raise

    def test_record_success_multiple_times(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        for ms in [5, 10, 15]:
            mgr.record_success(sid, latency_ms=ms)

    def test_record_error_does_not_raise(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        mgr.record_error(sid, latency_ms=50, error="timeout")

    def test_record_error_with_empty_message(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        mgr.record_error(sid, latency_ms=0, error="")

    def test_session_count_increases(self):
        mgr = _make_manager()
        assert mgr.session_count() == 0
        iid, _ = mgr.bind_and_register("maya")
        mgr.get_or_create_session("maya", iid)
        assert mgr.session_count() >= 1

    def test_list_sessions_returns_list(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        mgr.get_or_create_session("maya", iid)
        sessions = mgr.list_sessions()
        assert isinstance(sessions, list)

    def test_list_sessions_non_empty_after_creation(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        mgr.get_or_create_session("maya", iid)
        assert len(mgr.list_sessions()) >= 1

    def test_list_sessions_for_dcc_returns_list(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        mgr.get_or_create_session("maya", iid)
        result = mgr.list_sessions_for_dcc("maya")
        assert isinstance(result, list)

    def test_list_sessions_for_dcc_empty_for_unknown_dcc(self):
        mgr = _make_manager()
        result = mgr.list_sessions_for_dcc("nonexistent_dcc")
        assert result == [] or isinstance(result, list)

    def test_close_session_returns_bool(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        result = mgr.close_session(sid)
        assert isinstance(result, bool)

    def test_close_session_unknown_returns_false_or_raises(self):
        mgr = _make_manager()
        try:
            result = mgr.close_session("no-such-session")
            assert result is False
        except (RuntimeError, ValueError):
            pass  # Also acceptable behaviour

    def test_begin_reconnect_returns_int(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        attempt = mgr.begin_reconnect(sid)
        assert isinstance(attempt, int)

    def test_reconnect_success_does_not_raise(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session("maya", iid)
        mgr.begin_reconnect(sid)
        mgr.reconnect_success(sid)  # should not raise

    def test_multiple_sessions_different_dccs(self):
        mgr = _make_manager()
        iid1, _ = mgr.bind_and_register("maya")
        iid2, _ = mgr.bind_and_register("blender")
        sid1 = mgr.get_or_create_session("maya", iid1)
        sid2 = mgr.get_or_create_session("blender", iid2)
        assert sid1 != sid2
        assert mgr.session_count() >= 2


# ===========================================================================
# TransportManager - Connection Pool
# ===========================================================================


class TestTransportManagerConnectionPool:
    """acquire_connection, release_connection, pool_size, pool_count_for_dcc."""

    def test_pool_size_initial_zero(self):
        mgr = _make_manager()
        assert mgr.pool_size() == 0

    def test_pool_count_for_dcc_initial_zero(self):
        mgr = _make_manager()
        assert mgr.pool_count_for_dcc("maya") == 0

    def test_acquire_connection_returns_string_or_raises(self):
        """acquire_connection tries real IPC; may raise RuntimeError in CI/no-DCC environments."""
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        try:
            conn_id = mgr.acquire_connection("maya", iid)
            assert isinstance(conn_id, str)
        except RuntimeError:
            pass  # Expected when no DCC process is listening

    def test_acquire_connection_no_instance_or_raises(self):
        """acquire_connection with no instance may raise RuntimeError in CI."""
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        try:
            conn_id = mgr.acquire_connection("maya")
            assert isinstance(conn_id, str)
        except RuntimeError:
            pass  # Expected when no DCC process is listening

    def test_pool_size_increases_after_acquire(self):
        """Pool size increases when connection acquired; skips if IPC unavailable."""
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        try:
            mgr.acquire_connection("maya", iid)
            assert mgr.pool_size() >= 1
        except RuntimeError:
            pytest.skip("IPC connection not available in this environment")

    def test_pool_count_for_dcc_increases_after_acquire(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        try:
            mgr.acquire_connection("maya", iid)
            assert mgr.pool_count_for_dcc("maya") >= 1
        except RuntimeError:
            pytest.skip("IPC connection not available in this environment")

    def test_release_connection_does_not_raise(self):
        mgr = _make_manager()
        iid, _ = mgr.bind_and_register("maya")
        try:
            mgr.acquire_connection("maya", iid)
            mgr.release_connection("maya", iid)  # should not raise
        except RuntimeError:
            pytest.skip("IPC connection not available in this environment")

    def test_multiple_acquire_different_dccs(self):
        mgr = _make_manager()
        iid1, _ = mgr.bind_and_register("maya")
        iid2, _ = mgr.bind_and_register("blender")
        try:
            mgr.acquire_connection("maya", iid1)
            mgr.acquire_connection("blender", iid2)
            assert mgr.pool_size() >= 2
        except RuntimeError:
            pytest.skip("IPC connection not available in this environment")


# ===========================================================================
# TransportManager - Cleanup
# ===========================================================================


class TestTransportManagerCleanup:
    """cleanup method returns (sessions_removed, conns_removed, services_removed)."""

    def test_cleanup_returns_tuple(self):
        mgr = _make_manager()
        result = mgr.cleanup()
        assert isinstance(result, tuple)

    def test_cleanup_returns_three_ints(self):
        mgr = _make_manager()
        result = mgr.cleanup()
        assert len(result) == 3
        assert all(isinstance(v, int) for v in result)

    def test_cleanup_on_empty_manager(self):
        mgr = _make_manager()
        sessions_removed, conns_removed, services_removed = mgr.cleanup()
        assert sessions_removed == 0
        assert conns_removed == 0
        assert services_removed == 0

    def test_cleanup_idempotent(self):
        mgr = _make_manager()
        mgr.cleanup()
        result2 = mgr.cleanup()
        assert isinstance(result2, tuple)

    def test_len_returns_int(self):
        mgr = _make_manager()
        assert isinstance(len(mgr), int)


# ===========================================================================
# TransportManager - Routing
# ===========================================================================


class TestTransportManagerRouting:
    """get_or_create_session_routed, find_best_service with RoutingStrategy."""

    def test_get_or_create_session_routed_no_strategy(self):
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session_routed("maya")
        assert isinstance(sid, str)

    def test_get_or_create_session_routed_with_first_available(self):
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session_routed("maya", strategy=m.RoutingStrategy.FIRST_AVAILABLE)
        assert isinstance(sid, str)

    def test_get_or_create_session_routed_with_round_robin(self):
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        sid = mgr.get_or_create_session_routed("maya", strategy=m.RoutingStrategy.ROUND_ROBIN)
        assert isinstance(sid, str)

    def test_find_best_service_returns_service_entry(self):
        mgr = _make_manager()
        mgr.bind_and_register("maya")
        entry = mgr.find_best_service("maya")
        assert isinstance(entry, m.ServiceEntry)

    def test_find_best_service_dcc_type_matches(self):
        mgr = _make_manager()
        mgr.bind_and_register("blender")
        entry = mgr.find_best_service("blender")
        assert entry.dcc_type == "blender"

    def test_find_best_service_no_instances_raises(self):
        mgr = _make_manager()
        with pytest.raises(RuntimeError):
            mgr.find_best_service("nonexistent_dcc")

    def test_routing_strategy_all_variants_exist(self):
        for variant in ["FIRST_AVAILABLE", "ROUND_ROBIN", "LEAST_BUSY", "SPECIFIC", "SCENE_MATCH", "RANDOM"]:
            assert hasattr(m.RoutingStrategy, variant)

    def test_routing_strategy_repr_non_empty(self):
        r = repr(m.RoutingStrategy.FIRST_AVAILABLE)
        assert len(r) > 0

    def test_routing_strategy_eq(self):
        assert m.RoutingStrategy.FIRST_AVAILABLE == m.RoutingStrategy.FIRST_AVAILABLE
        assert m.RoutingStrategy.FIRST_AVAILABLE != m.RoutingStrategy.ROUND_ROBIN


# ===========================================================================
# ListenerHandle
# ===========================================================================


class TestListenerHandle:
    """ListenerHandle.accept_count, is_shutdown, transport_name, local_address, shutdown."""

    def _make_handle(self) -> m.ListenerHandle:
        addr = m.TransportAddress.tcp("127.0.0.1", 0)
        listener = m.IpcListener.bind(addr)
        return listener.into_handle()

    def test_accept_count_initial_zero(self):
        handle = self._make_handle()
        assert handle.accept_count == 0

    def test_is_shutdown_initial_false(self):
        handle = self._make_handle()
        assert handle.is_shutdown is False

    def test_transport_name_non_empty(self):
        handle = self._make_handle()
        assert isinstance(handle.transport_name, str)
        assert len(handle.transport_name) > 0

    def test_transport_name_is_tcp(self):
        handle = self._make_handle()
        assert handle.transport_name == "tcp"

    def test_local_address_returns_transport_address(self):
        handle = self._make_handle()
        addr = handle.local_address()
        assert isinstance(addr, m.TransportAddress)

    def test_local_address_is_tcp(self):
        handle = self._make_handle()
        addr = handle.local_address()
        assert addr.is_tcp

    def test_shutdown_sets_is_shutdown_true(self):
        handle = self._make_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_idempotent(self):
        handle = self._make_handle()
        handle.shutdown()
        handle.shutdown()  # should not raise
        assert handle.is_shutdown is True

    def test_repr_non_empty(self):
        handle = self._make_handle()
        r = repr(handle)
        assert len(r) > 0

    def test_repr_contains_handle(self):
        handle = self._make_handle()
        r = repr(handle).lower()
        assert "handle" in r or "listener" in r or "tcp" in r


# ===========================================================================
# PyProcessMonitor
# ===========================================================================


class TestPyProcessMonitorConstruction:
    def test_creates(self):
        mon = m.PyProcessMonitor()
        assert mon is not None

    def test_tracked_count_initial_zero(self):
        mon = m.PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_repr_non_empty(self):
        mon = m.PyProcessMonitor()
        assert len(repr(mon)) > 0

    def test_list_all_initial_empty(self):
        mon = m.PyProcessMonitor()
        assert mon.list_all() == []


class TestPyProcessMonitorTrack:
    def test_track_own_pid(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        assert mon.tracked_count() == 1

    def test_untrack_own_pid(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.untrack(os.getpid())
        assert mon.tracked_count() == 0

    def test_track_multiple_pids(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.track(1, "init")
        assert mon.tracked_count() == 2

    def test_untrack_nonexistent_does_not_raise(self):
        mon = m.PyProcessMonitor()
        mon.untrack(99999999)  # should not raise

    def test_track_then_refresh_then_query(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info is not None

    def test_query_returns_dict_with_pid(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert isinstance(info, dict)
        assert "pid" in info

    def test_query_returns_dict_with_name(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert "name" in info

    def test_query_pid_matches(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert info["pid"] == os.getpid()

    def test_query_unknown_pid_returns_none(self):
        mon = m.PyProcessMonitor()
        result = mon.query(99999999)
        assert result is None

    def test_is_alive_own_pid(self):
        mon = m.PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_nonexistent_pid(self):
        mon = m.PyProcessMonitor()
        assert mon.is_alive(99999999) is False

    def test_list_all_after_track_and_refresh(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        lst = mon.list_all()
        assert isinstance(lst, list)
        assert len(lst) >= 1

    def test_list_all_items_are_dicts(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        for item in mon.list_all():
            assert isinstance(item, dict)

    def test_query_status_field_present(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert "status" in info

    def test_query_memory_bytes_field(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert "memory_bytes" in info

    def test_query_restart_count_field(self):
        mon = m.PyProcessMonitor()
        mon.track(os.getpid(), "self")
        mon.refresh()
        info = mon.query(os.getpid())
        assert "restart_count" in info
        assert info["restart_count"] == 0


# ===========================================================================
# PyCrashRecoveryPolicy
# ===========================================================================


class TestPyCrashRecoveryPolicyConstruction:
    def test_creates(self):
        p = m.PyCrashRecoveryPolicy()
        assert p is not None

    def test_max_restarts_default(self):
        p = m.PyCrashRecoveryPolicy()
        assert p.max_restarts == 3

    def test_max_restarts_custom(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=5)
        assert p.max_restarts == 5

    def test_repr_non_empty(self):
        p = m.PyCrashRecoveryPolicy()
        assert len(repr(p)) > 0

    def test_independence(self):
        p1 = m.PyCrashRecoveryPolicy(max_restarts=2)
        p2 = m.PyCrashRecoveryPolicy(max_restarts=7)
        assert p1.max_restarts == 2
        assert p2.max_restarts == 7


class TestPyCrashRecoveryPolicyShouldRestart:
    def test_should_restart_crashed(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("crashed") is True

    def test_should_restart_unresponsive(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("unresponsive") is True

    def test_should_restart_stopped_returns_false(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=3)
        # "stopped" is a valid status but does not trigger restart
        result = p.should_restart("stopped")
        assert result is False

    def test_should_restart_unknown_raises(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=3)
        with pytest.raises((ValueError, RuntimeError)):
            p.should_restart("exited")  # unknown status raises


class TestPyCrashRecoveryPolicyBackoff:
    def test_use_fixed_backoff(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=1000)
        delay = p.next_delay_ms("maya", attempt=0)
        assert delay == 1000

    def test_use_fixed_backoff_consistent(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=500)
        assert p.next_delay_ms("maya", 0) == p.next_delay_ms("maya", 2)

    def test_use_exponential_backoff(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        delay_0 = p.next_delay_ms("maya", attempt=0)
        delay_1 = p.next_delay_ms("maya", attempt=1)
        assert delay_0 <= delay_1  # exponential: delay grows

    def test_exponential_capped_at_max(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=20)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        for attempt in range(10):
            delay = p.next_delay_ms("maya", attempt=attempt)
            assert delay <= 5000

    def test_next_delay_ms_exceeds_max_restarts_raises(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=2)
        p.use_fixed_backoff(delay_ms=100)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", attempt=5)  # beyond max_restarts

    def test_zero_max_restarts(self):
        p = m.PyCrashRecoveryPolicy(max_restarts=0)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", attempt=0)


# ===========================================================================
# SceneStatistics
# ===========================================================================


class TestSceneStatisticsConstruction:
    def test_default_all_zero(self):
        s = m.SceneStatistics()
        assert s.object_count == 0
        assert s.vertex_count == 0
        assert s.polygon_count == 0
        assert s.material_count == 0
        assert s.texture_count == 0
        assert s.light_count == 0
        assert s.camera_count == 0

    def test_custom_values(self):
        s = m.SceneStatistics(
            object_count=10,
            vertex_count=200,
            polygon_count=100,
            material_count=5,
            texture_count=3,
            light_count=2,
            camera_count=1,
        )
        assert s.object_count == 10
        assert s.vertex_count == 200
        assert s.polygon_count == 100
        assert s.material_count == 5
        assert s.texture_count == 3
        assert s.light_count == 2
        assert s.camera_count == 1

    def test_repr_non_empty(self):
        s = m.SceneStatistics(object_count=5)
        assert len(repr(s)) > 0

    def test_partial_init(self):
        s = m.SceneStatistics(object_count=3)
        assert s.object_count == 3
        assert s.vertex_count == 0

    def test_independence(self):
        s1 = m.SceneStatistics(object_count=1)
        s2 = m.SceneStatistics(object_count=99)
        assert s1.object_count == 1
        assert s2.object_count == 99


# ===========================================================================
# SceneInfo
# ===========================================================================


class TestSceneInfoConstruction:
    def test_default_construction(self):
        si = m.SceneInfo()
        assert si is not None

    def test_default_file_path_empty(self):
        si = m.SceneInfo()
        assert si.file_path == ""

    def test_default_name_untitled(self):
        si = m.SceneInfo()
        assert si.name == "untitled"

    def test_default_modified_false(self):
        si = m.SceneInfo()
        assert si.modified is False

    def test_default_format_empty(self):
        si = m.SceneInfo()
        assert si.format == ""

    def test_default_frame_range_none(self):
        si = m.SceneInfo()
        assert si.frame_range is None

    def test_default_fps_none(self):
        si = m.SceneInfo()
        assert si.fps is None

    def test_default_up_axis_none(self):
        si = m.SceneInfo()
        assert si.up_axis is None

    def test_default_units_none(self):
        si = m.SceneInfo()
        assert si.units is None

    def test_custom_file_path(self):
        si = m.SceneInfo(file_path="/project/scene.ma")
        assert si.file_path == "/project/scene.ma"

    def test_custom_name(self):
        si = m.SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_custom_modified_true(self):
        si = m.SceneInfo(modified=True)
        assert si.modified is True

    def test_custom_format(self):
        si = m.SceneInfo(format="mayaAscii")
        assert si.format == "mayaAscii"

    def test_custom_frame_range(self):
        si = m.SceneInfo(frame_range=(1.0, 100.0))
        assert si.frame_range == (1.0, 100.0)

    def test_custom_fps(self):
        si = m.SceneInfo(fps=24.0)
        assert si.fps == 24.0

    def test_custom_up_axis(self):
        si = m.SceneInfo(up_axis="Y")
        assert si.up_axis == "Y"

    def test_custom_units(self):
        si = m.SceneInfo(units="cm")
        assert si.units == "cm"

    def test_statistics_default(self):
        si = m.SceneInfo()
        assert isinstance(si.statistics, m.SceneStatistics)

    def test_custom_statistics(self):
        stats = m.SceneStatistics(object_count=7)
        si = m.SceneInfo(statistics=stats)
        assert si.statistics.object_count == 7

    def test_metadata_empty_default(self):
        si = m.SceneInfo()
        assert isinstance(si.metadata, dict)

    def test_custom_metadata(self):
        si = m.SceneInfo(metadata={"key": "value"})
        assert si.metadata.get("key") == "value"

    def test_current_frame_none_by_default(self):
        si = m.SceneInfo()
        assert si.current_frame is None

    def test_custom_current_frame(self):
        si = m.SceneInfo(current_frame=10.0)
        assert si.current_frame == 10.0

    def test_repr_non_empty(self):
        si = m.SceneInfo(name="test")
        assert len(repr(si)) > 0

    def test_independence(self):
        si1 = m.SceneInfo(name="a")
        si2 = m.SceneInfo(name="b")
        assert si1.name == "a"
        assert si2.name == "b"


# ===========================================================================
# ScriptResult
# ===========================================================================


class TestScriptResultConstruction:
    def test_creates_success(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        assert r is not None

    def test_success_field(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        assert r.success is True

    def test_failure_field(self):
        r = m.ScriptResult(success=False, execution_time_ms=5)
        assert r.success is False

    def test_execution_time_ms(self):
        r = m.ScriptResult(success=True, execution_time_ms=42)
        assert r.execution_time_ms == 42

    def test_output_default_none(self):
        r = m.ScriptResult(success=True, execution_time_ms=0)
        assert r.output is None

    def test_error_default_none(self):
        r = m.ScriptResult(success=True, execution_time_ms=0)
        assert r.error is None

    def test_output_custom(self):
        r = m.ScriptResult(success=True, execution_time_ms=5, output="sphere1")
        assert r.output == "sphere1"

    def test_error_custom(self):
        r = m.ScriptResult(success=False, execution_time_ms=5, error="NameError: x")
        assert r.error == "NameError: x"

    def test_context_default_empty(self):
        r = m.ScriptResult(success=True, execution_time_ms=0)
        assert isinstance(r.context, dict)

    def test_context_custom(self):
        r = m.ScriptResult(success=True, execution_time_ms=0, context={"node": "sphereShape1"})
        assert r.context.get("node") == "sphereShape1"

    def test_to_dict_returns_dict(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        d = r.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_has_success_key(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        assert "success" in r.to_dict()

    def test_to_dict_success_value(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        assert r.to_dict()["success"] is True

    def test_repr_non_empty(self):
        r = m.ScriptResult(success=True, execution_time_ms=10)
        assert len(repr(r)) > 0

    def test_independence(self):
        r1 = m.ScriptResult(success=True, execution_time_ms=1)
        r2 = m.ScriptResult(success=False, execution_time_ms=2)
        assert r1.success is True
        assert r2.success is False


# ===========================================================================
# DccErrorCode / DccError
# ===========================================================================


class TestDccErrorCodeVariants:
    def test_connection_failed_exists(self):
        assert hasattr(m.DccErrorCode, "CONNECTION_FAILED")

    def test_timeout_exists(self):
        assert hasattr(m.DccErrorCode, "TIMEOUT")

    def test_script_error_exists(self):
        assert hasattr(m.DccErrorCode, "SCRIPT_ERROR")

    def test_not_responding_exists(self):
        assert hasattr(m.DccErrorCode, "NOT_RESPONDING")

    def test_unsupported_exists(self):
        assert hasattr(m.DccErrorCode, "UNSUPPORTED")

    def test_permission_denied_exists(self):
        assert hasattr(m.DccErrorCode, "PERMISSION_DENIED")

    def test_invalid_input_exists(self):
        assert hasattr(m.DccErrorCode, "INVALID_INPUT")

    def test_scene_error_exists(self):
        assert hasattr(m.DccErrorCode, "SCENE_ERROR")

    def test_internal_exists(self):
        assert hasattr(m.DccErrorCode, "INTERNAL")

    def test_repr_non_empty(self):
        assert len(repr(m.DccErrorCode.TIMEOUT)) > 0

    def test_equality(self):
        assert m.DccErrorCode.TIMEOUT == m.DccErrorCode.TIMEOUT
        assert m.DccErrorCode.TIMEOUT != m.DccErrorCode.SCRIPT_ERROR

    def test_all_distinct(self):
        codes = [
            m.DccErrorCode.CONNECTION_FAILED,
            m.DccErrorCode.TIMEOUT,
            m.DccErrorCode.SCRIPT_ERROR,
            m.DccErrorCode.NOT_RESPONDING,
            m.DccErrorCode.UNSUPPORTED,
            m.DccErrorCode.PERMISSION_DENIED,
            m.DccErrorCode.INVALID_INPUT,
            m.DccErrorCode.SCENE_ERROR,
            m.DccErrorCode.INTERNAL,
        ]
        seen = set()
        for c in codes:
            r = repr(c)
            assert r not in seen, f"Duplicate repr: {r}"
            seen.add(r)


class TestDccErrorConstruction:
    def test_creates(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="timed out")
        assert e is not None

    def test_code_field(self):
        e = m.DccError(code=m.DccErrorCode.SCRIPT_ERROR, message="err")
        assert e.code == m.DccErrorCode.SCRIPT_ERROR

    def test_message_field(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="operation timed out")
        assert e.message == "operation timed out"

    def test_details_default_none(self):
        e = m.DccError(code=m.DccErrorCode.INTERNAL, message="err")
        assert e.details is None

    def test_details_custom(self):
        e = m.DccError(code=m.DccErrorCode.INTERNAL, message="err", details="trace here")
        assert e.details == "trace here"

    def test_recoverable_default_false(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="t")
        assert e.recoverable is False

    def test_recoverable_custom_true(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="t", recoverable=True)
        assert e.recoverable is True

    def test_repr_non_empty(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="t")
        assert len(repr(e)) > 0

    def test_str_non_empty(self):
        e = m.DccError(code=m.DccErrorCode.TIMEOUT, message="timed out")
        assert len(str(e)) > 0

    def test_independence(self):
        e1 = m.DccError(code=m.DccErrorCode.TIMEOUT, message="a")
        e2 = m.DccError(code=m.DccErrorCode.SCRIPT_ERROR, message="b")
        assert e1.code != e2.code
        assert e1.message != e2.message


# ===========================================================================
# ResourceAnnotations / ResourceTemplateDefinition
# ===========================================================================


class TestResourceAnnotationsConstruction:
    def test_default_construction(self):
        ra = m.ResourceAnnotations()
        assert ra is not None

    def test_audience_default_empty(self):
        ra = m.ResourceAnnotations()
        assert isinstance(ra.audience, list)
        assert ra.audience == []

    def test_priority_default_none(self):
        ra = m.ResourceAnnotations()
        assert ra.priority is None

    def test_custom_audience(self):
        ra = m.ResourceAnnotations(audience=["user", "agent"])
        assert "user" in ra.audience
        assert "agent" in ra.audience

    def test_custom_priority(self):
        ra = m.ResourceAnnotations(priority=0.8)
        assert ra.priority == pytest.approx(0.8)

    def test_repr_non_empty(self):
        ra = m.ResourceAnnotations(audience=["x"])
        assert len(repr(ra)) > 0

    def test_independence(self):
        ra1 = m.ResourceAnnotations(priority=0.1)
        ra2 = m.ResourceAnnotations(priority=0.9)
        assert ra1.priority != ra2.priority


class TestResourceTemplateDefinitionConstruction:
    def test_creates(self):
        rtd = m.ResourceTemplateDefinition(
            uri_template="file://{path}",
            name="file-resource",
            description="A file resource",
        )
        assert rtd is not None

    def test_uri_template_field(self):
        rtd = m.ResourceTemplateDefinition("file://{path}", "file", "desc")
        assert rtd.uri_template == "file://{path}"

    def test_name_field(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "myname", "desc")
        assert rtd.name == "myname"

    def test_description_field(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "my description")
        assert rtd.description == "my description"

    def test_mime_type_default(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "d")
        assert rtd.mime_type == "text/plain"

    def test_mime_type_custom(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "d", mime_type="application/json")
        assert rtd.mime_type == "application/json"

    def test_annotations_default_none(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "d")
        assert rtd.annotations is None

    def test_annotations_custom(self):
        ann = m.ResourceAnnotations(priority=0.5)
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "d", annotations=ann)
        assert rtd.annotations is not None

    def test_repr_non_empty(self):
        rtd = m.ResourceTemplateDefinition("x://{y}", "n", "d")
        assert len(repr(rtd)) > 0

    def test_independence(self):
        rtd1 = m.ResourceTemplateDefinition("a://{b}", "n1", "d1")
        rtd2 = m.ResourceTemplateDefinition("c://{d}", "n2", "d2")
        assert rtd1.uri_template != rtd2.uri_template


# ===========================================================================
# PromptArgument / PromptDefinition
# ===========================================================================


class TestPromptArgumentConstruction:
    def test_creates(self):
        pa = m.PromptArgument(name="radius", description="Sphere radius")
        assert pa is not None

    def test_name_field(self):
        pa = m.PromptArgument(name="radius", description="desc")
        assert pa.name == "radius"

    def test_description_field(self):
        pa = m.PromptArgument(name="x", description="X coordinate")
        assert pa.description == "X coordinate"

    def test_required_default_false(self):
        pa = m.PromptArgument(name="x", description="d")
        assert pa.required is False

    def test_required_custom_true(self):
        pa = m.PromptArgument(name="x", description="d", required=True)
        assert pa.required is True

    def test_repr_non_empty(self):
        pa = m.PromptArgument(name="x", description="d")
        assert len(repr(pa)) > 0

    def test_equality_same_values(self):
        pa1 = m.PromptArgument(name="x", description="d", required=True)
        pa2 = m.PromptArgument(name="x", description="d", required=True)
        assert pa1 == pa2

    def test_inequality_different_name(self):
        pa1 = m.PromptArgument(name="x", description="d")
        pa2 = m.PromptArgument(name="y", description="d")
        assert pa1 != pa2

    def test_independence(self):
        pa1 = m.PromptArgument(name="a", description="aa", required=True)
        pa2 = m.PromptArgument(name="b", description="bb", required=False)
        assert pa1.name != pa2.name
        assert pa1.required != pa2.required


class TestPromptDefinitionConstruction:
    def test_creates_minimal(self):
        pd = m.PromptDefinition(name="create_sphere", description="Create a sphere")
        assert pd is not None

    def test_name_field(self):
        pd = m.PromptDefinition(name="create_sphere", description="d")
        assert pd.name == "create_sphere"

    def test_description_field(self):
        pd = m.PromptDefinition(name="n", description="Make a sphere")
        assert pd.description == "Make a sphere"

    def test_arguments_default_empty(self):
        pd = m.PromptDefinition(name="n", description="d")
        assert pd.arguments == []

    def test_arguments_with_one(self):
        pa = m.PromptArgument(name="radius", description="r")
        pd = m.PromptDefinition(name="n", description="d", arguments=[pa])
        assert len(pd.arguments) == 1

    def test_arguments_with_multiple(self):
        pa1 = m.PromptArgument(name="radius", description="r")
        pa2 = m.PromptArgument(name="segments", description="s", required=True)
        pd = m.PromptDefinition(name="n", description="d", arguments=[pa1, pa2])
        assert len(pd.arguments) == 2

    def test_argument_fields_preserved(self):
        pa = m.PromptArgument(name="r", description="radius", required=True)
        pd = m.PromptDefinition(name="n", description="d", arguments=[pa])
        assert pd.arguments[0].name == "r"
        assert pd.arguments[0].required is True

    def test_equality_same_values(self):
        pd1 = m.PromptDefinition(name="n", description="d")
        pd2 = m.PromptDefinition(name="n", description="d")
        assert pd1 == pd2

    def test_inequality_different_name(self):
        pd1 = m.PromptDefinition(name="a", description="d")
        pd2 = m.PromptDefinition(name="b", description="d")
        assert pd1 != pd2

    def test_repr_non_empty(self):
        pd = m.PromptDefinition(name="n", description="d")
        assert len(repr(pd)) > 0

    def test_independence(self):
        pd1 = m.PromptDefinition(name="p1", description="d1")
        pd2 = m.PromptDefinition(name="p2", description="d2")
        assert pd1.name != pd2.name


# ===========================================================================
# CaptureResult
# ===========================================================================


class TestCaptureResultConstruction:
    def test_creates(self):
        cr = m.CaptureResult(data=b"fake_image", width=1920, height=1080, format="png")
        assert cr is not None

    def test_data_field(self):
        cr = m.CaptureResult(data=b"abc", width=100, height=100, format="png")
        assert cr.data == b"abc"

    def test_width_field(self):
        cr = m.CaptureResult(data=b"", width=640, height=480, format="jpeg")
        assert cr.width == 640

    def test_height_field(self):
        cr = m.CaptureResult(data=b"", width=640, height=480, format="jpeg")
        assert cr.height == 480

    def test_format_field(self):
        cr = m.CaptureResult(data=b"", width=1, height=1, format="raw_bgra")
        assert cr.format == "raw_bgra"

    def test_viewport_default_none(self):
        cr = m.CaptureResult(data=b"", width=1, height=1, format="png")
        assert cr.viewport is None

    def test_viewport_custom(self):
        cr = m.CaptureResult(data=b"", width=1, height=1, format="png", viewport="persp")
        assert cr.viewport == "persp"

    def test_data_size_method(self):
        cr = m.CaptureResult(data=b"hello world", width=1, height=1, format="png")
        assert cr.data_size() == 11

    def test_data_size_empty(self):
        cr = m.CaptureResult(data=b"", width=1, height=1, format="png")
        assert cr.data_size() == 0

    def test_repr_non_empty(self):
        cr = m.CaptureResult(data=b"x", width=1, height=1, format="png")
        assert len(repr(cr)) > 0

    def test_independence(self):
        cr1 = m.CaptureResult(data=b"a", width=100, height=100, format="png")
        cr2 = m.CaptureResult(data=b"b", width=200, height=200, format="jpeg")
        assert cr1.width != cr2.width
        assert cr1.format != cr2.format
