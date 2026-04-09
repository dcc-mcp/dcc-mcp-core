"""Tests for TransportManager, ServiceEntry/ServiceStatus, wrap_value/unwrap_value, PyProcessMonitor.

Coverage targets:
- TransportManager lifecycle (register/deregister/find/rank/update/shutdown)
- ServiceEntry attributes and to_dict()
- ServiceStatus enum values
- wrap_value / unwrap_value round-trip for all supported types
- PyProcessMonitor track/untrack/is_alive/list_all/tracked_count/refresh
"""

from __future__ import annotations

# Import built-in modules
import os
import tempfile

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import TransportManager
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def make_manager() -> tuple[TransportManager, str]:
    """Return a fresh TransportManager backed by a temp directory."""
    tmp = tempfile.mkdtemp()
    return TransportManager(tmp), tmp


# ---------------------------------------------------------------------------
# TestTransportManagerConstruction
# ---------------------------------------------------------------------------


class TestTransportManagerConstruction:
    """Basic construction and initial state."""

    def test_requires_registry_dir(self):
        with pytest.raises(TypeError):
            TransportManager()  # type: ignore[call-arg]

    def test_initial_is_not_shutdown(self):
        tm, _ = make_manager()
        assert tm.is_shutdown() is False

    def test_initial_pool_size_zero(self):
        tm, _ = make_manager()
        assert tm.pool_size() == 0

    def test_initial_session_count_zero(self):
        tm, _ = make_manager()
        assert tm.session_count() == 0

    def test_initial_list_all_services_empty(self):
        tm, _ = make_manager()
        assert tm.list_all_services() == []

    def test_initial_list_all_instances_empty(self):
        tm, _ = make_manager()
        assert tm.list_all_instances() == []

    def test_initial_list_sessions_empty(self):
        tm, _ = make_manager()
        assert tm.list_sessions() == []

    def test_initial_list_sessions_for_dcc_empty(self):
        tm, _ = make_manager()
        assert tm.list_sessions_for_dcc("maya") == []

    def test_initial_pool_count_for_dcc_zero(self):
        tm, _ = make_manager()
        assert tm.pool_count_for_dcc("maya") == 0

    def test_multiple_instances_independent(self):
        tm1, tmp1 = make_manager()
        tm2, tmp2 = make_manager()
        assert tmp1 != tmp2
        tm1.register_service("maya", "127.0.0.1", 9001, "tcp")
        assert len(tm2.list_all_instances()) == 0


# ---------------------------------------------------------------------------
# TestTransportManagerRegisterService
# ---------------------------------------------------------------------------


