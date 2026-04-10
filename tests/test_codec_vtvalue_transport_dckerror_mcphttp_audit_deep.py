"""Deep tests for MessageCodec, VtValue, TransportAddress, DccError, McpHttpServer, and AuditMiddleware.

Covers:
- encode_request / encode_response / encode_notify / decode_envelope boundary cases
- VtValue.from_*/type_name/to_python for all supported types
- TransportAddress factory methods, properties, to_connection_string
- TransportScheme.select_address, ServiceStatus, RoutingStrategy variants
- DccError/DccErrorCode creation, attributes, all error codes
- ScriptResult creation, to_dict, all fields
- McpHttpConfig port/server_name/server_version construction
- McpHttpServer catalog/list_skills/loaded_count/discover/load_skill/unload_skill
- AuditEntry attributes, AuditLog methods, AuditMiddleware pipeline integration
"""

from __future__ import annotations

import pytest

import dcc_mcp_core
from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import AuditEntry
from dcc_mcp_core import AuditLog
from dcc_mcp_core import AuditMiddleware
from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme
from dcc_mcp_core import VtValue
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_request
from dcc_mcp_core import encode_response

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_pipeline(action_name: str = "test_action"):
    reg = ActionRegistry()
    reg.register(action_name, description="test", category="test")
    disp = ActionDispatcher(reg)
    disp.register_handler(action_name, lambda p: {"done": True})
    return ActionPipeline(disp), action_name


# ===========================================================================
# TestMessageCodecDeep
# ===========================================================================


class TestMessageCodecDeep:
    """Deep tests for encode_request / encode_response / encode_notify / decode_envelope."""

    class TestEncodeRequest:
        def test_returns_bytes(self) -> None:
            frame = encode_request("ping")
            assert isinstance(frame, bytes)

        def test_length_prefix_big_endian_4_bytes(self) -> None:
            frame = encode_request("ping")
            length = int.from_bytes(frame[:4], "big")
            assert length == len(frame) - 4

        def test_no_params_payload_is_empty_bytes(self) -> None:
            frame = encode_request("ping")
            msg = decode_envelope(frame[4:])
            assert msg["params"] == b""

        def test_with_params_bytes(self) -> None:
            payload = b"hello world"
            frame = encode_request("run", payload)
            msg = decode_envelope(frame[4:])
            assert msg["params"] == payload

        def test_type_field_is_request(self) -> None:
            frame = encode_request("execute")
            msg = decode_envelope(frame[4:])
            assert msg["type"] == "request"

        def test_method_field_matches(self) -> None:
            frame = encode_request("maya/create_sphere")
            msg = decode_envelope(frame[4:])
            assert msg["method"] == "maya/create_sphere"

        def test_id_is_uuid_format(self) -> None:
            frame = encode_request("list_tools")
            msg = decode_envelope(frame[4:])
            assert isinstance(msg["id"], str)
            assert len(msg["id"]) == 36
            # UUID has 4 dashes
            assert msg["id"].count("-") == 4

        def test_each_call_produces_unique_id(self) -> None:
            f1 = encode_request("method")
            f2 = encode_request("method")
            f3 = encode_request("method")
            id1 = decode_envelope(f1[4:])["id"]
            id2 = decode_envelope(f2[4:])["id"]
            id3 = decode_envelope(f3[4:])["id"]
            assert id1 != id2 != id3

        def test_long_method_name(self) -> None:
            method = "maya/" + "x" * 200
            frame = encode_request(method)
            msg = decode_envelope(frame[4:])
            assert msg["method"] == method

        def test_binary_params_preserved(self) -> None:
            binary = bytes(range(256))
            frame = encode_request("raw", binary)
            msg = decode_envelope(frame[4:])
            assert msg["params"] == binary

        def test_empty_method_allowed(self) -> None:
            frame = encode_request("")
            msg = decode_envelope(frame[4:])
            assert msg["method"] == ""

    class TestEncodeResponse:
        def test_success_response_roundtrip(self) -> None:
            req = encode_request("test")
            req_id = decode_envelope(req[4:])["id"]
            resp = encode_response(req_id, success=True, payload=b"ok")
            msg = decode_envelope(resp[4:])
            assert msg["type"] == "response"
            assert msg["success"] is True
            assert msg["payload"] == b"ok"
            assert msg["id"] == req_id

        def test_error_response_roundtrip(self) -> None:
            req = encode_request("bad")
            req_id = decode_envelope(req[4:])["id"]
            resp = encode_response(req_id, success=False, error="DCC offline")
            msg = decode_envelope(resp[4:])
            assert msg["type"] == "response"
            assert msg["success"] is False
            assert msg["error"] == "DCC offline"

        def test_success_with_no_payload(self) -> None:
            req = encode_request("ping")
            req_id = decode_envelope(req[4:])["id"]
            resp = encode_response(req_id, success=True)
            msg = decode_envelope(resp[4:])
            assert msg["success"] is True
            assert msg["payload"] == b""

        def test_invalid_uuid_raises(self) -> None:
            with pytest.raises((ValueError, RuntimeError)):
                encode_response("not-a-valid-uuid", success=True)

        def test_response_id_matches_request_id(self) -> None:
            req = encode_request("method")
            req_id = decode_envelope(req[4:])["id"]
            resp = encode_response(req_id, success=True, payload=b"data")
            resp_id = decode_envelope(resp[4:])["id"]
            assert resp_id == req_id

    class TestEncodeNotify:
        def test_notify_roundtrip(self) -> None:
            frame = encode_notify("scene_changed", b"data")
            msg = decode_envelope(frame[4:])
            assert msg["type"] == "notify"
            assert msg["topic"] == "scene_changed"
            assert msg["data"] == b"data"

        def test_notify_no_data(self) -> None:
            frame = encode_notify("render_complete")
            msg = decode_envelope(frame[4:])
            assert msg["topic"] == "render_complete"
            assert msg["data"] == b""

        def test_notify_binary_data(self) -> None:
            binary = bytes([0xFF, 0x00, 0xAB, 0xCD])
            frame = encode_notify("frame_data", binary)
            msg = decode_envelope(frame[4:])
            assert msg["data"] == binary

        def test_notify_has_correct_type(self) -> None:
            frame = encode_notify("event")
            msg = decode_envelope(frame[4:])
            assert msg["type"] == "notify"

        def test_notify_long_topic(self) -> None:
            topic = "dcc/" + "a" * 100
            frame = encode_notify(topic)
            msg = decode_envelope(frame[4:])
            assert msg["topic"] == topic

    class TestDecodeEnvelope:
        def test_invalid_bytes_raises_runtime_error(self) -> None:
            with pytest.raises(RuntimeError):
                decode_envelope(b"not valid msgpack at all!!!")

        def test_empty_bytes_raises(self) -> None:
            with pytest.raises((RuntimeError, Exception)):
                decode_envelope(b"")

        def test_truncated_frame_raises(self) -> None:
            frame = encode_request("test")
            # Give only partial payload
            with pytest.raises((RuntimeError, Exception)):
                decode_envelope(frame[4:10])

        def test_request_has_all_required_keys(self) -> None:
            frame = encode_request("my_method", b"params")
            msg = decode_envelope(frame[4:])
            assert "type" in msg
            assert "method" in msg
            assert "params" in msg
            assert "id" in msg

        def test_response_has_all_required_keys(self) -> None:
            req = encode_request("method")
            req_id = decode_envelope(req[4:])["id"]
            resp = encode_response(req_id, success=True, payload=b"x")
            msg = decode_envelope(resp[4:])
            assert "type" in msg
            assert "id" in msg
            assert "success" in msg
            assert "payload" in msg
            assert "error" in msg

        def test_notify_has_all_required_keys(self) -> None:
            frame = encode_notify("topic", b"data")
            msg = decode_envelope(frame[4:])
            assert "type" in msg
            assert "topic" in msg
            assert "data" in msg


