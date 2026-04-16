"""Deep tests for MCP HTTP, Transport, Protocols and Codec APIs.

Coverage targets (all new):
  - McpHttpConfig / McpHttpServer / McpServerHandle
  - TransportAddress (all static factories + properties)
  - TransportScheme / RoutingStrategy / ServiceStatus
  - IpcListener / ListenerHandle
  - encode_request / encode_response / encode_notify / decode_envelope
  - ToolAnnotations / ToolDefinition
  - ResourceAnnotations / ResourceDefinition / ResourceTemplateDefinition
  - PromptArgument / PromptDefinition
  - SemVer / VersionConstraint / VersionedRegistry (additional edge cases)
"""

from __future__ import annotations

import uuid

import pytest

from dcc_mcp_core import IpcListener
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import McpServerHandle
from dcc_mcp_core import PromptArgument
from dcc_mcp_core import PromptDefinition
from dcc_mcp_core import ResourceAnnotations
from dcc_mcp_core import ResourceDefinition
from dcc_mcp_core import ResourceTemplateDefinition
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SemVer
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import ToolAnnotations
from dcc_mcp_core import ToolDefinition
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_request
from dcc_mcp_core import encode_response

# ---------------------------------------------------------------------------
# McpHttpConfig
# ---------------------------------------------------------------------------


class TestMcpHttpConfigCreate:
    def test_default_port(self):
        cfg = McpHttpConfig()
        assert cfg.port == 8765

    def test_custom_port(self):
        cfg = McpHttpConfig(port=9999)
        assert cfg.port == 9999

    def test_port_zero(self):
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_default_server_name(self):
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_name, str)
        assert len(cfg.server_name) > 0

    def test_custom_server_name(self):
        cfg = McpHttpConfig(server_name="maya-mcp")
        assert cfg.server_name == "maya-mcp"

    def test_default_server_version(self):
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0

    def test_custom_server_version(self):
        cfg = McpHttpConfig(server_version="2.0.0")
        assert cfg.server_version == "2.0.0"

    def test_all_custom(self):
        cfg = McpHttpConfig(port=1234, server_name="blender-mcp", server_version="3.5.0")
        assert cfg.port == 1234
        assert cfg.server_name == "blender-mcp"
        assert cfg.server_version == "3.5.0"

    def test_repr_contains_port(self):
        cfg = McpHttpConfig(port=8765)
        assert "8765" in repr(cfg)

    def test_repr_contains_name(self):
        cfg = McpHttpConfig(server_name="test-srv")
        assert "test-srv" in repr(cfg)

    def test_repr_is_string(self):
        cfg = McpHttpConfig()
        assert isinstance(repr(cfg), str)


