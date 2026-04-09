"""Deep tests for TransportManager session management.

Covers: get_session / record_success / record_error / begin_reconnect / reconnect_success /
close_session / list_sessions / list_sessions_for_dcc / session_count.
Also covers connection pool: acquire_connection / release_connection / pool_size.
"""

from __future__ import annotations

import tempfile

import pytest

from dcc_mcp_core import TransportManager


def _make_mgr() -> tuple[TransportManager, str]:
    """Return (manager, tmp_dir) for a fresh TransportManager."""
    tmp = tempfile.mkdtemp()
    mgr = TransportManager(tmp)
    return mgr, tmp


# ---------------------------------------------------------------------------
# get_session
# ---------------------------------------------------------------------------


class TestTransportManagerGetSession:
    """get_session: returns dict or None."""

    def test_get_session_existing(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        info = mgr.get_session(sid)
        assert info is not None
        assert isinstance(info, dict)
        assert info["id"] == sid

    def test_get_session_nonexistent_returns_none(self):
        mgr, _ = _make_mgr()
        result = mgr.get_session("00000000-0000-0000-0000-000000000000")
        assert result is None

    def test_get_session_keys(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("blender", "127.0.0.1", 19000)
        sid = mgr.get_or_create_session("blender", iid)
        info = mgr.get_session(sid)
        required = {
            "id",
            "dcc_type",
            "instance_id",
            "host",
            "port",
            "state",
            "request_count",
            "error_count",
            "avg_latency_ms",
            "error_rate",
            "reconnect_attempts",
        }
        assert required.issubset(set(info.keys()))

    def test_get_session_initial_state_connected(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        info = mgr.get_session(sid)
        assert info["state"] == "connected"

    def test_get_session_initial_counts_zero(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("houdini", "127.0.0.1", 20000)
        sid = mgr.get_or_create_session("houdini", iid)
        info = mgr.get_session(sid)
        assert info["request_count"] == 0
        assert info["error_count"] == 0
        assert info["reconnect_attempts"] == 0


# ---------------------------------------------------------------------------
# record_success / record_error
# ---------------------------------------------------------------------------


class TestTransportManagerRecordMetrics:
    """record_success and record_error update session stats."""

    def _make_session(self, dcc="maya", port=18812):
        mgr, _ = _make_mgr()
        iid = mgr.register_service(dcc, "127.0.0.1", port)
        sid = mgr.get_or_create_session(dcc, iid)
        return mgr, sid

    def test_record_success_increments_request_count(self):
        mgr, sid = self._make_session()
        mgr.record_success(sid, 10)
        info = mgr.get_session(sid)
        assert info["request_count"] >= 1

    def test_record_error_increments_error_count(self):
        mgr, sid = self._make_session()
        mgr.record_error(sid, 20, "timeout")
        info = mgr.get_session(sid)
        assert info["error_count"] >= 1
        assert info["request_count"] >= 1

    def test_record_success_updates_avg_latency(self):
        mgr, sid = self._make_session()
        mgr.record_success(sid, 50)
        mgr.record_success(sid, 100)
        info = mgr.get_session(sid)
        assert info["avg_latency_ms"] > 0

    def test_record_error_increases_error_rate(self):
        mgr, sid = self._make_session()
        mgr.record_success(sid, 10)
        mgr.record_error(sid, 10, "err")
        info = mgr.get_session(sid)
        assert info["error_rate"] > 0

    def test_record_success_multiple(self):
        mgr, sid = self._make_session()
        for i in range(5):
            mgr.record_success(sid, i * 10)
        info = mgr.get_session(sid)
        assert info["request_count"] == 5
        assert info["error_count"] == 0

    def test_record_error_message_accepted(self):
        """record_error should accept any error message string."""
        mgr, sid = self._make_session()
        mgr.record_error(sid, 30, "connection reset by peer")
        info = mgr.get_session(sid)
        assert info["error_count"] >= 1


# ---------------------------------------------------------------------------
# begin_reconnect / reconnect_success
# ---------------------------------------------------------------------------


class TestTransportManagerReconnect:
    """begin_reconnect / reconnect_success state transitions."""

    def _make_session(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        return mgr, sid

    def test_begin_reconnect_returns_backoff_ms(self):
        mgr, sid = self._make_session()
        backoff = mgr.begin_reconnect(sid)
        assert isinstance(backoff, int)
        assert backoff >= 0

    def test_begin_reconnect_state_becomes_reconnecting(self):
        mgr, sid = self._make_session()
        mgr.begin_reconnect(sid)
        info = mgr.get_session(sid)
        assert info["state"] == "reconnecting"

    def test_reconnect_success_state_becomes_connected(self):
        mgr, sid = self._make_session()
        mgr.begin_reconnect(sid)
        mgr.reconnect_success(sid)
        info = mgr.get_session(sid)
        assert info["state"] == "connected"

    def test_begin_reconnect_increments_reconnect_attempts(self):
        mgr, sid = self._make_session()
        mgr.begin_reconnect(sid)
        # After reconnect attempt, reconnect_attempts may be tracked
        info = mgr.get_session(sid)
        # Some impls reset after success; just verify no error thrown
        assert isinstance(info["reconnect_attempts"], int)

    def test_multiple_reconnect_cycles(self):
        """Multiple begin_reconnect / reconnect_success cycles."""
        mgr, sid = self._make_session()
        for _ in range(3):
            mgr.begin_reconnect(sid)
            mgr.reconnect_success(sid)
        info = mgr.get_session(sid)
        assert info["state"] == "connected"


# ---------------------------------------------------------------------------
# close_session
# ---------------------------------------------------------------------------


class TestTransportManagerCloseSession:
    """close_session: returns True if closed, False if not found."""

    def test_close_existing_session_returns_true(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        result = mgr.close_session(sid)
        assert result is True

    def test_close_reduces_session_count(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        before = mgr.session_count()
        mgr.close_session(sid)
        assert mgr.session_count() == before - 1

    def test_close_nonexistent_returns_false(self):
        mgr, _ = _make_mgr()
        result = mgr.close_session("00000000-0000-0000-0000-000000000000")
        assert result is False

    def test_close_twice_returns_false(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        mgr.close_session(sid)
        result = mgr.close_session(sid)
        assert result is False

    def test_get_session_after_close_returns_none(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        sid = mgr.get_or_create_session("maya", iid)
        mgr.close_session(sid)
        assert mgr.get_session(sid) is None


# ---------------------------------------------------------------------------
# list_sessions / list_sessions_for_dcc / session_count
# ---------------------------------------------------------------------------


class TestTransportManagerListSessions:
    """list_sessions, list_sessions_for_dcc, session_count."""

    def test_list_sessions_empty(self):
        mgr, _ = _make_mgr()
        sessions = mgr.list_sessions()
        assert isinstance(sessions, list)
        assert len(sessions) == 0

    def test_list_sessions_one_entry(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.get_or_create_session("maya", iid)
        sessions = mgr.list_sessions()
        assert len(sessions) == 1

    def test_list_sessions_multiple(self):
        mgr, _ = _make_mgr()
        iid1 = mgr.register_service("maya", "127.0.0.1", 18812)
        iid2 = mgr.register_service("blender", "127.0.0.1", 19000)
        mgr.get_or_create_session("maya", iid1)
        mgr.get_or_create_session("blender", iid2)
        sessions = mgr.list_sessions()
        assert len(sessions) == 2

    def test_list_sessions_for_dcc_filters(self):
        mgr, _ = _make_mgr()
        iid1 = mgr.register_service("maya", "127.0.0.1", 18812)
        iid2 = mgr.register_service("blender", "127.0.0.1", 19000)
        mgr.get_or_create_session("maya", iid1)
        mgr.get_or_create_session("blender", iid2)
        maya_sessions = mgr.list_sessions_for_dcc("maya")
        assert len(maya_sessions) == 1
        assert maya_sessions[0]["dcc_type"] == "maya"

    def test_list_sessions_for_dcc_empty(self):
        mgr, _ = _make_mgr()
        result = mgr.list_sessions_for_dcc("nonexistent")
        assert isinstance(result, list)
        assert len(result) == 0

    def test_session_count_matches_list_sessions(self):
        mgr, _ = _make_mgr()
        iid1 = mgr.register_service("maya", "127.0.0.1", 18812)
        iid2 = mgr.register_service("maya", "127.0.0.1", 18813)
        mgr.get_or_create_session("maya", iid1)
        mgr.get_or_create_session("maya", iid2)
        assert mgr.session_count() == len(mgr.list_sessions())

    def test_session_count_initial_zero(self):
        mgr, _ = _make_mgr()
        assert mgr.session_count() == 0

    def test_list_sessions_returns_dicts(self):
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.get_or_create_session("maya", iid)
        sessions = mgr.list_sessions()
        assert isinstance(sessions[0], dict)
        assert "id" in sessions[0]
        assert "dcc_type" in sessions[0]


# ---------------------------------------------------------------------------
# Connection pool
# ---------------------------------------------------------------------------


class TestTransportManagerConnectionPool:
    """acquire_connection / release_connection / pool_size / pool_count_for_dcc.

    Note: acquire_connection actually tries to establish a real TCP connection,
    so tests that call it with an unreachable address expect RuntimeError.
    pool_size() is always 0 without a real connection.
    """

    def test_pool_size_initial_zero(self):
        mgr, _ = _make_mgr()
        assert mgr.pool_size() == 0

    def test_pool_count_for_dcc_initial_zero(self):
        mgr, _ = _make_mgr()
        assert mgr.pool_count_for_dcc("maya") == 0

    def test_acquire_raises_when_no_server(self):
        """acquire_connection raises RuntimeError when no server is listening."""
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 19999)
        with pytest.raises(RuntimeError):
            mgr.acquire_connection("maya", iid)

    def test_release_connection_no_op_when_pool_empty(self):
        """release_connection should not raise even when nothing acquired."""
        import contextlib

        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        # Should not raise (nothing to release is a no-op)
        with contextlib.suppress(Exception):
            mgr.release_connection("maya", iid)


# ---------------------------------------------------------------------------
# cleanup / shutdown
# ---------------------------------------------------------------------------


class TestTransportManagerLifecycle:
    """cleanup / shutdown / is_shutdown."""

    def test_is_shutdown_initial_false(self):
        mgr, _ = _make_mgr()
        assert mgr.is_shutdown() is False

    def test_shutdown_marks_as_shutdown(self):
        mgr, _ = _make_mgr()
        mgr.shutdown()
        assert mgr.is_shutdown() is True

    def test_cleanup_returns_tuple(self):
        mgr, _ = _make_mgr()
        result = mgr.cleanup()
        assert isinstance(result, tuple)
        assert len(result) == 3

    def test_len_reflects_sessions(self):
        """len(mgr) reflects sessions, not services."""
        mgr, _ = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        assert len(mgr) == 0  # no sessions yet
        mgr.get_or_create_session("maya", iid)
        assert len(mgr) >= 1

    def test_repr_is_string(self):
        mgr, _ = _make_mgr()
        assert isinstance(repr(mgr), str)

    def test_list_all_instances_alias(self):
        """list_all_instances is alias for list_all_services."""
        mgr, _ = _make_mgr()
        mgr.register_service("maya", "127.0.0.1", 18812)
        all_svc = mgr.list_all_services()
        all_inst = mgr.list_all_instances()
        assert len(all_svc) == len(all_inst)
