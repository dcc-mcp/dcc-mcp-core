"""Tests for TransportManager.find_best_service, rank_services, and ServiceStatus."""

from __future__ import annotations

import tempfile

import pytest

from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import TransportManager


def make_tm() -> TransportManager:
    """Return a fresh TransportManager with a temp registry dir."""
    return TransportManager(tempfile.mkdtemp())


class TestServiceStatusEnum:
    """Tests for ServiceStatus enum values."""

    def test_available_exists(self):
        assert hasattr(ServiceStatus, "AVAILABLE")

    def test_busy_exists(self):
        assert hasattr(ServiceStatus, "BUSY")

    def test_shutting_down_exists(self):
        assert hasattr(ServiceStatus, "SHUTTING_DOWN")

    def test_unreachable_exists(self):
        assert hasattr(ServiceStatus, "UNREACHABLE")

    def test_available_is_service_status(self):
        assert isinstance(ServiceStatus.AVAILABLE, ServiceStatus)

    def test_busy_is_service_status(self):
        assert isinstance(ServiceStatus.BUSY, ServiceStatus)

    def test_shutting_down_is_service_status(self):
        assert isinstance(ServiceStatus.SHUTTING_DOWN, ServiceStatus)

    def test_unreachable_is_service_status(self):
        assert isinstance(ServiceStatus.UNREACHABLE, ServiceStatus)

    def test_statuses_are_distinct(self):
        statuses = [
            ServiceStatus.AVAILABLE,
            ServiceStatus.BUSY,
            ServiceStatus.SHUTTING_DOWN,
            ServiceStatus.UNREACHABLE,
        ]
        for i, a in enumerate(statuses):
            for j, b in enumerate(statuses):
                if i != j:
                    assert a != b


class TestFindBestServiceEmpty:
    """Tests for find_best_service when no services are registered."""

    def test_find_best_raises_when_empty(self):
        tm = make_tm()
        with pytest.raises(RuntimeError):
            tm.find_best_service("maya")

    def test_find_best_raises_for_unknown_dcc(self):
        tm = make_tm()
        with pytest.raises(RuntimeError):
            tm.find_best_service("nonexistent_dcc")

    def test_rank_services_empty_raises(self):
        tm = make_tm()
        with pytest.raises(RuntimeError):
            tm.rank_services("maya")

    def test_rank_services_unknown_dcc_raises(self):
        tm = make_tm()
        with pytest.raises(RuntimeError):
            tm.rank_services("blender")


