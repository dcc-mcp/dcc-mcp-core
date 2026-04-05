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