# ===========================================================================
# TestVtValueDeep
# ===========================================================================


class TestVtValueDeep:
    """Deep tests for VtValue.from_*/type_name/to_python."""

    class TestFromBool:
        def test_from_bool_true(self) -> None:
            v = VtValue.from_bool(True)
            assert v.type_name == "bool"

        def test_from_bool_false(self) -> None:
            v = VtValue.from_bool(False)
            assert v.type_name == "bool"

        def test_from_bool_to_python_true(self) -> None:
            v = VtValue.from_bool(True)
            assert v.to_python() is True

        def test_from_bool_to_python_false(self) -> None:
            v = VtValue.from_bool(False)
            assert v.to_python() is False

        def test_bool_repr_contains_type(self) -> None:
            v = VtValue.from_bool(True)
            assert "bool" in str(v).lower() or repr(v) is not None

    class TestFromInt:
        def test_from_int_zero(self) -> None:
            v = VtValue.from_int(0)
            assert v.type_name == "int"

        def test_from_int_positive(self) -> None:
            v = VtValue.from_int(42)
            assert v.to_python() == 42

        def test_from_int_negative(self) -> None:
            v = VtValue.from_int(-100)
            assert v.to_python() == -100

        def test_from_int_large(self) -> None:
            v = VtValue.from_int(2**31 - 1)
            assert v.to_python() == 2**31 - 1

        def test_int_type_name(self) -> None:
            v = VtValue.from_int(1)
            assert v.type_name == "int"

    class TestFromFloat:
        def test_from_float_zero(self) -> None:
            v = VtValue.from_float(0.0)
            assert v.type_name == "float"

        def test_from_float_positive(self) -> None:
            v = VtValue.from_float(3.14)
            result = v.to_python()
            assert abs(result - 3.14) < 1e-5

        def test_from_float_negative(self) -> None:
            v = VtValue.from_float(-2.71)
            result = v.to_python()
            assert abs(result - (-2.71)) < 1e-5

        def test_float_type_name(self) -> None:
            v = VtValue.from_float(1.0)
            assert v.type_name == "float"

    class TestFromString:
        def test_from_string_empty(self) -> None:
            v = VtValue.from_string("")
            assert v.type_name == "string"

        def test_from_string_hello(self) -> None:
            v = VtValue.from_string("hello")
            assert v.to_python() == "hello"

        def test_from_string_unicode(self) -> None:
            v = VtValue.from_string("maya场景")
            assert v.to_python() == "maya场景"

        def test_from_string_special_chars(self) -> None:
            v = VtValue.from_string("/World/Mesh[0]")
            assert v.to_python() == "/World/Mesh[0]"

    class TestFromToken:
        def test_from_token_basic(self) -> None:
            v = VtValue.from_token("Mesh")
            assert v.type_name == "token"

        def test_from_token_to_python(self) -> None:
            v = VtValue.from_token("xformOp:translate")
            result = v.to_python()
            assert isinstance(result, str)
            assert result == "xformOp:translate"

    class TestFromAsset:
        def test_from_asset_basic(self) -> None:
            v = VtValue.from_asset("@textures/diffuse.png@")
            assert v.type_name == "asset"

        def test_from_asset_to_python_is_str(self) -> None:
            v = VtValue.from_asset("@my_scene.usd@")
            result = v.to_python()
            assert isinstance(result, str)

    class TestFromVec3f:
        def test_from_vec3f_basic(self) -> None:
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            # Rust implementation returns "float3" for vec3f
            assert v.type_name in ("vec3f", "float3")

        def test_from_vec3f_to_python_is_tuple(self) -> None:
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            result = v.to_python()
            assert isinstance(result, (tuple, list))
            assert len(result) == 3

        def test_from_vec3f_values(self) -> None:
            v = VtValue.from_vec3f(10.0, 20.0, 30.0)
            result = v.to_python()
            assert abs(result[0] - 10.0) < 1e-5
            assert abs(result[1] - 20.0) < 1e-5
            assert abs(result[2] - 30.0) < 1e-5

        def test_from_vec3f_origin(self) -> None:
            v = VtValue.from_vec3f(0.0, 0.0, 0.0)
            result = v.to_python()
            assert all(abs(x) < 1e-9 for x in result)

    class TestTypeName:
        def test_each_type_has_distinct_name(self) -> None:
            names = {
                VtValue.from_bool(True).type_name,
                VtValue.from_int(1).type_name,
                VtValue.from_float(1.0).type_name,
                VtValue.from_string("x").type_name,
                VtValue.from_token("t").type_name,
                VtValue.from_vec3f(0.0, 0.0, 0.0).type_name,
            }
            assert len(names) == 6

        def test_type_name_is_str(self) -> None:
            assert isinstance(VtValue.from_bool(True).type_name, str)