class TestFindBestServiceSingleInstance:
    """Tests for find_best_service with a single registered service."""

    def test_find_best_returns_service_entry(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert entry is not None

    def test_find_best_returns_correct_host(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert entry.host == "localhost"

    def test_find_best_returns_correct_port(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert entry.port == 7001

    def test_find_best_returns_correct_dcc_type(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert entry.dcc_type == "maya"

    def test_find_best_returns_available_status(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert entry.status == ServiceStatus.AVAILABLE

    def test_find_best_instance_id_is_string(self):
        tm = make_tm()
        instance_id = tm.register_service("maya", "localhost", 7001)
        entry = tm.find_best_service("maya")
        assert isinstance(entry.instance_id, str)
        assert entry.instance_id == instance_id

    def test_rank_services_single_returns_one_entry(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        result = tm.rank_services("maya")
        assert len(result) == 1

    def test_rank_services_single_matches_find_best(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        best = tm.find_best_service("maya")
        ranked = tm.rank_services("maya")
        assert ranked[0].instance_id == best.instance_id


class TestFindBestServiceMultipleInstances:
    """Tests for find_best_service and rank_services with multiple instances."""

    def test_rank_services_returns_all_instances(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("maya", "localhost", 7002)
        tm.register_service("maya", "localhost", 7003)
        result = tm.rank_services("maya")
        assert len(result) == 3

    def test_rank_services_returns_list(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("maya", "localhost", 7002)
        result = tm.rank_services("maya")
        assert isinstance(result, list)

    def test_find_best_returns_one_of_registered(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        entry = tm.find_best_service("maya")
        assert entry.instance_id in {id1, id2}

    def test_rank_all_instance_ids_unique(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        id3 = tm.register_service("maya", "localhost", 7003)
        ranked = tm.rank_services("maya")
        ids = [e.instance_id for e in ranked]
        assert len(set(ids)) == 3
        assert set(ids) == {id1, id2, id3}

    def test_rank_services_first_is_find_best(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("maya", "localhost", 7002)
        best = tm.find_best_service("maya")
        ranked = tm.rank_services("maya")
        # The best service should be among the first entries
        assert ranked[0].instance_id == best.instance_id

    def test_rank_with_version_metadata(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001, version="2024", scene="shot_001")
        tm.register_service("maya", "localhost", 7002, version="2025")
        ranked = tm.rank_services("maya")
        assert len(ranked) == 2

    def test_different_dcc_types_independent(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("blender", "localhost", 8001)
        tm.register_service("blender", "localhost", 8002)

        maya_ranked = tm.rank_services("maya")
        blender_ranked = tm.rank_services("blender")
        assert len(maya_ranked) == 1
        assert len(blender_ranked) == 2

    def test_find_best_cross_dcc_isolated(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("blender", "localhost", 8001)

        maya_best = tm.find_best_service("maya")
        blender_best = tm.find_best_service("blender")
        assert maya_best.dcc_type == "maya"
        assert blender_best.dcc_type == "blender"
        assert maya_best.port == 7001
        assert blender_best.port == 8001


class TestUpdateServiceStatus:
    """Tests for update_service_status and its effect on ranking."""

    def test_update_status_to_busy(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        tm.update_service_status("maya", inst_id, ServiceStatus.BUSY)
        entry = tm.get_service("maya", inst_id)
        assert entry.status == ServiceStatus.BUSY

    def test_update_status_to_unreachable(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        tm.update_service_status("maya", inst_id, ServiceStatus.UNREACHABLE)
        entry = tm.get_service("maya", inst_id)
        assert entry.status == ServiceStatus.UNREACHABLE

    def test_update_status_to_available_again(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        tm.update_service_status("maya", inst_id, ServiceStatus.BUSY)
        tm.update_service_status("maya", inst_id, ServiceStatus.AVAILABLE)
        entry = tm.get_service("maya", inst_id)
        assert entry.status == ServiceStatus.AVAILABLE

    def test_rank_prefers_available_over_busy(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        # Mark id1 BUSY
        tm.update_service_status("maya", id1, ServiceStatus.BUSY)
        ranked = tm.rank_services("maya")
        # id2 (AVAILABLE) should rank higher than id1 (BUSY)
        assert ranked[0].instance_id == id2

    def test_find_best_prefers_available_over_busy(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        tm.update_service_status("maya", id1, ServiceStatus.BUSY)
        best = tm.find_best_service("maya")
        assert best.instance_id == id2

    def test_update_status_string_raises(self):
        """Status must be ServiceStatus enum, not str."""
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        with pytest.raises((TypeError, RuntimeError)):
            tm.update_service_status("maya", inst_id, "BUSY")


class TestDeregisterService:
    """Tests for deregister_service and its effect on ranking."""

    def test_deregister_removes_from_rank(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        tm.deregister_service("maya", id1)
        ranked = tm.rank_services("maya")
        assert len(ranked) == 1
        assert ranked[0].instance_id == id2

    def test_find_best_after_deregister(self):
        tm = make_tm()
        id1 = tm.register_service("maya", "localhost", 7001)
        id2 = tm.register_service("maya", "localhost", 7002)
        tm.deregister_service("maya", id1)
        best = tm.find_best_service("maya")
        assert best.instance_id == id2

    def test_deregister_all_raises_find_best(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        tm.deregister_service("maya", inst_id)
        with pytest.raises(RuntimeError):
            tm.find_best_service("maya")

    def test_deregister_all_rank_raises(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        tm.deregister_service("maya", inst_id)
        with pytest.raises(RuntimeError):
            tm.rank_services("maya")


class TestGetService:
    """Tests for get_service direct lookup."""

    def test_get_service_returns_entry(self):
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        entry = tm.get_service("maya", inst_id)
        assert entry.instance_id == inst_id

    def test_get_service_wrong_id_returns_none(self):
        """get_service with valid UUID but not registered returns None."""
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        entry = tm.get_service("maya", "00000000-0000-0000-0000-000000000000")
        assert entry is None

    def test_get_service_wrong_dcc_returns_none(self):
        """get_service with wrong dcc_type returns None (id not found in that dcc)."""
        tm = make_tm()
        inst_id = tm.register_service("maya", "localhost", 7001)
        entry = tm.get_service("blender", inst_id)
        assert entry is None


class TestListAllServices:
    """Tests for list_all_services."""

    def test_list_all_empty(self):
        tm = make_tm()
        assert tm.list_all_services() == []

    def test_list_all_returns_registered(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("blender", "localhost", 8001)
        all_svcs = tm.list_all_services()
        assert len(all_svcs) == 2

    def test_list_all_across_dcc_types(self):
        tm = make_tm()
        tm.register_service("maya", "localhost", 7001)
        tm.register_service("maya", "localhost", 7002)
        tm.register_service("houdini", "localhost", 9001)
        all_svcs = tm.list_all_services()
        assert len(all_svcs) == 3
        dcc_types = {e.dcc_type for e in all_svcs}
        assert "maya" in dcc_types
        assert "houdini" in dcc_types
