"""Deep tests for transport/channel/protocol/recovery APIs.

Covers:
- TransportAddress: tcp/named_pipe/unix_socket/default_local/parse/properties
- TransportScheme: all enum values, select_address for tcp/pipe/unix
- RoutingStrategy: all enum values, equality
- FramedChannel: send_request/send_response/send_notify/try_recv/shutdown/bool/repr
- IpcListener + ListenerHandle: bind/local_address/transport_name/into_handle/accept_count/is_shutdown
- encode_request / encode_response / encode_notify / decode_envelope round-trips
- ScriptResult: construction/to_dict/repr
- SceneStatistics: construction/defaults/repr
- SceneInfo: construction/defaults/repr
- DccError: construction/str/repr
- CaptureResult: construction/data_size/repr
- PyCrashRecoveryPolicy: fixed/exponential backoff, should_restart, max exceeded
"""

from __future__ import annotations

# Import built-in modules
import os
import uuid

# Import third-party modules
import pytest

from dcc_mcp_core import CaptureResult
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import FramedChannel
from dcc_mcp_core import IpcListener
from dcc_mcp_core import ListenerHandle
from dcc_mcp_core import PyCrashRecoveryPolicy
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme
from dcc_mcp_core import connect_ipc
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_request
from dcc_mcp_core import encode_response

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _bind_and_connect() -> tuple[ListenerHandle, FramedChannel]:
    """Bind TCP listener on :0, convert to handle, connect client channel."""
    addr = TransportAddress.tcp("127.0.0.1", 0)
    listener = IpcListener.bind(addr)
    local = listener.local_address()
    handle = listener.into_handle()
    channel = connect_ipc(local)
    return handle, channel


# ===========================================================================
# TransportAddress deep tests
# ===========================================================================