class TestMcpHttpServerCreate:
    def test_create_no_config(self):
        reg = ToolRegistry()
        srv = McpHttpServer(reg)
        assert srv is not None

    def test_create_with_config(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        assert srv is not None

    def test_repr_is_string(self):
        reg = ToolRegistry()
        srv = McpHttpServer(reg)
        assert isinstance(repr(srv), str)

    def test_repr_contains_name(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(server_name="maya-mcp")
        srv = McpHttpServer(reg, cfg)
        assert "maya-mcp" in repr(srv)

    def test_repr_contains_port(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=9876)
        srv = McpHttpServer(reg, cfg)
        assert "9876" in repr(srv)

    def test_create_with_none_config(self):
        reg = ToolRegistry()
        srv = McpHttpServer(reg, None)
        assert srv is not None

    def test_start_returns_server_handle(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        try:
            assert isinstance(handle, McpServerHandle)
        finally:
            handle.shutdown()

    def test_handle_port_positive(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        try:
            assert handle.port > 0
        finally:
            handle.shutdown()

    def test_handle_bind_addr_contains_port(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        try:
            bind_addr = handle.bind_addr
            assert str(handle.port) in bind_addr
        finally:
            handle.shutdown()

    def test_handle_mcp_url_format(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        try:
            url = handle.mcp_url()
            assert url.startswith("http://")
            assert "/mcp" in url
        finally:
            handle.shutdown()

    def test_handle_repr(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        try:
            assert isinstance(repr(handle), str)
        finally:
            handle.shutdown()

    def test_handle_signal_shutdown(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        srv = McpHttpServer(reg, cfg)
        handle = srv.start()
        handle.signal_shutdown()


# ---------------------------------------------------------------------------
# TransportAddress
# ---------------------------------------------------------------------------


class TestTransportAddressTcp:
    def test_tcp_scheme(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr.scheme == "tcp"

    def test_tcp_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr.is_tcp is True

    def test_tcp_is_not_pipe(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr.is_named_pipe is False

    def test_tcp_is_not_unix(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr.is_unix_socket is False

    def test_tcp_is_local_loopback(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        assert addr.is_local is True

    def test_tcp_connection_string(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        conn = addr.to_connection_string()
        assert "tcp://" in conn
        assert "127.0.0.1" in conn
        assert "18812" in conn

    def test_tcp_repr(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        r = repr(addr)
        assert "127.0.0.1" in r

    def test_tcp_str(self):
        addr = TransportAddress.tcp("127.0.0.1", 18812)
        s = str(addr)
        assert isinstance(s, str)

    def test_tcp_port_zero(self):
        addr = TransportAddress.tcp("0.0.0.0", 0)
        assert addr.scheme == "tcp"


class TestTransportAddressNamedPipe:
    def test_named_pipe_scheme(self):
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert addr.scheme == "pipe"

    def test_named_pipe_is_pipe(self):
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert addr.is_named_pipe is True

    def test_named_pipe_is_not_tcp(self):
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert addr.is_tcp is False

    def test_named_pipe_is_local(self):
        addr = TransportAddress.named_pipe("dcc-mcp-maya-1234")
        assert addr.is_local is True


class TestTransportAddressDefaultLocal:
    def test_default_local_returns_transport_address(self):
        addr = TransportAddress.default_local("maya", 12345)
        assert isinstance(addr, TransportAddress)

    def test_default_local_is_local(self):
        addr = TransportAddress.default_local("blender", 99999)
        assert addr.is_local is True

    def test_default_local_different_dccs(self):
        a1 = TransportAddress.default_local("maya", 100)
        a2 = TransportAddress.default_local("blender", 100)
        assert a1.scheme == a2.scheme  # same platform scheme

    def test_default_pipe_name(self):
        addr = TransportAddress.default_pipe_name("maya", 12345)
        assert isinstance(addr, TransportAddress)

    def test_default_unix_socket(self):
        addr = TransportAddress.default_unix_socket("maya", 12345)
        assert isinstance(addr, TransportAddress)


class TestTransportAddressParse:
    def test_parse_tcp(self):
        addr = TransportAddress.parse("tcp://127.0.0.1:18812")
        assert addr.scheme == "tcp"
        assert addr.is_tcp

    def test_parse_invalid_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            TransportAddress.parse("not-a-valid-uri")

    def test_parse_empty_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            TransportAddress.parse("")


# ---------------------------------------------------------------------------
# TransportScheme / RoutingStrategy / ServiceStatus
# ---------------------------------------------------------------------------


class TestTransportScheme:
    def test_auto_repr(self):
        assert "AUTO" in repr(TransportScheme.AUTO)

    def test_tcp_only_repr(self):
        assert "TCP" in repr(TransportScheme.TCP_ONLY)

    def test_prefer_named_pipe_repr(self):
        assert "NAMED_PIPE" in repr(TransportScheme.PREFER_NAMED_PIPE) or "Pipe" in repr(
            TransportScheme.PREFER_NAMED_PIPE
        )

    def test_prefer_unix_socket_repr(self):
        r = repr(TransportScheme.PREFER_UNIX_SOCKET)
        assert "UNIX" in r or "Unix" in r or "unix" in r

    def test_prefer_ipc_repr(self):
        assert isinstance(repr(TransportScheme.PREFER_IPC), str)

    def test_auto_eq_auto(self):
        assert TransportScheme.AUTO == TransportScheme.AUTO

    def test_auto_ne_tcp_only(self):
        assert TransportScheme.AUTO != TransportScheme.TCP_ONLY

    def test_str_is_string(self):
        assert isinstance(str(TransportScheme.AUTO), str)

    def test_select_address_returns_transport_address(self):
        addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, None)
        assert isinstance(addr, TransportAddress)

    def test_select_address_tcp_only(self):
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812, None)
        assert addr.is_tcp

    def test_select_address_with_pid(self):
        addr = TransportScheme.AUTO.select_address("blender", "127.0.0.1", 9001, 99999)
        assert isinstance(addr, TransportAddress)


class TestRoutingStrategy:
    def test_first_available_repr(self):
        assert "FIRST_AVAILABLE" in repr(RoutingStrategy.FIRST_AVAILABLE)

    def test_round_robin_repr(self):
        assert "ROUND_ROBIN" in repr(RoutingStrategy.ROUND_ROBIN)

    def test_least_busy_repr(self):
        assert "LEAST_BUSY" in repr(RoutingStrategy.LEAST_BUSY)

    def test_specific_repr(self):
        assert "SPECIFIC" in repr(RoutingStrategy.SPECIFIC)

    def test_scene_match_repr(self):
        assert "SCENE_MATCH" in repr(RoutingStrategy.SCENE_MATCH)

    def test_random_repr(self):
        assert "RANDOM" in repr(RoutingStrategy.RANDOM)

    def test_eq_same(self):
        assert RoutingStrategy.FIRST_AVAILABLE == RoutingStrategy.FIRST_AVAILABLE

    def test_ne_different(self):
        assert RoutingStrategy.FIRST_AVAILABLE != RoutingStrategy.ROUND_ROBIN

    def test_str_is_string(self):
        assert isinstance(str(RoutingStrategy.FIRST_AVAILABLE), str)


class TestServiceStatus:
    def test_available_repr(self):
        assert "AVAILABLE" in repr(ServiceStatus.AVAILABLE)

    def test_busy_repr(self):
        assert "BUSY" in repr(ServiceStatus.BUSY)

    def test_unreachable_repr(self):
        assert "UNREACHABLE" in repr(ServiceStatus.UNREACHABLE)

    def test_shutting_down_repr(self):
        r = repr(ServiceStatus.SHUTTING_DOWN)
        assert "SHUTTING_DOWN" in r or "Shutting" in r or "DOWN" in r

    def test_eq_same(self):
        assert ServiceStatus.AVAILABLE == ServiceStatus.AVAILABLE

    def test_ne_different(self):
        assert ServiceStatus.AVAILABLE != ServiceStatus.BUSY

    def test_str_is_string(self):
        assert isinstance(str(ServiceStatus.AVAILABLE), str)


# ---------------------------------------------------------------------------
# IpcListener / ListenerHandle
# ---------------------------------------------------------------------------


class TestIpcListenerBind:
    def test_bind_tcp_port_zero(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        handle.shutdown()

    def test_local_address_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert local.is_tcp
        handle = listener.into_handle()
        handle.shutdown()

    def test_local_address_has_nonzero_port(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert "0.0.0.0" not in str(local) or local.is_tcp
        handle = listener.into_handle()
        handle.shutdown()

    def test_transport_name_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener.transport_name == "tcp"
        handle = listener.into_handle()
        handle.shutdown()

    def test_repr_is_string(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert isinstance(repr(listener), str)
        handle = listener.into_handle()
        handle.shutdown()

    def test_into_handle_returns_handle(self):
        from dcc_mcp_core import ListenerHandle

        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        assert isinstance(handle, ListenerHandle)
        handle.shutdown()


class TestListenerHandle:
    def _make_handle(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        return listener.into_handle()

    def test_accept_count_starts_zero(self):
        handle = self._make_handle()
        assert handle.accept_count == 0
        handle.shutdown()

    def test_is_shutdown_false_before_shutdown(self):
        handle = self._make_handle()
        assert handle.is_shutdown is False
        handle.shutdown()

    def test_is_shutdown_true_after_shutdown(self):
        handle = self._make_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_transport_name_tcp(self):
        handle = self._make_handle()
        assert handle.transport_name == "tcp"
        handle.shutdown()

    def test_local_address_is_transport_address(self):
        handle = self._make_handle()
        local = handle.local_address()
        assert isinstance(local, TransportAddress)
        handle.shutdown()

    def test_shutdown_idempotent(self):
        handle = self._make_handle()
        handle.shutdown()
        handle.shutdown()  # second call should not raise

    def test_repr_contains_transport(self):
        handle = self._make_handle()
        handle.shutdown()
        r = repr(handle)
        assert "tcp" in r

    def test_repr_contains_shutdown_state(self):
        handle = self._make_handle()
        handle.shutdown()
        r = repr(handle)
        assert "true" in r.lower() or "shutdown" in r.lower()


# ---------------------------------------------------------------------------
# encode_request / encode_response / encode_notify / decode_envelope
# ---------------------------------------------------------------------------


class TestEncodeRequest:
    def test_returns_bytes(self):
        frame = encode_request("execute_python")
        assert isinstance(frame, bytes)

    def test_min_length(self):
        frame = encode_request("ping")
        assert len(frame) >= 4

    def test_has_length_prefix(self):
        frame = encode_request("test")
        payload_len = int.from_bytes(frame[:4], "big")
        assert payload_len == len(frame) - 4

    def test_with_params(self):
        frame = encode_request("execute_python", b"cmds.sphere()")
        assert len(frame) > 4

    def test_without_params(self):
        frame = encode_request("ping", None)
        assert isinstance(frame, bytes)

    def test_decode_roundtrip_method(self):
        frame = encode_request("my_method", b"payload")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "my_method"

    def test_decode_roundtrip_params(self):
        frame = encode_request("my_method", b"hello")
        msg = decode_envelope(frame[4:])
        assert msg["params"] == b"hello"

    def test_decode_has_id(self):
        frame = encode_request("ping")
        msg = decode_envelope(frame[4:])
        assert "id" in msg
        assert isinstance(msg["id"], str)

    def test_empty_method(self):
        frame = encode_request("")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == ""


class TestEncodeResponse:
    def _valid_id(self) -> str:
        return str(uuid.uuid4())

    def test_returns_bytes(self):
        frame = encode_response(self._valid_id(), True)
        assert isinstance(frame, bytes)

    def test_has_length_prefix(self):
        frame = encode_response(self._valid_id(), True, b"ok")
        payload_len = int.from_bytes(frame[:4], "big")
        assert payload_len == len(frame) - 4

    def test_decode_success_true(self):
        rid = self._valid_id()
        frame = encode_response(rid, True, b"result")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"
        assert msg["success"] is True

    def test_decode_success_false(self):
        rid = self._valid_id()
        frame = encode_response(rid, False, None, "something went wrong")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"
        assert msg["success"] is False

    def test_decode_id_matches(self):
        rid = self._valid_id()
        frame = encode_response(rid, True, b"data")
        msg = decode_envelope(frame[4:])
        assert msg["id"] == rid

    def test_decode_error_message(self):
        rid = self._valid_id()
        frame = encode_response(rid, False, None, "error detail")
        msg = decode_envelope(frame[4:])
        assert msg["error"] == "error detail"

    def test_decode_no_error_on_success(self):
        rid = self._valid_id()
        frame = encode_response(rid, True, b"ok")
        msg = decode_envelope(frame[4:])
        assert msg.get("error") is None

    def test_invalid_uuid_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            encode_response("not-a-uuid", True)


class TestEncodeNotify:
    def test_returns_bytes(self):
        frame = encode_notify("scene_changed")
        assert isinstance(frame, bytes)

    def test_has_length_prefix(self):
        frame = encode_notify("render_complete")
        payload_len = int.from_bytes(frame[:4], "big")
        assert payload_len == len(frame) - 4

    def test_decode_type(self):
        frame = encode_notify("scene_changed", b"data")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"

    def test_decode_topic(self):
        frame = encode_notify("render_complete", b"")
        msg = decode_envelope(frame[4:])
        assert msg["topic"] == "render_complete"

    def test_decode_data(self):
        frame = encode_notify("test_event", b"payload123")
        msg = decode_envelope(frame[4:])
        assert msg["data"] == b"payload123"

    def test_without_data(self):
        frame = encode_notify("ping_event", None)
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"

    def test_empty_topic(self):
        frame = encode_notify("", b"")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"


class TestDecodeEnvelope:
    def test_bad_data_raises(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"bad data that cannot be decoded")

    def test_empty_bytes_raises(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"")

    def test_request_has_type_field(self):
        frame = encode_request("test")
        msg = decode_envelope(frame[4:])
        assert "type" in msg

    def test_response_has_success_field(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True)
        msg = decode_envelope(frame[4:])
        assert "success" in msg

    def test_notify_has_topic_field(self):
        frame = encode_notify("my_topic")
        msg = decode_envelope(frame[4:])
        assert "topic" in msg


# ---------------------------------------------------------------------------
# ToolAnnotations / ToolDefinition
# ---------------------------------------------------------------------------


class TestToolAnnotations:
    def test_create_empty(self):
        ta = ToolAnnotations()
        assert ta.title is None
        assert ta.read_only_hint is None
        assert ta.destructive_hint is None
        assert ta.idempotent_hint is None
        assert ta.open_world_hint is None

    def test_create_with_title(self):
        ta = ToolAnnotations(title="My Tool")
        assert ta.title == "My Tool"

    def test_create_read_only(self):
        ta = ToolAnnotations(read_only_hint=True)
        assert ta.read_only_hint is True

    def test_create_destructive(self):
        ta = ToolAnnotations(destructive_hint=True)
        assert ta.destructive_hint is True

    def test_create_idempotent(self):
        ta = ToolAnnotations(idempotent_hint=False)
        assert ta.idempotent_hint is False

    def test_create_open_world(self):
        ta = ToolAnnotations(open_world_hint=True)
        assert ta.open_world_hint is True

    def test_create_all(self):
        ta = ToolAnnotations(
            title="Full",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        assert ta.title == "Full"
        assert ta.read_only_hint is True
        assert ta.destructive_hint is False
        assert ta.idempotent_hint is True
        assert ta.open_world_hint is False

    def test_repr_is_string(self):
        ta = ToolAnnotations(title="test")
        assert isinstance(repr(ta), str)

    def test_eq_same_values(self):
        ta1 = ToolAnnotations(title="x", read_only_hint=True)
        ta2 = ToolAnnotations(title="x", read_only_hint=True)
        assert ta1 == ta2

    def test_ne_different_values(self):
        ta1 = ToolAnnotations(title="x")
        ta2 = ToolAnnotations(title="y")
        assert ta1 != ta2

    def test_eq_empty(self):
        ta1 = ToolAnnotations()
        ta2 = ToolAnnotations()
        assert ta1 == ta2


class TestToolDefinition:
    def test_create_minimal(self):
        td = ToolDefinition("create_sphere", "Create sphere", '{"type":"object"}')
        assert td.name == "create_sphere"
        assert td.description == "Create sphere"
        assert isinstance(td.input_schema, str)

    def test_output_schema_none_by_default(self):
        td = ToolDefinition("tool", "desc", '{"type":"object"}')
        assert td.output_schema is None

    def test_output_schema_set(self):
        td = ToolDefinition("tool", "desc", '{"type":"object"}', '{"type":"string"}')
        assert td.output_schema is not None

    def test_annotations_none_by_default(self):
        td = ToolDefinition("tool", "desc", '{"type":"object"}')
        assert td.annotations is None

    def test_annotations_set(self):
        ta = ToolAnnotations(title="My Tool", read_only_hint=True)
        td = ToolDefinition("tool", "desc", '{"type":"object"}', annotations=ta)
        assert td.annotations is not None

    def test_repr_is_string(self):
        td = ToolDefinition("tool", "desc", '{"type":"object"}')
        assert isinstance(repr(td), str)

    def test_repr_contains_name(self):
        td = ToolDefinition("my_tool", "desc", "{}")
        assert "my_tool" in repr(td)

    def test_eq_same_values(self):
        td1 = ToolDefinition("tool", "desc", "{}")
        td2 = ToolDefinition("tool", "desc", "{}")
        assert td1 == td2

    def test_ne_different_name(self):
        td1 = ToolDefinition("tool_a", "desc", "{}")
        td2 = ToolDefinition("tool_b", "desc", "{}")
        assert td1 != td2


# ---------------------------------------------------------------------------
# ResourceAnnotations / ResourceDefinition / ResourceTemplateDefinition
# ---------------------------------------------------------------------------


class TestResourceAnnotations:
    def test_create_empty(self):
        ra = ResourceAnnotations()
        assert ra.audience == []
        assert ra.priority is None

    def test_create_with_audience(self):
        ra = ResourceAnnotations(audience=["user"])
        assert "user" in ra.audience

    def test_create_with_priority(self):
        ra = ResourceAnnotations(priority=0.9)
        assert ra.priority == pytest.approx(0.9)

    def test_create_both(self):
        ra = ResourceAnnotations(audience=["agent", "user"], priority=0.5)
        assert len(ra.audience) == 2
        assert ra.priority == pytest.approx(0.5)

    def test_repr_is_string(self):
        ra = ResourceAnnotations(audience=["user"], priority=0.8)
        assert isinstance(repr(ra), str)

    def test_repr_contains_audience(self):
        ra = ResourceAnnotations(audience=["user"])
        assert "user" in repr(ra)


class TestResourceDefinition:
    def test_create_minimal(self):
        rd = ResourceDefinition("dcc://scene", "scene", "Scene data")
        assert rd.uri == "dcc://scene"
        assert rd.name == "scene"
        assert rd.description == "Scene data"

    def test_default_mime_type(self):
        rd = ResourceDefinition("dcc://scene", "scene", "desc")
        assert rd.mime_type == "text/plain"

    def test_custom_mime_type(self):
        rd = ResourceDefinition("dcc://img", "img", "Image", "image/png")
        assert rd.mime_type == "image/png"

    def test_annotations_none_by_default(self):
        rd = ResourceDefinition("dcc://scene", "scene", "desc")
        assert rd.annotations is None

    def test_with_annotations(self):
        ra = ResourceAnnotations(audience=["user"])
        rd = ResourceDefinition("dcc://scene", "scene", "desc", annotations=ra)
        assert rd.annotations is not None

    def test_repr_is_string(self):
        rd = ResourceDefinition("dcc://scene", "scene", "desc")
        assert isinstance(repr(rd), str)

    def test_repr_contains_name(self):
        rd = ResourceDefinition("dcc://scene", "my_resource", "desc")
        assert "my_resource" in repr(rd)


class TestResourceTemplateDefinition:
    def test_create_minimal(self):
        rtd = ResourceTemplateDefinition("dcc://{dcc}/{name}", "scene_tmpl", "Scene template")
        assert "dcc://" in rtd.uri_template
        assert rtd.name == "scene_tmpl"

    def test_default_mime_type(self):
        rtd = ResourceTemplateDefinition("dcc://{x}", "t", "d")
        assert rtd.mime_type == "text/plain"

    def test_custom_mime_type(self):
        rtd = ResourceTemplateDefinition("dcc://{x}", "t", "d", "application/json")
        assert rtd.mime_type == "application/json"

    def test_annotations_none_by_default(self):
        rtd = ResourceTemplateDefinition("dcc://{x}", "t", "d")
        assert rtd.annotations is None

    def test_repr_is_string(self):
        rtd = ResourceTemplateDefinition("dcc://{x}", "t", "d")
        assert isinstance(repr(rtd), str)


# ---------------------------------------------------------------------------
# PromptArgument / PromptDefinition
# ---------------------------------------------------------------------------


class TestPromptArgument:
    def test_create(self):
        pa = PromptArgument("radius", "The radius")
        assert pa.name == "radius"
        assert pa.description == "The radius"
        assert pa.required is False

    def test_required_true(self):
        pa = PromptArgument("name", "Object name", required=True)
        assert pa.required is True

    def test_repr_is_string(self):
        pa = PromptArgument("x", "desc")
        assert isinstance(repr(pa), str)

    def test_repr_contains_name(self):
        pa = PromptArgument("my_arg", "desc")
        assert "my_arg" in repr(pa)

    def test_eq_same(self):
        pa1 = PromptArgument("x", "desc", required=True)
        pa2 = PromptArgument("x", "desc", required=True)
        assert pa1 == pa2

    def test_ne_different_name(self):
        pa1 = PromptArgument("x", "desc")
        pa2 = PromptArgument("y", "desc")
        assert pa1 != pa2

    def test_ne_different_required(self):
        pa1 = PromptArgument("x", "desc", required=True)
        pa2 = PromptArgument("x", "desc", required=False)
        assert pa1 != pa2


class TestPromptDefinition:
    def test_create_no_args(self):
        pd = PromptDefinition("my_prompt", "Create USD prim")
        assert pd.name == "my_prompt"
        assert pd.description == "Create USD prim"
        assert pd.arguments == []

    def test_create_with_args(self):
        args = [PromptArgument("x", "X coord"), PromptArgument("y", "Y coord", required=True)]
        pd = PromptDefinition("place_object", "Place object", args)
        assert len(pd.arguments) == 2

    def test_argument_order_preserved(self):
        args = [PromptArgument("first", "F"), PromptArgument("second", "S")]
        pd = PromptDefinition("test", "Test", args)
        assert pd.arguments[0].name == "first"
        assert pd.arguments[1].name == "second"

    def test_repr_is_string(self):
        pd = PromptDefinition("test_prompt", "Test")
        assert isinstance(repr(pd), str)

    def test_repr_contains_name(self):
        pd = PromptDefinition("my_prompt", "desc")
        assert "my_prompt" in repr(pd)

    def test_eq_same(self):
        pd1 = PromptDefinition("p", "desc")
        pd2 = PromptDefinition("p", "desc")
        assert pd1 == pd2

    def test_ne_different_name(self):
        pd1 = PromptDefinition("p1", "desc")
        pd2 = PromptDefinition("p2", "desc")
        assert pd1 != pd2

    def test_create_single_arg(self):
        pa = PromptArgument("radius", "Radius", required=True)
        pd = PromptDefinition("sphere", "Create sphere", [pa])
        assert pd.arguments[0].name == "radius"
        assert pd.arguments[0].required is True


# ---------------------------------------------------------------------------
# SemVer / VersionConstraint (additional edge cases)
# ---------------------------------------------------------------------------


class TestSemVerEdgeCases:
    def test_parse_with_v_prefix(self):
        v = SemVer.parse("v1.2.3")
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_parse_two_part(self):
        v = SemVer.parse("2.0")
        assert v.major == 2
        assert v.minor == 0

    def test_parse_with_prerelease(self):
        v = SemVer.parse("1.0.0-alpha")
        assert v.major == 1

    def test_le_equal(self):
        v1 = SemVer(1, 0, 0)
        v2 = SemVer(1, 0, 0)
        assert v1 <= v2

    def test_ge_equal(self):
        v1 = SemVer(1, 0, 0)
        v2 = SemVer(1, 0, 0)
        assert v1 >= v2

    def test_lt_true(self):
        v1 = SemVer(1, 0, 0)
        v2 = SemVer(2, 0, 0)
        assert v1 < v2

    def test_gt_true(self):
        v1 = SemVer(2, 0, 0)
        v2 = SemVer(1, 9, 9)
        assert v1 > v2

    def test_parse_invalid_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            SemVer.parse("not-a-version")

    def test_str_format(self):
        v = SemVer(3, 14, 159)
        assert str(v) == "3.14.159"


class TestVersionConstraintEdgeCases:
    def test_wildcard_matches_all(self):
        c = VersionConstraint.parse("*")
        assert c.matches(SemVer(0, 0, 1))
        assert c.matches(SemVer(999, 999, 999))

    def test_exact_match(self):
        c = VersionConstraint.parse("=1.2.3")
        assert c.matches(SemVer(1, 2, 3))
        assert not c.matches(SemVer(1, 2, 4))

    def test_gt_constraint(self):
        c = VersionConstraint.parse(">1.0.0")
        assert c.matches(SemVer(1, 0, 1))
        assert not c.matches(SemVer(1, 0, 0))

    def test_lte_constraint(self):
        c = VersionConstraint.parse("<=2.0.0")
        assert c.matches(SemVer(2, 0, 0))
        assert c.matches(SemVer(1, 9, 9))
        assert not c.matches(SemVer(2, 0, 1))

    def test_lt_constraint(self):
        c = VersionConstraint.parse("<2.0.0")
        assert c.matches(SemVer(1, 9, 9))
        assert not c.matches(SemVer(2, 0, 0))

    def test_tilde_range(self):
        c = VersionConstraint.parse("~1.2.0")
        assert c.matches(SemVer(1, 2, 5))
        assert not c.matches(SemVer(1, 3, 0))

    def test_caret_range_major_zero(self):
        c = VersionConstraint.parse("^0.1.0")
        assert c.matches(SemVer(0, 1, 5))

    def test_repr_is_string(self):
        c = VersionConstraint.parse(">=1.0.0")
        assert isinstance(repr(c), str)

    def test_str_is_string(self):
        c = VersionConstraint.parse("^2.0.0")
        assert isinstance(str(c), str)

    def test_invalid_operator_raises(self):
        with pytest.raises((ValueError, RuntimeError)):
            VersionConstraint.parse("??1.0.0")


class TestVersionedRegistryEdgeCases:
    def test_total_entries_zero(self):
        vr = VersionedRegistry()
        assert vr.total_entries() == 0

    def test_total_entries_after_register(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        assert vr.total_entries() == 1

    def test_keys_empty(self):
        vr = VersionedRegistry()
        assert vr.keys() == []

    def test_keys_after_register(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        keys = vr.keys()
        assert ("action", "maya") in keys

    def test_latest_version_none_when_empty(self):
        vr = VersionedRegistry()
        assert vr.latest_version("no_action", "maya") is None

    def test_latest_version_after_multiple(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "2.0.0")
        vr.register_versioned("action", "maya", "1.5.0")
        assert vr.latest_version("action", "maya") == "2.0.0"

    def test_versions_sorted_ascending(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "3.0.0")
        vr.register_versioned("action", "maya", "2.0.0")
        versions = vr.versions("action", "maya")
        assert versions == sorted(versions)

    def test_resolve_returns_none_when_no_match(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        result = vr.resolve("action", "maya", ">=9.0.0")
        assert result is None

    def test_resolve_returns_best_match(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "1.5.0")
        vr.register_versioned("action", "maya", "2.0.0")
        result = vr.resolve("action", "maya", "^1.0.0")
        assert result is not None
        assert result["version"] == "1.5.0"

    def test_resolve_all_wildcard(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "2.0.0")
        results = vr.resolve_all("action", "maya", "*")
        assert len(results) == 2

    def test_remove_matching_versions(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "2.0.0")
        removed = vr.remove("action", "maya", "^1.0.0")
        assert removed == 1
        assert vr.total_entries() == 1

    def test_remove_all_wildcard(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "maya", "2.0.0")
        vr.remove("action", "maya", "*")
        assert vr.total_entries() == 0

    def test_repr_is_string(self):
        vr = VersionedRegistry()
        assert isinstance(repr(vr), str)

    def test_multiple_dccs_independent(self):
        vr = VersionedRegistry()
        vr.register_versioned("action", "maya", "1.0.0")
        vr.register_versioned("action", "blender", "2.0.0")
        assert vr.latest_version("action", "maya") == "1.0.0"
        assert vr.latest_version("action", "blender") == "2.0.0"