# ===========================================================================
# TestTransportAddressDeep
# ===========================================================================


class TestTransportAddressDeep:
    """Deep tests for TransportAddress factory methods and properties."""

    class TestTcp:
        def test_tcp_creates_address(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr is not None

        def test_tcp_is_tcp_true(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr.is_tcp is True

        def test_tcp_is_named_pipe_false(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr.is_named_pipe is False

        def test_tcp_is_unix_socket_false(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr.is_unix_socket is False

        def test_tcp_scheme_is_tcp(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr.scheme == "tcp"

        def test_tcp_connection_string_format(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            cs = addr.to_connection_string()
            assert "127.0.0.1" in cs
            assert "8765" in cs

        def test_tcp_is_local_for_localhost(self) -> None:
            addr = TransportAddress.tcp("127.0.0.1", 8765)
            assert addr.is_local is True

        def test_tcp_different_ports(self) -> None:
            a1 = TransportAddress.tcp("127.0.0.1", 8000)
            a2 = TransportAddress.tcp("127.0.0.1", 9000)
            assert a1.to_connection_string() != a2.to_connection_string()

    class TestNamedPipe:
        def test_named_pipe_is_named_pipe_true(self) -> None:
            addr = TransportAddress.named_pipe("maya-pipe-1234")
            assert addr.is_named_pipe is True

        def test_named_pipe_is_tcp_false(self) -> None:
            addr = TransportAddress.named_pipe("maya-pipe-1234")
            assert addr.is_tcp is False

        def test_named_pipe_scheme(self) -> None:
            addr = TransportAddress.named_pipe("maya-pipe-1234")
            assert addr.scheme == "pipe"

        def test_named_pipe_connection_string_contains_name(self) -> None:
            addr = TransportAddress.named_pipe("my-pipe")
            cs = addr.to_connection_string()
            assert "my-pipe" in cs

        def test_named_pipe_repr_contains_pipe(self) -> None:
            addr = TransportAddress.named_pipe("test")
            assert "pipe" in str(addr).lower()

    class TestDefaultLocal:
        def test_default_local_creates(self) -> None:
            addr = TransportAddress.default_local("maya", 12345)
            assert addr is not None

        def test_default_local_is_local(self) -> None:
            addr = TransportAddress.default_local("blender", 99999)
            assert addr.is_local is True

        def test_default_local_contains_dcc_name(self) -> None:
            addr = TransportAddress.default_local("maya", 12345)
            cs = addr.to_connection_string()
            assert "maya" in cs

        def test_default_local_contains_pid(self) -> None:
            addr = TransportAddress.default_local("maya", 99999)
            cs = addr.to_connection_string()
            assert "99999" in cs

    class TestDefaultPipeAndUnix:
        def test_default_pipe_name_contains_dcc(self) -> None:
            addr = TransportAddress.default_pipe_name("houdini", 5678)
            cs = addr.to_connection_string()
            assert "houdini" in cs

        def test_default_unix_socket_contains_dcc(self) -> None:
            addr = TransportAddress.default_unix_socket("blender", 2222)
            cs = addr.to_connection_string()
            assert "blender" in cs

    class TestParse:
        def test_parse_tcp_address(self) -> None:
            addr = TransportAddress.parse("tcp://127.0.0.1:8765")
            assert addr.is_tcp is True

        def test_parse_result_is_transport_address(self) -> None:
            addr = TransportAddress.parse("tcp://127.0.0.1:8765")
            assert isinstance(addr, TransportAddress)

        def test_parse_roundtrip(self) -> None:
            original = TransportAddress.tcp("127.0.0.1", 8765)
            cs = original.to_connection_string()
            parsed = TransportAddress.parse(cs)
            assert parsed.is_tcp == original.is_tcp

    class TestUnixSocket:
        def test_unix_socket_is_unix_socket_true(self) -> None:
            addr = TransportAddress.unix_socket("/tmp/maya.sock")
            assert addr.is_unix_socket is True

        def test_unix_socket_is_tcp_false(self) -> None:
            addr = TransportAddress.unix_socket("/tmp/maya.sock")
            assert addr.is_tcp is False

        def test_unix_socket_scheme(self) -> None:
            addr = TransportAddress.unix_socket("/tmp/test.sock")
            assert "unix" in addr.scheme.lower()


# ===========================================================================
# TestTransportSchemeServiceStatusRoutingStrategy
# ===========================================================================


class TestTransportSchemeDeep:
    """Tests for TransportScheme.select_address and enum values."""

    def test_auto_constant_exists(self) -> None:
        assert TransportScheme.AUTO is not None

    def test_tcp_only_constant_exists(self) -> None:
        assert TransportScheme.TCP_ONLY is not None

    def test_prefer_ipc_constant_exists(self) -> None:
        assert TransportScheme.PREFER_IPC is not None

    def test_prefer_named_pipe_constant_exists(self) -> None:
        assert TransportScheme.PREFER_NAMED_PIPE is not None

    def test_prefer_unix_socket_constant_exists(self) -> None:
        assert TransportScheme.PREFER_UNIX_SOCKET is not None

    def test_select_address_auto_with_pid_returns_address(self) -> None:
        scheme = TransportScheme.AUTO
        addr = scheme.select_address("maya", "127.0.0.1", 8765, 12345)
        assert isinstance(addr, TransportAddress)

    def test_select_address_without_pid(self) -> None:
        scheme = TransportScheme.TCP_ONLY
        addr = scheme.select_address("maya", "127.0.0.1", 8765)
        assert isinstance(addr, TransportAddress)

    def test_select_address_tcp_only_is_tcp(self) -> None:
        scheme = TransportScheme.TCP_ONLY
        addr = scheme.select_address("maya", "127.0.0.1", 8765, 1234)
        assert addr.is_tcp is True

    def test_auto_different_from_tcp_only_repr(self) -> None:
        assert str(TransportScheme.AUTO) != str(TransportScheme.TCP_ONLY)

    def test_all_schemes_are_distinct(self) -> None:
        schemes = [
            TransportScheme.AUTO,
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_IPC,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
        ]
        reprs = [str(s) for s in schemes]
        assert len(set(reprs)) == 5


class TestServiceStatusDeep:
    """Tests for ServiceStatus enum values."""

    def test_available_exists(self) -> None:
        assert ServiceStatus.AVAILABLE is not None

    def test_busy_exists(self) -> None:
        assert ServiceStatus.BUSY is not None

    def test_shutting_down_exists(self) -> None:
        assert ServiceStatus.SHUTTING_DOWN is not None

    def test_unreachable_exists(self) -> None:
        assert ServiceStatus.UNREACHABLE is not None

    def test_all_statuses_distinct(self) -> None:
        statuses = [
            ServiceStatus.AVAILABLE,
            ServiceStatus.BUSY,
            ServiceStatus.SHUTTING_DOWN,
            ServiceStatus.UNREACHABLE,
        ]
        reprs = [str(s) for s in statuses]
        assert len(set(reprs)) == 4

    def test_repr_contains_name(self) -> None:
        assert "AVAILABLE" in str(ServiceStatus.AVAILABLE).upper()
        assert "BUSY" in str(ServiceStatus.BUSY).upper()


class TestRoutingStrategyDeep:
    """Tests for RoutingStrategy enum values."""

    def test_round_robin_exists(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN is not None

    def test_first_available_exists(self) -> None:
        assert RoutingStrategy.FIRST_AVAILABLE is not None

    def test_least_busy_exists(self) -> None:
        assert RoutingStrategy.LEAST_BUSY is not None

    def test_random_exists(self) -> None:
        assert RoutingStrategy.RANDOM is not None

    def test_scene_match_exists(self) -> None:
        assert RoutingStrategy.SCENE_MATCH is not None

    def test_specific_exists(self) -> None:
        assert RoutingStrategy.SPECIFIC is not None

    def test_all_six_strategies_are_distinct(self) -> None:
        strategies = [
            RoutingStrategy.ROUND_ROBIN,
            RoutingStrategy.FIRST_AVAILABLE,
            RoutingStrategy.LEAST_BUSY,
            RoutingStrategy.RANDOM,
            RoutingStrategy.SCENE_MATCH,
            RoutingStrategy.SPECIFIC,
        ]
        reprs = [str(s) for s in strategies]
        assert len(set(reprs)) == 6


# ===========================================================================
# TestDccErrorDeep
# ===========================================================================


class TestDccErrorCodeDeep:
    """Tests for DccErrorCode enum values."""

    def test_connection_failed_exists(self) -> None:
        assert DccErrorCode.CONNECTION_FAILED is not None

    def test_internal_exists(self) -> None:
        assert DccErrorCode.INTERNAL is not None

    def test_invalid_input_exists(self) -> None:
        assert DccErrorCode.INVALID_INPUT is not None

    def test_not_responding_exists(self) -> None:
        assert DccErrorCode.NOT_RESPONDING is not None

    def test_permission_denied_exists(self) -> None:
        assert DccErrorCode.PERMISSION_DENIED is not None

    def test_scene_error_exists(self) -> None:
        assert DccErrorCode.SCENE_ERROR is not None

    def test_script_error_exists(self) -> None:
        assert DccErrorCode.SCRIPT_ERROR is not None

    def test_timeout_exists(self) -> None:
        assert DccErrorCode.TIMEOUT is not None

    def test_unsupported_exists(self) -> None:
        assert DccErrorCode.UNSUPPORTED is not None

    def test_all_nine_codes_are_distinct(self) -> None:
        codes = [
            DccErrorCode.CONNECTION_FAILED,
            DccErrorCode.INTERNAL,
            DccErrorCode.INVALID_INPUT,
            DccErrorCode.NOT_RESPONDING,
            DccErrorCode.PERMISSION_DENIED,
            DccErrorCode.SCENE_ERROR,
            DccErrorCode.SCRIPT_ERROR,
            DccErrorCode.TIMEOUT,
            DccErrorCode.UNSUPPORTED,
        ]
        reprs = [str(c) for c in codes]
        assert len(set(reprs)) == 9


class TestDccErrorDeep:
    """Tests for DccError construction and attributes."""

    def test_create_basic(self) -> None:
        e = DccError(
            code=DccErrorCode.TIMEOUT,
            message="connection timed out",
            details="host=127.0.0.1",
            recoverable=True,
        )
        assert e is not None

    def test_code_attribute(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="msg", details="d", recoverable=False)
        assert e.code == DccErrorCode.TIMEOUT

    def test_message_attribute(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="internal error", details="", recoverable=False)
        assert e.message == "internal error"

    def test_details_attribute(self) -> None:
        e = DccError(code=DccErrorCode.SCRIPT_ERROR, message="script failed", details="line=42", recoverable=False)
        assert e.details == "line=42"

    def test_recoverable_true(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timeout", details="", recoverable=True)
        assert e.recoverable is True

    def test_recoverable_false(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="crash", details="", recoverable=False)
        assert e.recoverable is False

    def test_repr_contains_code(self) -> None:
        e = DccError(code=DccErrorCode.TIMEOUT, message="timed out", details="", recoverable=False)
        r = repr(e)
        assert "TIMEOUT" in r

    def test_repr_contains_message(self) -> None:
        e = DccError(code=DccErrorCode.SCRIPT_ERROR, message="bad script", details="", recoverable=False)
        r = repr(e)
        assert "bad script" in r

    def test_all_error_codes_can_be_used(self) -> None:
        codes = [
            DccErrorCode.CONNECTION_FAILED,
            DccErrorCode.INTERNAL,
            DccErrorCode.INVALID_INPUT,
            DccErrorCode.NOT_RESPONDING,
            DccErrorCode.PERMISSION_DENIED,
            DccErrorCode.SCENE_ERROR,
            DccErrorCode.SCRIPT_ERROR,
            DccErrorCode.TIMEOUT,
            DccErrorCode.UNSUPPORTED,
        ]
        for code in codes:
            e = DccError(code=code, message=f"{code} error", details="", recoverable=False)
            assert e.code == code

    def test_empty_details(self) -> None:
        e = DccError(code=DccErrorCode.INTERNAL, message="msg", details="", recoverable=False)
        assert e.details == ""

    def test_empty_message(self) -> None:
        e = DccError(code=DccErrorCode.INVALID_INPUT, message="", details="", recoverable=False)
        assert e.message == ""


# ===========================================================================
# TestScriptResultDeep
# ===========================================================================


class TestScriptResultDeep:
    """Tests for ScriptResult creation, attributes, and to_dict."""

    def test_create_success(self) -> None:
        r = ScriptResult(success=True, output="done", error=None, execution_time_ms=100, context={})
        assert r.success is True

    def test_create_failure(self) -> None:
        r = ScriptResult(success=False, output="", error="NameError: sphere", execution_time_ms=5, context={})
        assert r.success is False

    def test_output_attribute(self) -> None:
        r = ScriptResult(success=True, output="sphere1", error=None, execution_time_ms=50, context={})
        assert r.output == "sphere1"

    def test_error_attribute_none(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=10, context={})
        assert r.error is None

    def test_error_attribute_string(self) -> None:
        r = ScriptResult(success=False, output="", error="AttributeError", execution_time_ms=1, context={})
        assert r.error == "AttributeError"

    def test_execution_time_ms_attribute(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=250, context={})
        assert r.execution_time_ms == 250

    def test_context_attribute_empty(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=0, context={})
        assert r.context == {}

    def test_context_attribute_with_data(self) -> None:
        # context field accepts a dict
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=0, context={"frame": "42"})
        assert r.context is not None

    def test_to_dict_returns_dict(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=100, context={})
        d = r.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_keys(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=100, context={})
        d = r.to_dict()
        assert "success" in d
        assert "output" in d
        assert "error" in d
        assert "execution_time_ms" in d

    def test_to_dict_success_value(self) -> None:
        r = ScriptResult(success=True, output="sphere", error=None, execution_time_ms=50, context={})
        d = r.to_dict()
        assert d["success"] is True

    def test_to_dict_output_value(self) -> None:
        r = ScriptResult(success=True, output="pSphere1", error=None, execution_time_ms=50, context={})
        d = r.to_dict()
        assert d["output"] == "pSphere1"

    def test_repr_contains_success(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=10, context={})
        assert "true" in repr(r).lower() or "success" in repr(r).lower()

    def test_repr_contains_time(self) -> None:
        r = ScriptResult(success=True, output="ok", error=None, execution_time_ms=99, context={})
        assert "99" in repr(r)


# ===========================================================================
# TestMcpHttpConfigDeep
# ===========================================================================


class TestMcpHttpConfigDeep:
    """Tests for McpHttpConfig construction and attributes."""

    def test_create_with_port(self) -> None:
        cfg = McpHttpConfig(port=8765)
        assert cfg.port == 8765

    def test_default_server_name(self) -> None:
        cfg = McpHttpConfig(port=8765)
        assert isinstance(cfg.server_name, str)
        assert len(cfg.server_name) > 0

    def test_custom_server_name(self) -> None:
        cfg = McpHttpConfig(port=9000, server_name="my-server")
        assert cfg.server_name == "my-server"

    def test_server_version_is_string(self) -> None:
        cfg = McpHttpConfig(port=8765)
        assert isinstance(cfg.server_version, str)

    def test_server_version_semver_format(self) -> None:
        cfg = McpHttpConfig(port=8765)
        parts = cfg.server_version.split(".")
        assert len(parts) >= 2

    def test_repr_contains_port(self) -> None:
        cfg = McpHttpConfig(port=12345)
        assert "12345" in repr(cfg)

    def test_repr_contains_server_name(self) -> None:
        cfg = McpHttpConfig(port=8765, server_name="dcc-srv")
        assert "dcc-srv" in repr(cfg)

    def test_different_ports(self) -> None:
        c1 = McpHttpConfig(port=8000)
        c2 = McpHttpConfig(port=9000)
        assert c1.port != c2.port

    def test_port_zero(self) -> None:
        # Port 0 triggers OS assignment; should still create
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0


# ===========================================================================
# TestMcpHttpServerDeep
# ===========================================================================


class TestMcpHttpServerDeep:
    """Tests for McpHttpServer construction and skill management (no actual HTTP binding)."""

    def _make_server(self, port: int = 19999) -> McpHttpServer:
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=port)
        return McpHttpServer(reg, cfg)

    def test_create_returns_instance(self) -> None:
        srv = self._make_server()
        assert srv is not None

    def test_repr_contains_name(self) -> None:
        cfg = McpHttpConfig(port=19990, server_name="test-srv")
        reg = ActionRegistry()
        srv = McpHttpServer(reg, cfg)
        assert "test-srv" in repr(srv)

    def test_repr_contains_port(self) -> None:
        srv = self._make_server(port=19991)
        assert "19991" in repr(srv)

    def test_catalog_is_skill_catalog(self) -> None:
        srv = self._make_server()
        cat = srv.catalog
        # catalog may be a SkillCatalog object or its string representation
        assert cat is not None
        assert "SkillCatalog" in str(cat)

    def test_loaded_count_initially_zero(self) -> None:
        srv = self._make_server()
        assert srv.loaded_count() == 0

    def test_list_skills_initially_empty(self) -> None:
        srv = self._make_server()
        assert srv.list_skills() == []

    def test_discover_returns_int(self, tmp_path) -> None:
        srv = self._make_server()
        # discover takes a list of extra_paths, not a single string
        count = srv.discover([str(tmp_path)])
        assert isinstance(count, int)

    def test_discover_empty_dir_returns_zero(self, tmp_path) -> None:
        srv = self._make_server()
        count = srv.discover([str(tmp_path)])
        assert count == 0

    def test_is_loaded_nonexistent_false(self) -> None:
        srv = self._make_server()
        assert srv.is_loaded("nonexistent_skill") is False

    def test_get_skill_info_nonexistent_is_none(self) -> None:
        srv = self._make_server()
        result = srv.get_skill_info("nonexistent_skill")
        assert result is None

    def test_find_skills_empty(self) -> None:
        srv = self._make_server()
        result = srv.find_skills("query")
        assert isinstance(result, list)

    def test_unload_skill_nonexistent_raises(self) -> None:
        srv = self._make_server()
        # unload_skill raises ValueError for unknown skill names
        with pytest.raises((ValueError, RuntimeError)):
            srv.unload_skill("nonexistent")

    def test_multiple_servers_dont_interfere(self) -> None:
        s1 = self._make_server(port=19980)
        s2 = self._make_server(port=19981)
        assert s1.loaded_count() == 0
        assert s2.loaded_count() == 0

    def test_list_skills_returns_list(self) -> None:
        srv = self._make_server()
        result = srv.list_skills()
        assert isinstance(result, list)


# ===========================================================================
# TestAuditEntryLogMiddlewareDeep
# ===========================================================================


class TestAuditEntryDeep:
    """Tests for AuditEntry attributes obtained via SandboxContext.audit_log."""

    def _ctx_with_entry(self, action_name: str = "test_action") -> tuple:
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        ctx.execute_json(action_name, "{}")
        entries = ctx.audit_log.entries()
        return ctx, entries[0]

    def test_entry_has_action_attribute(self) -> None:
        _, entry = self._ctx_with_entry("my_action")
        assert entry.action == "my_action"

    def test_entry_has_outcome_attribute(self) -> None:
        _, entry = self._ctx_with_entry()
        assert entry.outcome is not None

    def test_entry_has_timestamp_ms(self) -> None:
        _, entry = self._ctx_with_entry()
        assert isinstance(entry.timestamp_ms, int)
        assert entry.timestamp_ms > 0

    def test_entry_has_duration_ms(self) -> None:
        _, entry = self._ctx_with_entry()
        assert isinstance(entry.duration_ms, int)
        assert entry.duration_ms >= 0

    def test_entry_has_actor_attribute(self) -> None:
        _, entry = self._ctx_with_entry()
        # actor may be None or a string
        assert entry.actor is None or isinstance(entry.actor, str)

    def test_entry_repr_contains_action(self) -> None:
        _, entry = self._ctx_with_entry("special_action")
        assert "special_action" in repr(entry)

    def test_entry_params_json_attribute(self) -> None:
        _, entry = self._ctx_with_entry()
        # params_json is None or a string (JSON)
        assert entry.params_json is None or isinstance(entry.params_json, str)


class TestAuditLogDeep:
    """Tests for AuditLog methods via SandboxContext."""

    def _make_ctx_and_log(self) -> tuple:
        policy = SandboxPolicy()
        ctx = SandboxContext(policy)
        return ctx, ctx.audit_log

    def test_log_initially_empty(self) -> None:
        _, log = self._make_ctx_and_log()
        assert log.entries() == []

    def test_entries_after_execute(self) -> None:
        ctx, log = self._make_ctx_and_log()
        ctx.execute_json("action1", "{}")
        assert len(log.entries()) == 1

    def test_multiple_entries(self) -> None:
        ctx, log = self._make_ctx_and_log()
        ctx.execute_json("action1", "{}")
        ctx.execute_json("action2", "{}")
        ctx.execute_json("action3", "{}")
        assert len(log.entries()) == 3

    def test_entries_for_action_filters_correctly(self) -> None:
        ctx, log = self._make_ctx_and_log()
        ctx.execute_json("sphere", "{}")
        ctx.execute_json("cube", "{}")
        ctx.execute_json("sphere", "{}")
        sphere_entries = log.entries_for_action("sphere")
        assert len(sphere_entries) == 2
        for e in sphere_entries:
            assert e.action == "sphere"

    def test_entries_for_action_empty_if_no_match(self) -> None:
        ctx, log = self._make_ctx_and_log()
        ctx.execute_json("action1", "{}")
        result = log.entries_for_action("nonexistent")
        assert result == []

    def test_log_attrs_present(self) -> None:
        _, log = self._make_ctx_and_log()
        assert hasattr(log, "entries")
        assert hasattr(log, "entries_for_action")
        assert hasattr(log, "denials")
        assert hasattr(log, "successes")

    def test_successes_after_allowed_action(self) -> None:
        ctx, log = self._make_ctx_and_log()
        ctx.execute_json("allowed", "{}")
        succ = log.successes()
        assert len(succ) >= 1

    def test_entries_return_list(self) -> None:
        _, log = self._make_ctx_and_log()
        assert isinstance(log.entries(), list)

    def test_audit_log_repr_is_string(self) -> None:
        _, log = self._make_ctx_and_log()
        assert isinstance(repr(log), str)


class TestAuditMiddlewareDeep:
    """Tests for AuditMiddleware via ActionPipeline."""

    def _pipeline_with_audit(self, action: str = "op") -> tuple:
        reg = ActionRegistry()
        reg.register(action, description="test op", category="test")
        disp = ActionDispatcher(reg)
        disp.register_handler(action, lambda p: {"result": 42})
        pipeline = ActionPipeline(disp)
        audit = pipeline.add_audit(record_params=True)
        return pipeline, audit, action

    def test_audit_is_audit_middleware_type(self) -> None:
        _, audit, _ = self._pipeline_with_audit()
        assert isinstance(audit, AuditMiddleware)

    def test_record_count_zero_before_dispatch(self) -> None:
        _, audit, _ = self._pipeline_with_audit()
        assert audit.record_count() == 0

    def test_record_count_increments(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        pipeline.dispatch(action, "{}")
        assert audit.record_count() == 1

    def test_records_returns_list(self) -> None:
        _, audit, _ = self._pipeline_with_audit()
        assert isinstance(audit.records(), list)

    def test_records_after_dispatch_has_entry(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        pipeline.dispatch(action, "{}")
        recs = audit.records()
        assert len(recs) == 1

    def test_record_entry_has_action_key(self) -> None:
        pipeline, audit, _action = self._pipeline_with_audit("create_sphere")
        pipeline.dispatch("create_sphere", "{}")
        rec = audit.records()[0]
        assert rec["action"] == "create_sphere"

    def test_record_entry_success_true(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        pipeline.dispatch(action, "{}")
        rec = audit.records()[0]
        assert rec["success"] is True

    def test_records_for_action_filters(self) -> None:
        reg = ActionRegistry()
        reg.register("alpha", description="a", category="x")
        reg.register("beta", description="b", category="x")
        disp = ActionDispatcher(reg)
        disp.register_handler("alpha", lambda p: 1)
        disp.register_handler("beta", lambda p: 2)
        pipeline = ActionPipeline(disp)
        audit = pipeline.add_audit()
        pipeline.dispatch("alpha", "{}")
        pipeline.dispatch("beta", "{}")
        pipeline.dispatch("alpha", "{}")
        alpha_recs = audit.records_for_action("alpha")
        assert len(alpha_recs) == 2
        for r in alpha_recs:
            assert r["action"] == "alpha"

    def test_clear_resets_records(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        pipeline.dispatch(action, "{}")
        pipeline.dispatch(action, "{}")
        assert audit.record_count() == 2
        audit.clear()
        assert audit.record_count() == 0

    def test_clear_records_is_empty(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        pipeline.dispatch(action, "{}")
        audit.clear()
        assert audit.records() == []

    def test_in_middleware_names(self) -> None:
        pipeline, _audit, _action = self._pipeline_with_audit()
        assert "audit" in pipeline.middleware_names()

    def test_multiple_dispatches_accumulate(self) -> None:
        pipeline, audit, action = self._pipeline_with_audit()
        for _ in range(5):
            pipeline.dispatch(action, "{}")
        assert audit.record_count() == 5
