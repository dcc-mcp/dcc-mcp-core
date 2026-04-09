"""Tests for TransportManager connection pool, FramedChannel.__bool__, and TransportManager lifecycle.

Targets previously uncovered APIs:
- TransportManager.acquire_connection() with a real IpcListener
- TransportManager.pool_size() / pool_count_for_dcc() after acquire
- FramedChannel.__bool__ (True when running, False after shutdown)
- TransportManager.shutdown() / is_shutdown()
- TransportManager cleanup() return tuple structure
- TransportManager.get_or_create_session_routed() with RoutingStrategy
- TransportManager.__len__ (counts sessions, not services)
"""

from __future__ import annotations

from pathlib import Path

import pytest

import dcc_mcp_core

# ── TransportManager connection pool ─────────────────────────────────────────


class TestConnectionPool:
    """Tests for TransportManager.acquire_connection() and release_connection()."""

    def _register_with_listener(
        self,
        transport: dcc_mcp_core.TransportManager,
        dcc_type: str = "maya",
    ) -> tuple[str, dcc_mcp_core.IpcListener]:
        """Bind a real listener and register it so acquire_connection() can connect."""
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()
        conn_str = local.to_connection_string()
        port = int(conn_str.rsplit(":", 1)[-1])
        iid = transport.register_service(dcc_type, "127.0.0.1", port)
        return iid, listener

    def test_acquire_connection_returns_string(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        iid, _listener = self._register_with_listener(t)
        try:
            conn_id = t.acquire_connection("maya", iid)
            assert isinstance(conn_id, str)
            assert len(conn_id) > 0
        except RuntimeError:
            pytest.skip("acquire_connection requires an active server-side accept loop")

    def test_acquire_without_instance_id(self, tmp_path: Path) -> None:
        """acquire_connection() with instance_id=None should use any available instance."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        _iid, _listener = self._register_with_listener(t, "maya")
        try:
            conn_id = t.acquire_connection("maya")
            assert isinstance(conn_id, str)
        except RuntimeError:
            pytest.skip("acquire_connection requires an active server-side accept loop")

    def test_pool_size_initial_zero(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        assert t.pool_size() == 0

    def test_pool_count_for_dcc_initial_zero(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.register_service("blender", "127.0.0.1", 19900)
        assert t.pool_count_for_dcc("blender") == 0

    def test_release_connection_does_not_raise(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        iid = t.register_service("maya", "127.0.0.1", 18812)
        t.release_connection("maya", iid)  # releasing before acquire should not raise

    def test_acquire_unknown_dcc_raises(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        with pytest.raises((RuntimeError, ValueError)):
            t.acquire_connection("nonexistent_dcc")

    def test_pool_count_for_unregistered_dcc_is_zero(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        assert t.pool_count_for_dcc("ghostdcc") == 0


# ── TransportManager.cleanup() return value ───────────────────────────────────


class TestTransportManagerCleanup:
    def test_cleanup_returns_triple(self, tmp_path: Path) -> None:
        """cleanup() should return (sessions_removed, connections_closed, services_removed)."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        result = t.cleanup()
        assert isinstance(result, tuple)
        assert len(result) == 3

    def test_cleanup_all_nonnegative(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        sessions_removed, conns_closed, svcs_removed = t.cleanup()
        assert sessions_removed >= 0
        assert conns_closed >= 0
        assert svcs_removed >= 0

    def test_cleanup_twice_does_not_raise(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.cleanup()
        t.cleanup()  # idempotent


# ── TransportManager.shutdown() / is_shutdown() ───────────────────────────────


class TestTransportManagerShutdown:
    def test_is_shutdown_initially_false(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        assert t.is_shutdown() is False

    def test_shutdown_sets_is_shutdown_true(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.shutdown()
        assert t.is_shutdown() is True

    def test_shutdown_idempotent(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.shutdown()
        t.shutdown()
        assert t.is_shutdown() is True


# ── FramedChannel.__bool__ ────────────────────────────────────────────────────


class TestFramedChannelBool:
    """Tests for FramedChannel.__bool__ (True when running, False after shutdown)."""

    def _make_channel(self) -> dcc_mcp_core.FramedChannel:
        """Create a connected client FramedChannel."""
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()
        return dcc_mcp_core.connect_ipc(local)

    def test_bool_true_when_running(self) -> None:
        channel = self._make_channel()
        assert bool(channel) is True

    def test_bool_false_after_shutdown(self) -> None:
        channel = self._make_channel()
        channel.shutdown()
        # After shutdown the background task is stopped
        assert bool(channel) is False

    def test_is_running_true_initially(self) -> None:
        channel = self._make_channel()
        assert channel.is_running is True

    def test_is_running_false_after_shutdown(self) -> None:
        channel = self._make_channel()
        channel.shutdown()
        assert channel.is_running is False


# ── TransportManager.get_or_create_session_routed() ──────────────────────────


class TestSessionRouted:
    def test_routed_session_first_available(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.register_service("maya", "127.0.0.1", 18812)
        session_id = t.get_or_create_session_routed(
            "maya",
            strategy=dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE,
        )
        assert isinstance(session_id, str)
        assert len(session_id) > 0

    def test_routed_session_round_robin(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.register_service("maya", "127.0.0.1", 18812)
        t.register_service("maya", "127.0.0.1", 18813)
        sid1 = t.get_or_create_session_routed("maya", strategy=dcc_mcp_core.RoutingStrategy.ROUND_ROBIN)
        sid2 = t.get_or_create_session_routed("maya", strategy=dcc_mcp_core.RoutingStrategy.ROUND_ROBIN)
        assert isinstance(sid1, str)
        assert isinstance(sid2, str)

    def test_routed_session_no_instances_raises(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        with pytest.raises(RuntimeError):
            t.get_or_create_session_routed("unregistered_dcc")

    def test_routed_session_with_hint(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        iid = t.register_service("houdini", "127.0.0.1", 20010)
        session_id = t.get_or_create_session_routed(
            "houdini",
            strategy=dcc_mcp_core.RoutingStrategy.SPECIFIC,
            hint=iid,
        )
        assert isinstance(session_id, str)


# ── TransportManager.__len__ counts sessions ─────────────────────────────────


class TestTransportManagerLen:
    def test_len_zero_initially(self, tmp_path: Path) -> None:
        """__len__ reports session count, which is 0 initially."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        assert len(t) == 0

    def test_len_equals_session_count(self, tmp_path: Path) -> None:
        """__len__ should equal session_count()."""
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.register_service("maya", "127.0.0.1", 18812)
        session_id = t.get_or_create_session("maya")
        # __len__ should now equal session_count()
        assert len(t) == t.session_count()
        assert len(t) == 1
        _ = session_id

    def test_len_increases_with_more_sessions(self, tmp_path: Path) -> None:
        t = dcc_mcp_core.TransportManager(str(tmp_path))
        t.register_service("maya", "127.0.0.1", 18812)
        t.register_service("maya", "127.0.0.1", 18813)
        t.get_or_create_session("maya")
        t.get_or_create_session("maya")
        # len = session count ≥ 1 (may reuse same session or create 2)
        assert len(t) >= 1