class TestTransportManagerRegisterService:
    """register_service returns instance_id and updates lists."""

    def test_register_returns_string_instance_id(self):
        tm, _ = make_manager()
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        assert isinstance(iid, str)
        assert len(iid) > 0

    def test_register_unique_instance_ids(self):
        tm, _ = make_manager()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        iid2 = tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        assert iid1 != iid2

    def test_register_increases_instance_count(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        assert len(tm.list_all_instances()) == 1
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        assert len(tm.list_all_instances()) == 2

    def test_register_different_dcc_types(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("blender", "127.0.0.1", 9002, "tcp")
        tm.register_service("houdini", "127.0.0.1", 9003, "tcp")
        assert len(tm.list_all_instances()) == 3

    def test_list_instances_filters_by_dcc_type(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        tm.register_service("blender", "127.0.0.1", 9003, "tcp")
        maya_list = tm.list_instances("maya")
        blender_list = tm.list_instances("blender")
        assert len(maya_list) == 2
        assert len(blender_list) == 1

    def test_list_instances_unknown_dcc_empty(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        assert tm.list_instances("houdini") == []


# ---------------------------------------------------------------------------
# TestServiceEntryAttributes
# ---------------------------------------------------------------------------


class TestServiceEntryAttributes:
    """ServiceEntry object attributes after registration."""

    def _get_entry(self, tm: TransportManager, dcc: str = "maya") -> object:
        instances = tm.list_all_instances()
        for inst in instances:
            if inst.dcc_type == dcc:
                return inst
        pytest.fail(f"No instance found for dcc_type={dcc}")

    def test_entry_dcc_type(self):
        tm, _ = make_manager()
        tm.register_service("blender", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm, "blender")
        assert inst.dcc_type == "blender"

    def test_entry_host(self):
        tm, _ = make_manager()
        tm.register_service("maya", "192.168.1.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.host == "192.168.1.1"

    def test_entry_port(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 7777, "tcp")
        inst = self._get_entry(tm)
        assert inst.port == 7777

    def test_entry_instance_id_is_string(self):
        tm, _ = make_manager()
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.instance_id == iid

    def test_entry_status_initially_available(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.status == ServiceStatus.AVAILABLE

    def test_entry_metadata_initially_empty_dict(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.metadata == {}

    def test_entry_scene_initially_none(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.scene is None

    def test_entry_is_ipc_false_for_tcp(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.is_ipc is False

    def test_entry_last_heartbeat_ms_is_int(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert isinstance(inst.last_heartbeat_ms, int)
        assert inst.last_heartbeat_ms > 0

    def test_entry_to_dict_keys(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        d = inst.to_dict()
        expected_keys = {
            "dcc_type",
            "instance_id",
            "host",
            "port",
            "version",
            "scene",
            "metadata",
            "status",
            "last_heartbeat_ms",
        }
        assert expected_keys.issubset(d.keys())

    def test_entry_to_dict_dcc_type_value(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        d = inst.to_dict()
        assert d["dcc_type"] == "maya"

    def test_entry_to_dict_status_is_string(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        d = inst.to_dict()
        assert isinstance(d["status"], str)
        assert d["status"] == "AVAILABLE"

    def test_entry_transport_address_none_by_default(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        inst = self._get_entry(tm)
        assert inst.transport_address is None


# ---------------------------------------------------------------------------
# TestServiceStatus
# ---------------------------------------------------------------------------


class TestServiceStatus:
    """ServiceStatus enum values."""

    def test_available_exists(self):
        assert hasattr(ServiceStatus, "AVAILABLE")

    def test_busy_exists(self):
        assert hasattr(ServiceStatus, "BUSY")

    def test_unreachable_exists(self):
        assert hasattr(ServiceStatus, "UNREACHABLE")

    def test_shutting_down_exists(self):
        assert hasattr(ServiceStatus, "SHUTTING_DOWN")

    def test_available_not_equal_busy(self):
        assert ServiceStatus.AVAILABLE != ServiceStatus.BUSY

    def test_available_not_equal_unreachable(self):
        assert ServiceStatus.AVAILABLE != ServiceStatus.UNREACHABLE

    def test_status_repr_contains_name(self):
        r = repr(ServiceStatus.AVAILABLE)
        assert "AVAILABLE" in r


# ---------------------------------------------------------------------------
# TestTransportManagerGetService
# ---------------------------------------------------------------------------


class TestTransportManagerGetService:
    """get_service by dcc_type + instance_id."""

    def test_get_service_returns_entry(self):
        tm, _ = make_manager()
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        svc = tm.get_service("maya", iid)
        assert svc is not None
        assert svc.instance_id == iid

    def test_get_service_wrong_dcc_type_returns_none_or_raises(self):
        # get_service filters by dcc_type; wrong type returns None or raises
        tm, _ = make_manager()
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        try:
            result = tm.get_service("blender", iid)
            # If it doesn't raise, result should be None (not found)
            assert result is None
        except (RuntimeError, KeyError, ValueError):
            pass  # Also acceptable behavior

    def test_get_service_unknown_instance_id_raises_value_error(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        # Non-UUID string causes ValueError at UUID parsing
        with pytest.raises((ValueError, RuntimeError, KeyError)):
            tm.get_service("maya", "nonexistent-id")


# ---------------------------------------------------------------------------
# TestTransportManagerFindAndRank
# ---------------------------------------------------------------------------


class TestTransportManagerFindAndRank:
    """find_best_service and rank_services."""

    def test_find_best_single_service(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        best = tm.find_best_service("maya")
        assert best is not None
        assert best.dcc_type == "maya"

    def test_find_best_multiple_returns_one(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        best = tm.find_best_service("maya")
        assert best is not None

    def test_find_best_no_service_raises(self):
        tm, _ = make_manager()
        with pytest.raises((RuntimeError, KeyError)):
            tm.find_best_service("houdini")

    def test_rank_services_returns_list(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        ranked = tm.rank_services("maya")
        assert isinstance(ranked, list)
        assert len(ranked) == 2

    def test_rank_services_all_same_dcc_type(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        tm.register_service("blender", "127.0.0.1", 9003, "tcp")
        ranked = tm.rank_services("maya")
        assert all(s.dcc_type == "maya" for s in ranked)

    def test_rank_services_unknown_raises(self):
        tm, _ = make_manager()
        tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        # rank_services raises RuntimeError when no service of that type is registered
        with pytest.raises(RuntimeError):
            tm.rank_services("houdini")


# ---------------------------------------------------------------------------
# TestTransportManagerUpdateStatus
# ---------------------------------------------------------------------------


class TestTransportManagerUpdateStatus:
    """update_service_status changes entry status."""

    def _register_and_get_iid(self, tm: TransportManager) -> str:
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        return iid

    def test_update_to_busy(self):
        tm, _ = make_manager()
        iid = self._register_and_get_iid(tm)
        tm.update_service_status("maya", iid, ServiceStatus.BUSY)
        svc = tm.get_service("maya", iid)
        assert svc.status == ServiceStatus.BUSY

    def test_update_to_unreachable(self):
        tm, _ = make_manager()
        iid = self._register_and_get_iid(tm)
        tm.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        svc = tm.get_service("maya", iid)
        assert svc.status == ServiceStatus.UNREACHABLE

    def test_update_to_shutting_down(self):
        tm, _ = make_manager()
        iid = self._register_and_get_iid(tm)
        tm.update_service_status("maya", iid, ServiceStatus.SHUTTING_DOWN)
        svc = tm.get_service("maya", iid)
        assert svc.status == ServiceStatus.SHUTTING_DOWN

    def test_update_back_to_available(self):
        tm, _ = make_manager()
        iid = self._register_and_get_iid(tm)
        tm.update_service_status("maya", iid, ServiceStatus.BUSY)
        tm.update_service_status("maya", iid, ServiceStatus.AVAILABLE)
        svc = tm.get_service("maya", iid)
        assert svc.status == ServiceStatus.AVAILABLE

    def test_update_requires_service_status_type(self):
        tm, _ = make_manager()
        iid = self._register_and_get_iid(tm)
        with pytest.raises(TypeError):
            tm.update_service_status("maya", iid, "busy")  # type: ignore[arg-type]


# ---------------------------------------------------------------------------
# TestTransportManagerDeregister
# ---------------------------------------------------------------------------


class TestTransportManagerDeregister:
    """deregister_service removes the entry."""

    def test_deregister_reduces_count(self):
        tm, _ = make_manager()
        iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        assert len(tm.list_all_instances()) == 2
        tm.deregister_service("maya", iid)
        assert len(tm.list_all_instances()) == 1

    def test_deregister_all_empties_list(self):
        tm, _ = make_manager()
        iid1 = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        iid2 = tm.register_service("maya", "127.0.0.1", 9002, "tcp")
        tm.deregister_service("maya", iid1)
        tm.deregister_service("maya", iid2)
        assert tm.list_all_instances() == []

    def test_deregister_only_removes_target_dcc(self):
        tm, _ = make_manager()
        maya_iid = tm.register_service("maya", "127.0.0.1", 9001, "tcp")
        tm.register_service("blender", "127.0.0.1", 9002, "tcp")
        tm.deregister_service("maya", maya_iid)
        remaining = tm.list_all_instances()
        assert len(remaining) == 1
        assert remaining[0].dcc_type == "blender"


# ---------------------------------------------------------------------------
# TestTransportManagerShutdown
# ---------------------------------------------------------------------------


class TestTransportManagerShutdown:
    """shutdown() marks is_shutdown True."""

    def test_shutdown_sets_is_shutdown(self):
        tm, _ = make_manager()
        assert tm.is_shutdown() is False
        tm.shutdown()
        assert tm.is_shutdown() is True

    def test_shutdown_idempotent(self):
        tm, _ = make_manager()
        tm.shutdown()
        tm.shutdown()
        assert tm.is_shutdown() is True


# ---------------------------------------------------------------------------
# TestWrapUnwrapValue
# ---------------------------------------------------------------------------


class TestWrapUnwrapValue:
    """wrap_value and unwrap_value round-trip tests."""

    def test_wrap_int(self):
        w = wrap_value(42)
        assert w is not None

    def test_unwrap_int(self):
        w = wrap_value(42)
        assert unwrap_value(w) == 42

    def test_wrap_negative_int(self):
        w = wrap_value(-100)
        assert unwrap_value(w) == -100

    def test_wrap_zero(self):
        w = wrap_value(0)
        assert unwrap_value(w) == 0

    def test_wrap_float(self):
        w = wrap_value(3.14)
        result = unwrap_value(w)
        assert abs(result - 3.14) < 1e-9

    def test_wrap_float_zero(self):
        w = wrap_value(0.0)
        assert unwrap_value(w) == 0.0

    def test_wrap_float_negative(self):
        w = wrap_value(-2.718)
        result = unwrap_value(w)
        assert abs(result - (-2.718)) < 1e-9

    def test_wrap_string(self):
        w = wrap_value("hello")
        assert unwrap_value(w) == "hello"

    def test_wrap_empty_string(self):
        w = wrap_value("")
        assert unwrap_value(w) == ""

    def test_wrap_unicode_string(self):
        w = wrap_value("你好世界")
        assert unwrap_value(w) == "你好世界"

    def test_wrap_true(self):
        w = wrap_value(True)
        assert unwrap_value(w) is True

    def test_wrap_false(self):
        w = wrap_value(False)
        assert unwrap_value(w) is False

    def test_wrap_none(self):
        w = wrap_value(None)
        assert unwrap_value(w) is None

    def test_wrap_list(self):
        lst = [1, 2, 3]
        w = wrap_value(lst)
        assert unwrap_value(w) == lst

    def test_wrap_empty_list(self):
        w = wrap_value([])
        assert unwrap_value(w) == []

    def test_wrap_nested_list(self):
        lst = [[1, 2], [3, 4]]
        w = wrap_value(lst)
        assert unwrap_value(w) == lst

    def test_wrap_dict(self):
        d = {"a": 1, "b": 2}
        w = wrap_value(d)
        assert unwrap_value(w) == d

    def test_wrap_empty_dict(self):
        w = wrap_value({})
        assert unwrap_value(w) == {}

    def test_wrap_bytes(self):
        b = b"binary data"
        w = wrap_value(b)
        assert unwrap_value(w) == b

    def test_wrap_int_value_attribute(self):
        w = wrap_value(99)
        assert hasattr(w, "value")
        assert w.value == 99

    def test_wrap_string_value_attribute(self):
        w = wrap_value("test")
        assert hasattr(w, "value")
        assert w.value == "test"

    def test_wrap_float_value_attribute(self):
        w = wrap_value(1.5)
        assert hasattr(w, "value")
        assert abs(w.value - 1.5) < 1e-9

    def test_wrap_bool_value_attribute(self):
        w = wrap_value(True)
        assert hasattr(w, "value")
        assert w.value is True

    def test_none_wrap_returns_none(self):
        result = wrap_value(None)
        assert result is None

    def test_double_wrap_unwrap(self):
        original = 42
        w = wrap_value(original)
        first_unwrap = unwrap_value(w)
        assert first_unwrap == original

    def test_large_int(self):
        big = 2**50
        w = wrap_value(big)
        assert unwrap_value(w) == big

    def test_list_with_mixed_types(self):
        lst = [1, "two", 3.0, None]
        w = wrap_value(lst)
        assert unwrap_value(w) == lst


# ---------------------------------------------------------------------------
# TestPyProcessMonitorLifecycle
# ---------------------------------------------------------------------------


class TestPyProcessMonitorLifecycle:
    """PyProcessMonitor construction and initial state."""

    def test_construction(self):
        pm = PyProcessMonitor()
        assert pm is not None

    def test_initial_tracked_count_zero(self):
        pm = PyProcessMonitor()
        assert pm.tracked_count() == 0

    def test_initial_list_all_empty(self):
        pm = PyProcessMonitor()
        assert pm.list_all() == []

    def test_refresh_no_error(self):
        pm = PyProcessMonitor()
        pm.refresh()

    def test_track_increases_count(self):
        pm = PyProcessMonitor()
        # Use a fake PID unlikely to exist (but track() accepts any int)
        pm.track(99999, "test-dcc")
        assert pm.tracked_count() == 1
        pm.untrack(99999)

    def test_untrack_decreases_count(self):
        pm = PyProcessMonitor()
        pm.track(99998, "test-dcc")
        assert pm.tracked_count() == 1
        pm.untrack(99998)
        assert pm.tracked_count() == 0

    def test_track_multiple_processes(self):
        pm = PyProcessMonitor()
        pm.track(99991, "dcc-1")
        pm.track(99992, "dcc-2")
        pm.track(99993, "dcc-3")
        assert pm.tracked_count() == 3
        pm.untrack(99991)
        pm.untrack(99992)
        pm.untrack(99993)

    def test_untrack_unknown_pid_no_error(self):
        pm = PyProcessMonitor()
        pm.untrack(99990)


# ---------------------------------------------------------------------------
# TestPyProcessMonitorAliveness
# ---------------------------------------------------------------------------


class TestPyProcessMonitorAliveness:
    """is_alive checks for tracked processes."""

    def test_is_alive_fake_pid_false(self):
        pm = PyProcessMonitor()
        # PID that almost certainly does not exist
        assert pm.is_alive(99995) is False

    def test_is_alive_current_process(self):
        pm = PyProcessMonitor()
        pid = os.getpid()
        # is_alive doesn't require tracking - it's just a liveness check
        # It may return True or False depending on sysinfo; just ensure it returns bool
        result = pm.is_alive(pid)
        assert isinstance(result, bool)

    def test_track_fake_pid_then_check(self):
        pm = PyProcessMonitor()
        pm.track(99994, "fake-dcc")
        result = pm.is_alive(99994)
        assert isinstance(result, bool)
        pm.untrack(99994)


# ---------------------------------------------------------------------------
# TestPyProcessMonitorListAll
# ---------------------------------------------------------------------------


class TestPyProcessMonitorListAll:
    """list_all returns list of dicts with known keys."""

    def test_list_all_returns_list(self):
        pm = PyProcessMonitor()
        result = pm.list_all()
        assert isinstance(result, list)

    def test_list_all_after_track_has_entry(self):
        pm = PyProcessMonitor()
        pm.track(99987, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        assert len(result) == 1
        pm.untrack(99987)

    def test_list_all_entry_has_pid(self):
        pm = PyProcessMonitor()
        pm.track(99986, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        assert len(result) == 1
        entry = result[0]
        assert "pid" in entry
        assert entry["pid"] == 99986
        pm.untrack(99986)

    def test_list_all_entry_has_name(self):
        pm = PyProcessMonitor()
        pm.track(99985, "my-dcc-name")
        pm.refresh()
        result = pm.list_all()
        entry = result[0]
        assert "name" in entry
        assert entry["name"] == "my-dcc-name"
        pm.untrack(99985)

    def test_list_all_entry_has_status(self):
        pm = PyProcessMonitor()
        pm.track(99984, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        entry = result[0]
        assert "status" in entry
        assert isinstance(entry["status"], str)
        pm.untrack(99984)

    def test_list_all_entry_has_cpu_usage(self):
        pm = PyProcessMonitor()
        pm.track(99983, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        entry = result[0]
        assert "cpu_usage_percent" in entry
        pm.untrack(99983)

    def test_list_all_entry_has_memory_bytes(self):
        pm = PyProcessMonitor()
        pm.track(99982, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        entry = result[0]
        assert "memory_bytes" in entry
        pm.untrack(99982)

    def test_list_all_entry_has_restart_count(self):
        pm = PyProcessMonitor()
        pm.track(99981, "fake-dcc")
        pm.refresh()
        result = pm.list_all()
        entry = result[0]
        assert "restart_count" in entry
        assert entry["restart_count"] == 0
        pm.untrack(99981)

    def test_list_all_after_untrack_empty(self):
        pm = PyProcessMonitor()
        pm.track(99980, "fake-dcc")
        pm.untrack(99980)
        result = pm.list_all()
        assert len(result) == 0

    def test_list_all_multiple_processes(self):
        pm = PyProcessMonitor()
        pm.track(99979, "dcc-a")
        pm.track(99978, "dcc-b")
        pm.refresh()
        result = pm.list_all()
        assert len(result) == 2
        pm.untrack(99979)
        pm.untrack(99978)


# ---------------------------------------------------------------------------
# TestPyProcessMonitorQuery
# ---------------------------------------------------------------------------


class TestPyProcessMonitorQuery:
    """query() returns None for fake/non-tracked PIDs (needs sysinfo)."""

    def test_query_untracked_pid_returns_none(self):
        pm = PyProcessMonitor()
        result = pm.query(99970)
        assert result is None

    def test_query_tracked_fake_pid_returns_none_after_refresh(self):
        pm = PyProcessMonitor()
        pm.track(99969, "fake-dcc")
        pm.refresh()
        # Fake PID not in sysinfo → query returns None
        result = pm.query(99969)
        assert result is None
        pm.untrack(99969)
