"""Tests for Transport Python bindings — TransportManager, ServiceEntry, ServiceStatus."""

# Import future modules
from __future__ import annotations

# Import built-in modules
from pathlib import Path

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core


class TestServiceStatus:
    def test_enum_values(self) -> None:
        assert dcc_mcp_core.ServiceStatus.AVAILABLE is not None
        assert dcc_mcp_core.ServiceStatus.BUSY is not None
        assert dcc_mcp_core.ServiceStatus.UNREACHABLE is not None
        assert dcc_mcp_core.ServiceStatus.SHUTTING_DOWN is not None

    def test_repr(self) -> None:
        s = dcc_mcp_core.ServiceStatus.AVAILABLE
        assert "AVAILABLE" in repr(s)

    def test_str(self) -> None:
        s = dcc_mcp_core.ServiceStatus.BUSY
        assert str(s) == "BUSY"

    def test_equality(self) -> None:
        a = dcc_mcp_core.ServiceStatus.AVAILABLE
        b = dcc_mcp_core.ServiceStatus.AVAILABLE
        assert a == b

    def test_inequality(self) -> None:
        a = dcc_mcp_core.ServiceStatus.AVAILABLE
        b = dcc_mcp_core.ServiceStatus.BUSY
        assert a != b


class TestTransportManager:
    def test_create(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert transport is not None
        assert "TransportManager" in repr(transport)

    def test_register_and_list_instances(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        assert isinstance(instance_id, str)
        assert len(instance_id) > 0

        instances = transport.list_instances("maya")
        assert len(instances) == 1
        assert instances[0].dcc_type == "maya"
        assert instances[0].host == "127.0.0.1"
        assert instances[0].port == 18812
        transport.shutdown()

    def test_register_with_metadata(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service(
            "blender",
            "127.0.0.1",
            19000,
            version="4.2",
            scene="/tmp/scene.blend",
            metadata={"pid": "12345"},
        )
        entry = transport.get_service("blender", instance_id)
        assert entry is not None
        assert entry.version == "4.2"
        assert entry.scene == "/tmp/scene.blend"
        assert entry.metadata["pid"] == "12345"
        transport.shutdown()

    def test_deregister_service(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        removed = transport.deregister_service("maya", instance_id)
        assert removed is True
        instances = transport.list_instances("maya")
        assert len(instances) == 0
        transport.shutdown()

    def test_deregister_nonexistent(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        # Use a valid UUID format
        removed = transport.deregister_service("maya", "00000000-0000-0000-0000-000000000000")
        assert removed is False
        transport.shutdown()

    def test_list_all_services(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.register_service("blender", "127.0.0.1", 19000)
        all_services = transport.list_all_services()
        assert len(all_services) == 2
        dcc_types = {s.dcc_type for s in all_services}
        assert dcc_types == {"maya", "blender"}
        transport.shutdown()

    def test_heartbeat(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        result = transport.heartbeat("maya", instance_id)
        assert result is True
        transport.shutdown()

    def test_update_service_status(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        updated = transport.update_service_status("maya", instance_id, dcc_mcp_core.ServiceStatus.BUSY)
        assert updated is True
        entry = transport.get_service("maya", instance_id)
        assert entry is not None
        assert entry.status == dcc_mcp_core.ServiceStatus.BUSY
        transport.shutdown()

    def test_update_status_nonexistent(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        updated = transport.update_service_status(
            "maya",
            "00000000-0000-0000-0000-000000000000",
            dcc_mcp_core.ServiceStatus.BUSY,
        )
        assert updated is False
        transport.shutdown()

    def test_get_service_none(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        entry = transport.get_service("maya", "00000000-0000-0000-0000-000000000000")
        assert entry is None
        transport.shutdown()

    def test_session_management(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya", instance_id)
        assert isinstance(session_id, str)
        assert len(session_id) > 0
        assert transport.session_count() == 1

        session = transport.get_session(session_id)
        assert session is not None
        assert session["dcc_type"] == "maya"
        transport.shutdown()

    def test_session_record_success(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya", instance_id)
        transport.record_success(session_id, 50)
        session = transport.get_session(session_id)
        assert session["request_count"] == 1
        transport.shutdown()

    def test_session_record_error(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya", instance_id)
        transport.record_error(session_id, 100, "timeout")
        session = transport.get_session(session_id)
        assert session["error_count"] == 1
        transport.shutdown()

    def test_close_session(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya", instance_id)
        closed = transport.close_session(session_id)
        assert closed is True
        assert transport.session_count() == 0
        transport.shutdown()

    def test_list_sessions(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.register_service("blender", "127.0.0.1", 19000)
        transport.get_or_create_session("maya")
        transport.get_or_create_session("blender")
        sessions = transport.list_sessions()
        assert len(sessions) == 2
        transport.shutdown()

    def test_list_sessions_for_dcc(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.register_service("blender", "127.0.0.1", 19000)
        transport.get_or_create_session("maya")
        transport.get_or_create_session("blender")
        maya_sessions = transport.list_sessions_for_dcc("maya")
        assert len(maya_sessions) == 1
        assert maya_sessions[0]["dcc_type"] == "maya"
        transport.shutdown()

    def test_pool_size(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert transport.pool_size() == 0
        transport.shutdown()

    def test_pool_count_for_dcc(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert transport.pool_count_for_dcc("maya") == 0
        transport.shutdown()

    def test_cleanup(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        result = transport.cleanup()
        assert isinstance(result, tuple)
        assert len(result) == 3
        transport.shutdown()

    def test_shutdown_and_is_shutdown(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert transport.is_shutdown() is False
        transport.shutdown()
        assert transport.is_shutdown() is True

    def test_len(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert len(transport) == 0
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.get_or_create_session("maya")
        assert len(transport) == 1
        transport.shutdown()

    def test_invalid_uuid_raises(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        with pytest.raises(ValueError, match="invalid UUID"):
            transport.deregister_service("maya", "not-a-uuid")
        transport.shutdown()

    def test_custom_config(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(
            str(tmp_path / "registry"),
            max_connections_per_dcc=5,
            idle_timeout=60,
            heartbeat_interval=2,
            connect_timeout=5,
            reconnect_max_retries=5,
        )
        assert transport is not None
        transport.shutdown()

    def test_session_count(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        assert transport.session_count() == 0
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.get_or_create_session("maya")
        assert transport.session_count() == 1
        transport.shutdown()

    def test_list_all_instances(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        transport.register_service("houdini", "127.0.0.1", 19100)
        all_inst = transport.list_all_instances()
        assert len(all_inst) == 2
        dcc_types = {e.dcc_type for e in all_inst}
        assert dcc_types == {"maya", "houdini"}
        transport.shutdown()

    def test_begin_reconnect_returns_backoff_ms(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya")
        backoff_ms = transport.begin_reconnect(session_id)
        assert isinstance(backoff_ms, int)
        assert backoff_ms > 0
        transport.shutdown()

    def test_reconnect_success_after_begin_reconnect(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session("maya")
        transport.begin_reconnect(session_id)
        # Should not raise
        transport.reconnect_success(session_id)
        session = transport.get_session(session_id)
        assert session is not None
        transport.shutdown()

    def test_get_or_create_session_routed_first_available(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("blender", "127.0.0.1", 19000)
        session_id = transport.get_or_create_session_routed("blender", dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE)
        assert isinstance(session_id, str)
        assert len(session_id) > 0
        transport.shutdown()

    def test_get_or_create_session_routed_round_robin(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        sid1 = transport.get_or_create_session_routed("maya", dcc_mcp_core.RoutingStrategy.ROUND_ROBIN)
        sid2 = transport.get_or_create_session_routed("maya", dcc_mcp_core.RoutingStrategy.ROUND_ROBIN)
        # Both sessions belong to the same (only) instance so may be the same session
        assert isinstance(sid1, str)
        assert isinstance(sid2, str)
        transport.shutdown()

    def test_get_or_create_session_routed_no_strategy(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        session_id = transport.get_or_create_session_routed("maya")
        assert isinstance(session_id, str)
        transport.shutdown()

    def test_find_best_service_returns_entry(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        best = transport.find_best_service("maya")
        assert best is not None
        assert best.dcc_type == "maya"
        assert best.host == "127.0.0.1"
        assert best.port == 18812
        transport.shutdown()

    def test_find_best_service_no_instances_raises(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        with pytest.raises(RuntimeError):
            transport.find_best_service("nonexistent-dcc")
        transport.shutdown()

    def test_rank_services_returns_sorted_list(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        transport.register_service("maya", "127.0.0.1", 18812)
        ranked = transport.rank_services("maya")
        assert isinstance(ranked, list)
        assert len(ranked) == 1
        assert ranked[0].dcc_type == "maya"
        transport.shutdown()

    def test_rank_services_no_instances_raises(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        with pytest.raises(RuntimeError):
            transport.rank_services("nonexistent")
        transport.shutdown()

    def test_rank_services_excludes_unreachable(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        iid = transport.register_service("maya", "127.0.0.1", 18812)
        transport.update_service_status("maya", iid, dcc_mcp_core.ServiceStatus.UNREACHABLE)
        with pytest.raises(RuntimeError):
            transport.rank_services("maya")
        transport.shutdown()

    def test_find_best_service_prefer_available_over_busy(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        busy_id = transport.register_service("maya", "127.0.0.1", 18812)
        available_id = transport.register_service("maya", "127.0.0.1", 18813)
        transport.update_service_status("maya", busy_id, dcc_mcp_core.ServiceStatus.BUSY)
        best = transport.find_best_service("maya")
        # The AVAILABLE instance should be preferred
        assert best.instance_id == available_id
        transport.shutdown()

    def test_begin_reconnect_invalid_session_raises(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        with pytest.raises((RuntimeError, ValueError)):
            transport.begin_reconnect("00000000-0000-0000-0000-000000000000")
        transport.shutdown()

    def test_reconnect_success_invalid_session_raises(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        with pytest.raises((RuntimeError, ValueError)):
            transport.reconnect_success("00000000-0000-0000-0000-000000000000")
        transport.shutdown()

    def test_bind_and_register_returns_instance_and_listener(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        result = transport.bind_and_register("maya")
        assert isinstance(result, tuple)
        assert len(result) == 2
        instance_id, listener = result
        assert isinstance(instance_id, str)
        assert len(instance_id) > 0
        # Instance should be registered
        instances = transport.list_instances("maya")
        assert len(instances) == 1
        # listener has local_address and transport_name but no shutdown
        addr = listener.local_address()
        assert addr is not None
        transport.shutdown()

    def test_bind_and_register_with_version_and_metadata(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id, listener = transport.bind_and_register("houdini", version="20.5", metadata={"pid": "9999"})
        entry = transport.get_service("houdini", instance_id)
        assert entry is not None
        assert entry.version == "20.5"
        assert entry.metadata["pid"] == "9999"
        assert listener.transport_name is not None
        transport.shutdown()


class TestServiceEntry:
    def test_attributes(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812, version="2024.2")
        entry = transport.get_service("maya", instance_id)
        assert entry is not None
        assert entry.dcc_type == "maya"
        assert entry.instance_id == instance_id
        assert entry.host == "127.0.0.1"
        assert entry.port == 18812
        assert entry.version == "2024.2"
        assert entry.scene is None
        assert isinstance(entry.metadata, dict)
        assert entry.status == dcc_mcp_core.ServiceStatus.AVAILABLE
        transport.shutdown()

    def test_repr(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("maya", "127.0.0.1", 18812)
        entry = transport.get_service("maya", instance_id)
        r = repr(entry)
        assert "ServiceEntry" in r
        assert "maya" in r
        assert "18812" in r
        transport.shutdown()

    def test_to_dict(self, tmp_path: Path) -> None:
        transport = dcc_mcp_core.TransportManager(str(tmp_path / "registry"))
        instance_id = transport.register_service("blender", "localhost", 19000, version="4.2")
        entry = transport.get_service("blender", instance_id)
        d = entry.to_dict()
        assert d["dcc_type"] == "blender"
        assert d["host"] == "localhost"
        assert d["port"] == 19000
        assert d["version"] == "4.2"
        assert d["status"] == "AVAILABLE"
        assert isinstance(d["metadata"], dict)
        transport.shutdown()


class TestTransportAddressParse:
    """Tests for TransportAddress.parse() — URI string parsing."""

    def test_parse_tcp(self) -> None:
        addr = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        assert addr.is_tcp
        assert addr.scheme == "tcp"
        assert "9000" in str(addr)

    def test_parse_pipe(self) -> None:
        addr = dcc_mcp_core.TransportAddress.parse("pipe://dcc-mcp-maya")
        assert addr.is_named_pipe
        assert addr.scheme == "pipe"

    def test_parse_unix(self) -> None:
        addr = dcc_mcp_core.TransportAddress.parse("unix:///tmp/dcc-mcp-test.sock")
        assert addr.is_unix_socket
        assert addr.scheme == "unix"

    def test_parse_invalid_raises(self) -> None:
        with pytest.raises(ValueError):
            dcc_mcp_core.TransportAddress.parse("not-a-uri")

    def test_parse_unknown_scheme_raises(self) -> None:
        with pytest.raises(ValueError):
            dcc_mcp_core.TransportAddress.parse("ftp://example.com")

    def test_parse_repr(self) -> None:
        addr = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:8080")
        r = repr(addr)
        assert "TransportAddress" in r
        assert "8080" in r

    def test_parse_equality(self) -> None:
        a = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        b = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 9000)
        assert a == b

    def test_parse_hash(self) -> None:
        a = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        b = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        assert hash(a) == hash(b)

    def test_is_local(self) -> None:
        tcp = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        assert tcp.is_local is True

        remote = dcc_mcp_core.TransportAddress.parse("tcp://192.168.1.100:9000")
        assert remote.is_local is False

    def test_to_connection_string(self) -> None:
        addr = dcc_mcp_core.TransportAddress.parse("tcp://127.0.0.1:9000")
        assert addr.to_connection_string() == "tcp://127.0.0.1:9000"


class TestTransportScheme:
    """Tests for TransportScheme enum and select_address()."""

    def test_enum_values_exist(self) -> None:
        assert dcc_mcp_core.TransportScheme.AUTO is not None
        assert dcc_mcp_core.TransportScheme.TCP_ONLY is not None
        assert dcc_mcp_core.TransportScheme.PREFER_NAMED_PIPE is not None
        assert dcc_mcp_core.TransportScheme.PREFER_UNIX_SOCKET is not None
        assert dcc_mcp_core.TransportScheme.PREFER_IPC is not None

    def test_repr(self) -> None:
        s = dcc_mcp_core.TransportScheme.AUTO
        assert "AUTO" in repr(s)

    def test_str(self) -> None:
        assert str(dcc_mcp_core.TransportScheme.TCP_ONLY) == "TCP_ONLY"
        assert str(dcc_mcp_core.TransportScheme.PREFER_IPC) == "PREFER_IPC"

    def test_equality(self) -> None:
        a = dcc_mcp_core.TransportScheme.AUTO
        b = dcc_mcp_core.TransportScheme.AUTO
        assert a == b

    def test_inequality(self) -> None:
        a = dcc_mcp_core.TransportScheme.AUTO
        b = dcc_mcp_core.TransportScheme.TCP_ONLY
        assert a != b

    def test_select_address_tcp_only(self) -> None:
        scheme = dcc_mcp_core.TransportScheme.TCP_ONLY
        addr = scheme.select_address("maya", "127.0.0.1", 18812)
        assert addr.is_tcp

    def test_select_address_auto_local(self) -> None:
        scheme = dcc_mcp_core.TransportScheme.AUTO
        addr = scheme.select_address("maya", "127.0.0.1", 18812, pid=12345)
        # AUTO on local host should prefer IPC or fallback to TCP
        assert addr.is_tcp or addr.is_named_pipe or addr.is_unix_socket

    def test_select_address_prefer_ipc(self) -> None:
        scheme = dcc_mcp_core.TransportScheme.PREFER_IPC
        addr = scheme.select_address("houdini", "127.0.0.1", 19000, pid=99999)
        # On Windows: named_pipe; on Unix: unix_socket; fallback: tcp
        assert addr.is_tcp or addr.is_named_pipe or addr.is_unix_socket

    def test_select_address_returns_transport_address(self) -> None:
        scheme = dcc_mcp_core.TransportScheme.TCP_ONLY
        result = scheme.select_address("blender", "localhost", 20000)
        assert isinstance(result, dcc_mcp_core.TransportAddress)


class TestIpcListener:
    """Tests for IpcListener Python bindings."""

    def test_bind_tcp_ephemeral(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        assert listener is not None

    def test_local_address_is_tcp(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()
        assert local.is_tcp

    def test_local_address_non_zero_port(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()
        conn_str = local.to_connection_string()
        port = int(conn_str.rsplit(":", 1)[-1])
        assert port > 0

    def test_transport_name(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        assert listener.transport_name == "tcp"

    def test_repr(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        r = repr(listener)
        assert "IpcListener" in r
        assert "tcp" in r

    def test_into_handle(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        handle = listener.into_handle()
        assert handle is not None
        assert isinstance(handle, dcc_mcp_core.ListenerHandle)

    def test_into_handle_twice_raises(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.into_handle()

    def test_local_address_after_into_handle_raises(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.local_address()

    def test_bind_invalid_address_raises(self) -> None:
        addr = dcc_mcp_core.TransportAddress.tcp("999.999.999.999", 0)
        with pytest.raises(RuntimeError):
            dcc_mcp_core.IpcListener.bind(addr)


class TestListenerHandle:
    """Tests for ListenerHandle Python bindings."""

    def _make_handle(self) -> dcc_mcp_core.ListenerHandle:
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        return listener.into_handle()

    def test_accept_count_initial(self) -> None:
        handle = self._make_handle()
        assert handle.accept_count == 0

    def test_is_shutdown_initial(self) -> None:
        handle = self._make_handle()
        assert handle.is_shutdown is False

    def test_transport_name(self) -> None:
        handle = self._make_handle()
        assert handle.transport_name == "tcp"

    def test_local_address(self) -> None:
        handle = self._make_handle()
        local = handle.local_address()
        assert local.is_tcp

    def test_shutdown(self) -> None:
        handle = self._make_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_idempotent(self) -> None:
        handle = self._make_handle()
        handle.shutdown()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_repr(self) -> None:
        handle = self._make_handle()
        r = repr(handle)
        assert "ListenerHandle" in r
        assert "tcp" in r
        assert "accept_count" in r


class TestConnectIpc:
    """Tests for connect_ipc() Python function."""

    def test_connect_to_listener(self) -> None:
        """connect_ipc() should successfully connect to a listening IpcListener."""
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()

        channel = dcc_mcp_core.connect_ipc(local)
        assert channel is not None

    def test_connect_returns_framed_channel(self) -> None:
        """connect_ipc() should return a FramedChannel instance."""
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()

        channel = dcc_mcp_core.connect_ipc(local)
        assert isinstance(channel, dcc_mcp_core.FramedChannel)

    def test_connect_invalid_address_raises(self) -> None:
        """connect_ipc() should raise RuntimeError for unreachable addresses."""
        # Use a port that should be unreachable (not listening)
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 1)
        with pytest.raises(RuntimeError):
            dcc_mcp_core.connect_ipc(addr)


class TestFramedChannel:
    """Tests for FramedChannel Python bindings.

    Tests that require no active peer use a standalone listener (bind only, no accept).
    Tests that require an active peer use a ListenerHandle to keep the server alive.
    """

    def _bind_and_connect(self) -> tuple[dcc_mcp_core.ListenerHandle, dcc_mcp_core.FramedChannel]:
        """Create an IpcListener, get its handle (keeps it alive), and connect a client channel."""
        addr = dcc_mcp_core.TransportAddress.tcp("127.0.0.1", 0)
        listener = dcc_mcp_core.IpcListener.bind(addr)
        local = listener.local_address()
        handle = listener.into_handle()
        channel = dcc_mcp_core.connect_ipc(local)
        return handle, channel

    def test_is_running_initially_true(self) -> None:
        """A freshly connected channel should be running."""
        _handle, channel = self._bind_and_connect()
        assert channel.is_running is True

    def test_shutdown_stops_channel(self) -> None:
        """shutdown() should stop the channel background reader."""
        _handle, channel = self._bind_and_connect()
        channel.shutdown()
        assert channel.is_running is False

    def test_shutdown_idempotent(self) -> None:
        """Calling shutdown() multiple times should not raise."""
        _handle, channel = self._bind_and_connect()
        channel.shutdown()
        channel.shutdown()
        assert channel.is_running is False

    def test_send_request_returns_string(self) -> None:
        """send_request() should return a UUID string."""
        _handle, channel = self._bind_and_connect()
        req_id = channel.send_request("ping", b"")
        assert isinstance(req_id, str)
        assert len(req_id) == 36  # UUID format: 8-4-4-4-12

    def test_send_request_different_ids(self) -> None:
        """Each call to send_request() should return a unique ID."""
        _handle, channel = self._bind_and_connect()
        id1 = channel.send_request("method_a", b"params1")
        id2 = channel.send_request("method_b", b"params2")
        assert id1 != id2

    def test_try_recv_empty_returns_none(self) -> None:
        """try_recv() on a channel with no pending messages should return None."""
        _handle, channel = self._bind_and_connect()
        result = channel.try_recv()
        assert result is None

    def test_send_notify(self) -> None:
        """send_notify() should not raise for valid topic and data."""
        _handle, channel = self._bind_and_connect()
        # Should not raise
        channel.send_notify("scene_changed", b"scene_data")

    def test_send_response(self) -> None:
        """send_response() should not raise for valid arguments."""
        _handle, channel = self._bind_and_connect()
        req_id = channel.send_request("test_method", b"params")
        # Server-side: send back a response (in practice the server does this)
        channel.send_response(req_id, success=True, payload=b"result")

    def test_repr_contains_class_name(self) -> None:
        """repr() should contain 'FramedChannel'."""
        _handle, channel = self._bind_and_connect()
        r = repr(channel)
        assert "FramedChannel" in r

    def test_ping_timeout_raises_runtime_error(self) -> None:
        """ping() raises RuntimeError when peer does not send Pong (no Pong handler)."""
        # The listener handle does not process pings (no Pong handler), so ping times out.
        # This verifies the ping mechanism exists and raises the correct exception on timeout.
        _handle, channel = self._bind_and_connect()
        with pytest.raises(RuntimeError, match="ping"):
            channel.ping()


class TestRoutingStrategy:
    """Tests for RoutingStrategy Python enum bindings."""

    def test_first_available_exists(self) -> None:
        """FIRST_AVAILABLE enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE is not None

    def test_round_robin_exists(self) -> None:
        """ROUND_ROBIN enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.ROUND_ROBIN is not None

    def test_least_busy_exists(self) -> None:
        """LEAST_BUSY enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.LEAST_BUSY is not None

    def test_specific_exists(self) -> None:
        """SPECIFIC enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.SPECIFIC is not None

    def test_scene_match_exists(self) -> None:
        """SCENE_MATCH enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.SCENE_MATCH is not None

    def test_random_exists(self) -> None:
        """RANDOM enum variant should be accessible."""
        assert dcc_mcp_core.RoutingStrategy.RANDOM is not None

    def test_str_first_available(self) -> None:
        """str() of FIRST_AVAILABLE should return 'FIRST_AVAILABLE'."""
        assert str(dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE) == "FIRST_AVAILABLE"

    def test_str_round_robin(self) -> None:
        """str() of ROUND_ROBIN should return 'ROUND_ROBIN'."""
        assert str(dcc_mcp_core.RoutingStrategy.ROUND_ROBIN) == "ROUND_ROBIN"

    def test_str_least_busy(self) -> None:
        """str() of LEAST_BUSY should return 'LEAST_BUSY'."""
        assert str(dcc_mcp_core.RoutingStrategy.LEAST_BUSY) == "LEAST_BUSY"

    def test_str_specific(self) -> None:
        """str() of SPECIFIC should return 'SPECIFIC'."""
        assert str(dcc_mcp_core.RoutingStrategy.SPECIFIC) == "SPECIFIC"

    def test_str_scene_match(self) -> None:
        """str() of SCENE_MATCH should return 'SCENE_MATCH'."""
        assert str(dcc_mcp_core.RoutingStrategy.SCENE_MATCH) == "SCENE_MATCH"

    def test_str_random(self) -> None:
        """str() of RANDOM should return 'RANDOM'."""
        assert str(dcc_mcp_core.RoutingStrategy.RANDOM) == "RANDOM"

    def test_repr_contains_class_and_variant(self) -> None:
        """repr() should contain 'RoutingStrategy.' prefix."""
        r = repr(dcc_mcp_core.RoutingStrategy.ROUND_ROBIN)
        assert "RoutingStrategy." in r
        assert "ROUND_ROBIN" in r

    def test_equality_same_variant(self) -> None:
        """Same variants should be equal."""
        assert dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE == dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE

    def test_inequality_different_variants(self) -> None:
        """Different variants should not be equal."""
        assert dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE != dcc_mcp_core.RoutingStrategy.ROUND_ROBIN

    def test_all_variants_are_distinct(self) -> None:
        """All six variants must be distinct from each other."""
        variants = [
            dcc_mcp_core.RoutingStrategy.FIRST_AVAILABLE,
            dcc_mcp_core.RoutingStrategy.ROUND_ROBIN,
            dcc_mcp_core.RoutingStrategy.LEAST_BUSY,
            dcc_mcp_core.RoutingStrategy.SPECIFIC,
            dcc_mcp_core.RoutingStrategy.SCENE_MATCH,
            dcc_mcp_core.RoutingStrategy.RANDOM,
        ]
        # All 6 variants must be pairwise distinct.
        for i, a in enumerate(variants):
            for j, b in enumerate(variants):
                if i != j:
                    assert a != b, f"variants[{i}] == variants[{j}] unexpectedly"


class TestMessageCodec:
    """Tests for encode_request / encode_response / encode_notify / decode_envelope.

    These functions provide low-level framed message codec support for DCC-side
    Python servers (e.g. dcc-mcp-rpyc lightweight server).
    """

    def test_encode_request_returns_bytes(self) -> None:
        """encode_request() should return bytes."""
        frame = dcc_mcp_core.encode_request("execute_python", b"params")
        assert isinstance(frame, bytes)

    def test_encode_request_has_length_prefix(self) -> None:
        """encode_request() result must be at least 4 bytes (length prefix)."""
        frame = dcc_mcp_core.encode_request("ping")
        assert len(frame) >= 4

    def test_encode_request_length_prefix_matches_payload(self) -> None:
        """The 4-byte length prefix must match the actual payload size."""
        frame = dcc_mcp_core.encode_request("test", b"hello")
        length = int.from_bytes(frame[:4], "big")
        assert length == len(frame) - 4

    def test_encode_request_default_params_empty(self) -> None:
        """encode_request() with no params should produce a valid frame."""
        frame = dcc_mcp_core.encode_request("ping")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "ping"
        assert msg["params"] == b""

    def test_decode_envelope_request_roundtrip(self) -> None:
        """encode_request() + decode_envelope() should roundtrip cleanly."""
        frame = dcc_mcp_core.encode_request("execute_python", b"cmds.sphere()")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "execute_python"
        assert msg["params"] == b"cmds.sphere()"
        assert isinstance(msg["id"], str)
        assert len(msg["id"]) == 36  # UUID format

    def test_encode_response_success_roundtrip(self) -> None:
        """encode_response(success=True) + decode_envelope() should roundtrip cleanly."""
        req_frame = dcc_mcp_core.encode_request("test")
        req_id = dcc_mcp_core.decode_envelope(req_frame[4:])["id"]

        resp_frame = dcc_mcp_core.encode_response(req_id, success=True, payload=b"result")
        msg = dcc_mcp_core.decode_envelope(resp_frame[4:])
        assert msg["type"] == "response"
        assert msg["id"] == req_id
        assert msg["success"] is True
        assert msg["payload"] == b"result"
        assert msg["error"] is None

    def test_encode_response_error_roundtrip(self) -> None:
        """encode_response(success=False) + decode_envelope() should include error string."""
        req_frame = dcc_mcp_core.encode_request("bad_method")
        req_id = dcc_mcp_core.decode_envelope(req_frame[4:])["id"]

        resp_frame = dcc_mcp_core.encode_response(req_id, success=False, error="unknown method")
        msg = dcc_mcp_core.decode_envelope(resp_frame[4:])
        assert msg["type"] == "response"
        assert msg["success"] is False
        assert msg["error"] == "unknown method"

    def test_encode_response_invalid_uuid_raises(self) -> None:
        """encode_response() should raise ValueError for invalid UUID strings."""
        with pytest.raises((ValueError, RuntimeError)):
            dcc_mcp_core.encode_response("not-a-uuid", success=True)

    def test_encode_notify_roundtrip(self) -> None:
        """encode_notify() + decode_envelope() should roundtrip cleanly."""
        frame = dcc_mcp_core.encode_notify("scene_changed", b"event_data")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert msg["topic"] == "scene_changed"
        assert msg["data"] == b"event_data"

    def test_encode_notify_no_data(self) -> None:
        """encode_notify() with no data should produce a valid frame."""
        frame = dcc_mcp_core.encode_notify("render_complete")
        msg = dcc_mcp_core.decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert msg["topic"] == "render_complete"
        assert msg["data"] == b""

    def test_decode_envelope_invalid_bytes_raises(self) -> None:
        """decode_envelope() should raise RuntimeError for invalid MessagePack data."""
        with pytest.raises(RuntimeError):
            dcc_mcp_core.decode_envelope(b"not valid msgpack data at all")

    def test_encode_request_unique_ids(self) -> None:
        """Each encode_request() call should produce a unique message ID."""
        f1 = dcc_mcp_core.encode_request("method")
        f2 = dcc_mcp_core.encode_request("method")
        id1 = dcc_mcp_core.decode_envelope(f1[4:])["id"]
        id2 = dcc_mcp_core.decode_envelope(f2[4:])["id"]
        assert id1 != id2