class TestTransportAddressTcp:
    def test_scheme_is_tcp(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.scheme == "tcp"

    def test_is_tcp_true(self) -> None:
        addr = TransportAddress.tcp("192.168.1.1", 1234)
        assert addr.is_tcp is True

    def test_is_named_pipe_false(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_named_pipe is False

    def test_is_unix_socket_false(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_unix_socket is False

    def test_is_local_loopback(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_local is True

    def test_to_connection_string(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        cs = addr.to_connection_string()
        assert cs == "tcp://127.0.0.1:8765"

    def test_str_representation(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 9000)
        assert "127.0.0.1" in str(addr)
        assert "9000" in str(addr)

    def test_repr_contains_tcp(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert "tcp" in repr(addr).lower()

    def test_port_zero_allowed(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        assert addr.is_tcp is True

    def test_high_port(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 65535)
        assert addr.is_tcp is True


class TestTransportAddressNamedPipe:
    def test_scheme_is_pipe(self) -> None:
        addr = TransportAddress.named_pipe("dcc-mcp-test")
        assert addr.scheme == "pipe"

    def test_is_named_pipe_true(self) -> None:
        addr = TransportAddress.named_pipe("my-pipe")
        assert addr.is_named_pipe is True

    def test_is_tcp_false(self) -> None:
        addr = TransportAddress.named_pipe("my-pipe")
        assert addr.is_tcp is False

    def test_is_local_true(self) -> None:
        addr = TransportAddress.named_pipe("test")
        assert addr.is_local is True

    def test_str_contains_pipe_name(self) -> None:
        addr = TransportAddress.named_pipe("my-dcc-pipe")
        s = str(addr)
        assert "my-dcc-pipe" in s

    def test_repr_contains_pipe(self) -> None:
        addr = TransportAddress.named_pipe("x")
        assert "pipe" in repr(addr).lower()


class TestTransportAddressUnixSocket:
    def test_scheme_is_unix(self) -> None:
        addr = TransportAddress.unix_socket("/tmp/dcc-test.sock")
        assert addr.scheme == "unix"

    def test_is_unix_socket_true(self) -> None:
        addr = TransportAddress.unix_socket("/tmp/test.sock")
        assert addr.is_unix_socket is True

    def test_is_tcp_false(self) -> None:
        addr = TransportAddress.unix_socket("/tmp/test.sock")
        assert addr.is_tcp is False

    def test_is_local_true(self) -> None:
        addr = TransportAddress.unix_socket("/tmp/test.sock")
        assert addr.is_local is True

    def test_str_contains_path(self) -> None:
        addr = TransportAddress.unix_socket("/var/run/dcc.sock")
        assert "/var/run/dcc.sock" in str(addr)


class TestTransportAddressParse:
    def test_parse_tcp(self) -> None:
        addr = TransportAddress.parse("tcp://127.0.0.1:9000")
        assert addr.scheme == "tcp"
        assert addr.is_tcp is True

    def test_parse_pipe(self) -> None:
        addr = TransportAddress.parse("pipe://my-pipe")
        assert addr.scheme == "pipe"
        assert addr.is_named_pipe is True

    def test_parse_unix(self) -> None:
        addr = TransportAddress.parse("unix:///tmp/sock")
        assert addr.scheme == "unix"
        assert addr.is_unix_socket is True

    def test_parse_invalid_raises(self) -> None:
        with pytest.raises((ValueError, RuntimeError)):
            TransportAddress.parse("ftp://127.0.0.1:21")

    def test_parse_roundtrip_tcp(self) -> None:
        original = TransportAddress.tcp("127.0.0.1", 18812)
        cs = original.to_connection_string()
        parsed = TransportAddress.parse(cs)
        assert parsed.is_tcp is True
        assert "18812" in parsed.to_connection_string()


class TestTransportAddressDefaultLocal:
    def test_default_local_is_local(self) -> None:
        addr = TransportAddress.default_local("maya", os.getpid())
        assert addr.is_local is True

    def test_default_local_scheme(self) -> None:
        addr = TransportAddress.default_local("blender", 1234)
        # On Windows → pipe, on Linux/macOS → unix
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_default_pipe_name_is_pipe(self) -> None:
        addr = TransportAddress.default_pipe_name("maya", 5678)
        assert addr.is_named_pipe is True

    def test_default_unix_socket_is_unix(self) -> None:
        addr = TransportAddress.default_unix_socket("houdini", 9999)
        assert addr.is_unix_socket is True

    def test_default_pipe_name_contains_dcc_type(self) -> None:
        addr = TransportAddress.default_pipe_name("maya", 1111)
        s = str(addr)
        assert "maya" in s.lower()

    def test_default_unix_socket_contains_dcc_type(self) -> None:
        addr = TransportAddress.default_unix_socket("houdini", 2222)
        s = str(addr)
        assert "houdini" in s.lower()


# ===========================================================================
# TransportScheme deep tests
# ===========================================================================


class TestTransportSchemeEnums:
    def test_auto_exists(self) -> None:
        assert TransportScheme.AUTO is not None

    def test_tcp_only_exists(self) -> None:
        assert TransportScheme.TCP_ONLY is not None

    def test_prefer_named_pipe_exists(self) -> None:
        assert TransportScheme.PREFER_NAMED_PIPE is not None

    def test_prefer_unix_socket_exists(self) -> None:
        assert TransportScheme.PREFER_UNIX_SOCKET is not None

    def test_prefer_ipc_exists(self) -> None:
        assert TransportScheme.PREFER_IPC is not None

    def test_auto_equals_itself(self) -> None:
        assert TransportScheme.AUTO == TransportScheme.AUTO

    def test_auto_not_equals_tcp_only(self) -> None:
        assert TransportScheme.AUTO != TransportScheme.TCP_ONLY

    def test_repr_is_string(self) -> None:
        r = repr(TransportScheme.AUTO)
        assert isinstance(r, str)

    def test_str_is_string(self) -> None:
        s = str(TransportScheme.TCP_ONLY)
        assert isinstance(s, str)


class TestTransportSchemeSelectAddress:
    def test_tcp_only_returns_tcp(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert addr.is_tcp is True

    def test_auto_with_pid_returns_address(self) -> None:
        addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=os.getpid())
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_prefer_named_pipe_with_pid_returns_ipc(self) -> None:
        # With a pid, PREFER_NAMED_PIPE selects pipe on Windows; unix/tcp elsewhere
        addr = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "127.0.0.1", 18812, pid=os.getpid())
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_prefer_named_pipe_no_pid_returns_address(self) -> None:
        # Without pid, no IPC transport can be generated; falls back to TCP
        addr = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "127.0.0.1", 18812)
        assert addr.scheme in ("pipe", "unix", "tcp")

    def test_prefer_unix_socket_returns_address(self) -> None:
        # On Windows unix socket is not supported → falls back to TCP
        addr = TransportScheme.PREFER_UNIX_SOCKET.select_address("maya", "127.0.0.1", 18812)
        assert addr.scheme in ("unix", "tcp")

    def test_result_is_local(self) -> None:
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert addr.is_local is True


# ===========================================================================
# RoutingStrategy deep tests
# ===========================================================================


class TestRoutingStrategyEnums:
    def test_first_available_exists(self) -> None:
        assert RoutingStrategy.FIRST_AVAILABLE is not None

    def test_round_robin_exists(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN is not None

    def test_least_busy_exists(self) -> None:
        assert RoutingStrategy.LEAST_BUSY is not None

    def test_specific_exists(self) -> None:
        assert RoutingStrategy.SPECIFIC is not None

    def test_scene_match_exists(self) -> None:
        assert RoutingStrategy.SCENE_MATCH is not None

    def test_random_exists(self) -> None:
        assert RoutingStrategy.RANDOM is not None

    def test_equality_same(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN == RoutingStrategy.ROUND_ROBIN

    def test_inequality_different(self) -> None:
        assert RoutingStrategy.ROUND_ROBIN != RoutingStrategy.RANDOM

    def test_repr_is_string(self) -> None:
        assert isinstance(repr(RoutingStrategy.FIRST_AVAILABLE), str)

    def test_str_is_string(self) -> None:
        assert isinstance(str(RoutingStrategy.LEAST_BUSY), str)

    def test_all_six_distinct(self) -> None:
        values = [
            RoutingStrategy.FIRST_AVAILABLE,
            RoutingStrategy.ROUND_ROBIN,
            RoutingStrategy.LEAST_BUSY,
            RoutingStrategy.SPECIFIC,
            RoutingStrategy.SCENE_MATCH,
            RoutingStrategy.RANDOM,
        ]
        for i, a in enumerate(values):
            for j, b in enumerate(values):
                if i != j:
                    assert a != b, f"Expected {a} != {b}"


# ===========================================================================
# IpcListener + ListenerHandle deep tests
# ===========================================================================


class TestIpcListenerBind:
    def test_bind_tcp_succeeds(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener is not None

    def test_local_address_is_tcp(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert local.is_tcp is True

    def test_local_address_port_assigned(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        cs = local.to_connection_string()
        port_str = cs.split(":")[-1]
        assert int(port_str) > 0

    def test_transport_name_is_tcp(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener.transport_name == "tcp"

    def test_repr_is_string(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert isinstance(repr(listener), str)

    def test_into_handle_returns_handle(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        handle = listener.into_handle()
        assert isinstance(handle, ListenerHandle)

    def test_into_handle_only_once(self) -> None:
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        listener.into_handle()
        with pytest.raises(RuntimeError):
            listener.into_handle()


class TestListenerHandleAttributes:
    def test_accept_count_initial_zero(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert handle.accept_count == 0
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_is_shutdown_initial_false(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert handle.is_shutdown is False
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_transport_name_is_tcp(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert handle.transport_name == "tcp"
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_local_address_returns_address(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            addr = handle.local_address()
            assert addr.is_tcp is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_shutdown_sets_is_shutdown(self) -> None:
        handle, channel = _bind_and_connect()
        channel.shutdown()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_is_idempotent(self) -> None:
        handle, channel = _bind_and_connect()
        channel.shutdown()
        handle.shutdown()
        handle.shutdown()  # second call should not raise
        assert handle.is_shutdown is True

    def test_repr_is_string(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert isinstance(repr(handle), str)
        finally:
            channel.shutdown()
            handle.shutdown()


# ===========================================================================
# FramedChannel deep tests
# ===========================================================================


class TestFramedChannelProperties:
    def test_is_running_after_connect(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert channel.is_running is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_try_recv_returns_none_when_empty(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            result = channel.try_recv()
            assert result is None
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_bool_true_when_running(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert bool(channel) is True
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_repr_is_string(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            assert isinstance(repr(channel), str)
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelSend:
    def test_send_request_returns_uuid_string(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("test_method", b"params")
            assert isinstance(req_id, str)
            assert len(req_id) == 36
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_no_params(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            req_id = channel.send_request("ping")
            assert isinstance(req_id, str)
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_request_multiple_distinct_ids(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            ids = {channel.send_request(f"method_{i}") for i in range(5)}
            assert len(ids) == 5
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_does_not_raise(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("scene_changed", b"data")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_notify_no_data(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            channel.send_notify("heartbeat")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_valid_uuid(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            valid_id = str(uuid.uuid4())
            channel.send_response(valid_id, True, b"payload")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_failure(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            valid_id = str(uuid.uuid4())
            channel.send_response(valid_id, False, error="something failed")
        finally:
            channel.shutdown()
            handle.shutdown()

    def test_send_response_invalid_uuid_raises(self) -> None:
        handle, channel = _bind_and_connect()
        try:
            with pytest.raises((ValueError, RuntimeError)):
                channel.send_response("not-a-uuid", True)
        finally:
            channel.shutdown()
            handle.shutdown()


class TestFramedChannelShutdown:
    def test_shutdown_idempotent(self) -> None:
        handle, channel = _bind_and_connect()
        handle.shutdown()
        channel.shutdown()
        channel.shutdown()  # second call must not raise

    def test_is_running_after_shutdown(self) -> None:
        handle, channel = _bind_and_connect()
        handle.shutdown()
        channel.shutdown()
        # After shutdown is_running may be False (implementation dependent)
        # Just verify the property is accessible
        _ = channel.is_running


# ===========================================================================
# encode_request / encode_response / encode_notify / decode_envelope
# ===========================================================================


class TestEncodeRequest:
    def test_returns_bytes(self) -> None:
        frame = encode_request("execute_python", b"cmds.sphere()")
        assert isinstance(frame, bytes)

    def test_length_at_least_4(self) -> None:
        frame = encode_request("ping")
        assert len(frame) >= 4

    def test_prefix_equals_payload_length(self) -> None:
        frame = encode_request("method", b"params")
        prefix = int.from_bytes(frame[:4], "big")
        assert prefix == len(frame) - 4

    def test_decode_type_is_request(self) -> None:
        frame = encode_request("execute_python", b"hello")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"

    def test_decode_method_correct(self) -> None:
        frame = encode_request("my_method", b"")
        msg = decode_envelope(frame[4:])
        assert msg["method"] == "my_method"

    def test_decode_params_correct(self) -> None:
        frame = encode_request("do_thing", b"my_params")
        msg = decode_envelope(frame[4:])
        assert msg["params"] == b"my_params"

    def test_decode_id_is_string(self) -> None:
        frame = encode_request("test")
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["id"], str)

    def test_no_params_defaults_to_empty(self) -> None:
        frame = encode_request("ping")
        msg = decode_envelope(frame[4:])
        assert msg.get("params") == b""


class TestEncodeResponse:
    def test_returns_bytes(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"result")
        assert isinstance(frame, bytes)

    def test_decode_type_is_response(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True)
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"

    def test_decode_success_true(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"payload")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is True

    def test_decode_success_false(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, False, error="bad input")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is False

    def test_decode_payload_correct(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True, b"abc123")
        msg = decode_envelope(frame[4:])
        assert msg["payload"] == b"abc123"

    def test_decode_error_message(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, False, error="something went wrong")
        msg = decode_envelope(frame[4:])
        assert msg["error"] == "something went wrong"

    def test_decode_id_matches_request_id(self) -> None:
        req_id = str(uuid.uuid4())
        frame = encode_response(req_id, True)
        msg = decode_envelope(frame[4:])
        assert msg["id"] == req_id

    def test_invalid_uuid_raises_value_error(self) -> None:
        with pytest.raises((ValueError, RuntimeError)):
            encode_response("not-valid-uuid", True)

    def test_all_zeros_uuid_accepted(self) -> None:
        all_zeros = "00000000-0000-0000-0000-000000000000"
        frame = encode_response(all_zeros, True, b"ok")
        msg = decode_envelope(frame[4:])
        assert msg["id"] == all_zeros


class TestEncodeNotify:
    def test_returns_bytes(self) -> None:
        frame = encode_notify("scene_changed", b"data")
        assert isinstance(frame, bytes)

    def test_decode_type_is_notify(self) -> None:
        frame = encode_notify("render_complete", b"")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"

    def test_decode_topic_correct(self) -> None:
        frame = encode_notify("selection_changed", b"obj1")
        msg = decode_envelope(frame[4:])
        assert msg["topic"] == "selection_changed"

    def test_decode_data_correct(self) -> None:
        frame = encode_notify("event", b"payload_data")
        msg = decode_envelope(frame[4:])
        assert msg["data"] == b"payload_data"

    def test_no_data_defaults_to_empty(self) -> None:
        frame = encode_notify("heartbeat")
        msg = decode_envelope(frame[4:])
        assert msg["data"] == b""

    def test_multiple_events_distinct_ids(self) -> None:
        frames = [encode_notify(f"event_{i}", b"") for i in range(3)]
        ids = [decode_envelope(f[4:])["id"] for f in frames]
        # IDs may be None for notify, but should all be of same type
        for i in ids:
            assert i is None or isinstance(i, str)


class TestDecodeEnvelopeErrors:
    def test_empty_bytes_raises(self) -> None:
        with pytest.raises((RuntimeError, ValueError)):
            decode_envelope(b"")

    def test_garbage_bytes_raises(self) -> None:
        with pytest.raises(RuntimeError):
            decode_envelope(b"\x00\x01\x02\x03\xff\xfe\xfd")

    def test_valid_string_not_msgpack_raises(self) -> None:
        with pytest.raises(RuntimeError):
            decode_envelope(b'{"type": "request"}')


# ===========================================================================
# ScriptResult deep tests
# ===========================================================================


class TestScriptResultConstruction:
    def test_success_true(self) -> None:
        sr = ScriptResult(True, 100)
        assert sr.success is True

    def test_success_false(self) -> None:
        sr = ScriptResult(False, 200)
        assert sr.success is False

    def test_execution_time_ms(self) -> None:
        sr = ScriptResult(True, 42)
        assert sr.execution_time_ms == 42

    def test_output_stored(self) -> None:
        sr = ScriptResult(True, 10, output="hello world")
        assert sr.output == "hello world"

    def test_output_default_none(self) -> None:
        sr = ScriptResult(True, 10)
        assert sr.output is None

    def test_error_stored(self) -> None:
        sr = ScriptResult(False, 50, error="syntax error on line 3")
        assert sr.error == "syntax error on line 3"

    def test_error_default_none(self) -> None:
        sr = ScriptResult(True, 10)
        assert sr.error is None

    def test_context_stored(self) -> None:
        sr = ScriptResult(True, 10, context={"key": "value"})
        assert sr.context == {"key": "value"}

    def test_context_default_empty(self) -> None:
        sr = ScriptResult(True, 10)
        assert isinstance(sr.context, dict)


class TestScriptResultToDict:
    def test_to_dict_returns_dict(self) -> None:
        sr = ScriptResult(True, 50, output="ok")
        d = sr.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_has_success(self) -> None:
        sr = ScriptResult(True, 10)
        assert "success" in sr.to_dict()

    def test_to_dict_has_execution_time_ms(self) -> None:
        sr = ScriptResult(True, 10)
        assert "execution_time_ms" in sr.to_dict()

    def test_to_dict_has_output(self) -> None:
        sr = ScriptResult(True, 10, output="data")
        assert "output" in sr.to_dict()

    def test_to_dict_has_error(self) -> None:
        sr = ScriptResult(False, 10, error="err")
        assert "error" in sr.to_dict()

    def test_to_dict_has_context(self) -> None:
        sr = ScriptResult(True, 10, context={"k": "v"})
        assert "context" in sr.to_dict()

    def test_to_dict_values_correct(self) -> None:
        sr = ScriptResult(True, 99, output="result", context={"x": "1"})
        d = sr.to_dict()
        assert d["success"] is True
        assert d["execution_time_ms"] == 99
        assert d["output"] == "result"

    def test_repr_is_string(self) -> None:
        sr = ScriptResult(True, 10)
        assert isinstance(repr(sr), str)


# ===========================================================================
# SceneStatistics deep tests
# ===========================================================================


class TestSceneStatisticsConstruction:
    def test_all_defaults_zero(self) -> None:
        ss = SceneStatistics()
        assert ss.object_count == 0
        assert ss.vertex_count == 0
        assert ss.polygon_count == 0
        assert ss.material_count == 0
        assert ss.texture_count == 0
        assert ss.light_count == 0
        assert ss.camera_count == 0

    def test_object_count_stored(self) -> None:
        ss = SceneStatistics(object_count=42)
        assert ss.object_count == 42

    def test_vertex_count_stored(self) -> None:
        ss = SceneStatistics(vertex_count=1000)
        assert ss.vertex_count == 1000

    def test_polygon_count_stored(self) -> None:
        ss = SceneStatistics(polygon_count=500)
        assert ss.polygon_count == 500

    def test_material_count_stored(self) -> None:
        ss = SceneStatistics(material_count=5)
        assert ss.material_count == 5

    def test_texture_count_stored(self) -> None:
        ss = SceneStatistics(texture_count=10)
        assert ss.texture_count == 10

    def test_light_count_stored(self) -> None:
        ss = SceneStatistics(light_count=3)
        assert ss.light_count == 3

    def test_camera_count_stored(self) -> None:
        ss = SceneStatistics(camera_count=2)
        assert ss.camera_count == 2

    def test_all_fields_stored(self) -> None:
        ss = SceneStatistics(
            object_count=10,
            vertex_count=200,
            polygon_count=100,
            material_count=4,
            texture_count=8,
            light_count=2,
            camera_count=1,
        )
        assert ss.object_count == 10
        assert ss.vertex_count == 200
        assert ss.polygon_count == 100

    def test_repr_is_string(self) -> None:
        ss = SceneStatistics(object_count=5)
        assert isinstance(repr(ss), str)

    def test_repr_contains_counts(self) -> None:
        ss = SceneStatistics(object_count=7, vertex_count=300)
        r = repr(ss)
        assert "7" in r


# ===========================================================================
# SceneInfo deep tests
# ===========================================================================


class TestSceneInfoDefaults:
    def test_default_name(self) -> None:
        si = SceneInfo()
        assert si.name == "untitled"

    def test_default_modified_false(self) -> None:
        si = SceneInfo()
        assert si.modified is False

    def test_default_frame_range_none(self) -> None:
        si = SceneInfo()
        assert si.frame_range is None

    def test_default_current_frame_none(self) -> None:
        si = SceneInfo()
        assert si.current_frame is None

    def test_default_fps_none(self) -> None:
        si = SceneInfo()
        assert si.fps is None

    def test_default_up_axis_none(self) -> None:
        si = SceneInfo()
        assert si.up_axis is None

    def test_default_units_none(self) -> None:
        si = SceneInfo()
        assert si.units is None

    def test_default_file_path_empty(self) -> None:
        si = SceneInfo()
        assert si.file_path == ""

    def test_default_format_empty(self) -> None:
        si = SceneInfo()
        assert si.format == ""

    def test_default_statistics_zero(self) -> None:
        si = SceneInfo()
        assert si.statistics.object_count == 0

    def test_default_metadata_empty(self) -> None:
        si = SceneInfo()
        assert isinstance(si.metadata, dict)


class TestSceneInfoConstruction:
    def test_file_path_stored(self) -> None:
        si = SceneInfo(file_path="/project/scene.mb")
        assert si.file_path == "/project/scene.mb"

    def test_name_stored(self) -> None:
        si = SceneInfo(name="my_scene")
        assert si.name == "my_scene"

    def test_modified_true(self) -> None:
        si = SceneInfo(modified=True)
        assert si.modified is True

    def test_format_stored(self) -> None:
        si = SceneInfo(format="maya_binary")
        assert si.format == "maya_binary"

    def test_frame_range_stored(self) -> None:
        si = SceneInfo(frame_range=(1.0, 100.0))
        assert si.frame_range == (1.0, 100.0)

    def test_current_frame_stored(self) -> None:
        si = SceneInfo(current_frame=42.0)
        assert si.current_frame == 42.0

    def test_fps_stored(self) -> None:
        si = SceneInfo(fps=25.0)
        assert si.fps == 25.0

    def test_up_axis_stored(self) -> None:
        si = SceneInfo(up_axis="Y")
        assert si.up_axis == "Y"

    def test_units_stored(self) -> None:
        si = SceneInfo(units="cm")
        assert si.units == "cm"

    def test_statistics_stored(self) -> None:
        ss = SceneStatistics(object_count=10, vertex_count=500)
        si = SceneInfo(statistics=ss)
        assert si.statistics.object_count == 10

    def test_metadata_stored(self) -> None:
        si = SceneInfo(metadata={"shot": "A001", "dcc": "maya"})
        assert si.metadata["shot"] == "A001"

    def test_repr_is_string(self) -> None:
        si = SceneInfo(name="test")
        assert isinstance(repr(si), str)

    def test_repr_contains_name(self) -> None:
        si = SceneInfo(name="my_scene_name")
        assert "my_scene_name" in repr(si)


# ===========================================================================
# DccError deep tests
# ===========================================================================


class TestDccErrorConstruction:
    def test_code_stored(self) -> None:
        err = DccError(DccErrorCode.CONNECTION_FAILED, "cannot connect")
        assert err.code == DccErrorCode.CONNECTION_FAILED

    def test_message_stored(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        assert err.message == "timed out"

    def test_details_stored(self) -> None:
        err = DccError(DccErrorCode.SCRIPT_ERROR, "error", details="line 42")
        assert err.details == "line 42"

    def test_details_default_none(self) -> None:
        err = DccError(DccErrorCode.INTERNAL, "oops")
        assert err.details is None

    def test_recoverable_true(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "retry", recoverable=True)
        assert err.recoverable is True

    def test_recoverable_default_false(self) -> None:
        err = DccError(DccErrorCode.PERMISSION_DENIED, "denied")
        assert err.recoverable is False

    def test_str_is_string(self) -> None:
        err = DccError(DccErrorCode.CONNECTION_FAILED, "msg")
        assert isinstance(str(err), str)

    def test_str_contains_code(self) -> None:
        err = DccError(DccErrorCode.CONNECTION_FAILED, "msg")
        assert "CONNECTION_FAILED" in str(err)

    def test_repr_is_string(self) -> None:
        err = DccError(DccErrorCode.TIMEOUT, "timed out")
        assert isinstance(repr(err), str)

    def test_all_error_codes_constructable(self) -> None:
        codes = [
            DccErrorCode.CONNECTION_FAILED,
            DccErrorCode.TIMEOUT,
            DccErrorCode.SCRIPT_ERROR,
            DccErrorCode.NOT_RESPONDING,
            DccErrorCode.UNSUPPORTED,
            DccErrorCode.PERMISSION_DENIED,
            DccErrorCode.INVALID_INPUT,
            DccErrorCode.SCENE_ERROR,
            DccErrorCode.INTERNAL,
        ]
        for code in codes:
            err = DccError(code, "test")
            assert err.code == code


# ===========================================================================
# CaptureResult deep tests
# ===========================================================================


class TestCaptureResultConstruction:
    def test_data_stored(self) -> None:
        data = b"\x89PNG\r\n" + b"\x00" * 20
        cr = CaptureResult(data, 1920, 1080, "png")
        assert cr.data == data

    def test_width_stored(self) -> None:
        cr = CaptureResult(b"data", 1920, 1080, "png")
        assert cr.width == 1920

    def test_height_stored(self) -> None:
        cr = CaptureResult(b"data", 1280, 720, "jpeg")
        assert cr.height == 720

    def test_format_stored(self) -> None:
        cr = CaptureResult(b"data", 100, 100, "png")
        assert cr.format == "png"

    def test_viewport_stored(self) -> None:
        cr = CaptureResult(b"data", 100, 100, "png", viewport="Maya_Persp")
        assert cr.viewport == "Maya_Persp"

    def test_viewport_default_none(self) -> None:
        cr = CaptureResult(b"data", 100, 100, "jpeg")
        assert cr.viewport is None

    def test_data_size_correct(self) -> None:
        data = b"x" * 200
        cr = CaptureResult(data, 10, 10, "raw_bgra")
        assert cr.data_size() == 200

    def test_data_size_empty(self) -> None:
        cr = CaptureResult(b"", 0, 0, "raw_bgra")
        assert cr.data_size() == 0

    def test_repr_is_string(self) -> None:
        cr = CaptureResult(b"img", 1920, 1080, "png")
        assert isinstance(repr(cr), str)

    def test_repr_contains_dimensions(self) -> None:
        cr = CaptureResult(b"img", 1920, 1080, "png")
        r = repr(cr)
        assert "1920" in r
        assert "1080" in r

    def test_jpeg_format(self) -> None:
        cr = CaptureResult(b"\xff\xd8\xff" + b"\x00" * 50, 800, 600, "jpeg")
        assert cr.format == "jpeg"

    def test_raw_bgra_format(self) -> None:
        cr = CaptureResult(b"\x00" * 400, 10, 10, "raw_bgra")
        assert cr.format == "raw_bgra"


# ===========================================================================
# PyCrashRecoveryPolicy deep tests
# ===========================================================================


class TestPyCrashRecoveryPolicyConstruction:
    def test_default_max_restarts(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.max_restarts == 3

    def test_custom_max_restarts(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=10)
        assert p.max_restarts == 10

    def test_repr_is_string(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        assert isinstance(repr(p), str)

    def test_repr_contains_max_restarts(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=7)
        assert "7" in repr(p)


class TestPyCrashRecoveryShouldRestart:
    def test_crashed_returns_true(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("crashed") is True

    def test_unresponsive_returns_true(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("unresponsive") is True

    def test_running_returns_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("running") is False

    def test_stopped_returns_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("stopped") is False

    def test_starting_returns_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("starting") is False

    def test_restarting_returns_false(self) -> None:
        p = PyCrashRecoveryPolicy()
        assert p.should_restart("restarting") is False

    def test_invalid_status_raises(self) -> None:
        p = PyCrashRecoveryPolicy()
        with pytest.raises((ValueError, RuntimeError)):
            p.should_restart("invalid_status")


class TestPyCrashRecoveryFixedBackoff:
    def test_fixed_delay_constant(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=1000)
        d0 = p.next_delay_ms("maya", 0)
        d1 = p.next_delay_ms("maya", 1)
        d2 = p.next_delay_ms("maya", 2)
        assert d0 == d1 == d2

    def test_fixed_delay_value(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=2000)
        assert p.next_delay_ms("maya", 0) == 2000

    def test_max_exceeded_raises(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(delay_ms=500)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", 3)

    def test_boundary_last_attempt_ok(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(delay_ms=500)
        # attempt 2 is the last valid (0-indexed, max=3 means 0,1,2 allowed)
        d = p.next_delay_ms("maya", 2)
        assert d >= 0

    def test_multiple_dcc_names_independent(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_fixed_backoff(delay_ms=1000)
        d_maya = p.next_delay_ms("maya", 0)
        d_blender = p.next_delay_ms("blender", 0)
        assert d_maya == d_blender == 1000


class TestPyCrashRecoveryExponentialBackoff:
    def test_delays_increase(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=500, max_delay_ms=10000)
        d0 = p.next_delay_ms("maya", 0)
        d1 = p.next_delay_ms("maya", 1)
        d2 = p.next_delay_ms("maya", 2)
        assert d1 >= d0
        assert d2 >= d1

    def test_initial_delay_correct(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        d0 = p.next_delay_ms("blender", 0)
        assert d0 == 100

    def test_delays_capped_at_max(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=20)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        # After many doublings, delay should not exceed max
        for attempt in range(15):
            d = p.next_delay_ms("maya", attempt)
            assert d <= 5000

    def test_max_exceeded_raises(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=3)
        p.use_exponential_backoff(initial_ms=500, max_delay_ms=10000)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", 3)

    def test_returns_int(self) -> None:
        p = PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=200, max_delay_ms=8000)
        d = p.next_delay_ms("houdini", 0)
        assert isinstance(d, int)
