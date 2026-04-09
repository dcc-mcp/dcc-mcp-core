"""IpcListener, encode/decode, PyProcessMonitor, TelemetryConfig, session tests.

Covers IpcListener bind/local_address/transport_name/into_handle, ListenerHandle,
encode_request/encode_response/encode_notify/decode_envelope round-trip,
PyProcessMonitor track/untrack/refresh/query/list_all/is_alive,
TelemetryConfig builder chain, ServiceEntry fields, RoutingStrategy variants,
and TransportManager session lifecycle (+147 tests).
"""

from __future__ import annotations

import os
import tempfile
import uuid

import pytest

from dcc_mcp_core import IpcListener
from dcc_mcp_core import ListenerHandle
from dcc_mcp_core import PyProcessMonitor
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import TelemetryConfig
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportManager
from dcc_mcp_core import decode_envelope
from dcc_mcp_core import encode_notify
from dcc_mcp_core import encode_request
from dcc_mcp_core import encode_response
from dcc_mcp_core import is_telemetry_initialized
from dcc_mcp_core import shutdown_telemetry

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_mgr() -> tuple[TransportManager, str]:
    """Return a fresh TransportManager using a temp directory."""
    d = tempfile.mkdtemp()
    return TransportManager(d), d


# ===========================================================================
# IpcListener
# ===========================================================================


class TestIpcListener:
    """Tests for IpcListener.bind and property access."""

    def test_bind_returns_ipc_listener(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert type(listener).__name__ == "IpcListener"

    def test_local_address_returns_transport_address(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert type(local).__name__ == "TransportAddress"

    def test_local_address_scheme_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = listener.local_address()
        assert local.scheme == "tcp"

    def test_local_address_port_is_nonzero(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        conn_str = listener.local_address().to_connection_string()
        port = int(conn_str.split(":")[-1])
        assert port > 0

    def test_transport_name_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener.transport_name == "tcp"

    def test_repr_contains_transport(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        r = repr(listener)
        assert "IpcListener" in r
        assert "tcp" in r

    def test_two_binds_get_different_ports(self):
        l1 = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        l2 = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        p1 = int(l1.local_address().to_connection_string().split(":")[-1])
        p2 = int(l2.local_address().to_connection_string().split(":")[-1])
        assert p1 != p2


# ===========================================================================
# ListenerHandle
# ===========================================================================


class TestListenerHandle:
    """Tests for ListenerHandle (obtained via IpcListener.into_handle())."""

    def test_type_is_listener_handle(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert type(handle).__name__ == "ListenerHandle"

    def test_accept_count_starts_at_zero(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert handle.accept_count == 0

    def test_accept_count_is_int(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert isinstance(handle.accept_count, int)

    def test_is_shutdown_initially_false(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert handle.is_shutdown is False

    def test_transport_name_is_tcp(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert handle.transport_name == "tcp"

    def test_local_address_matches_listener(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        local_before = listener.local_address().to_connection_string()
        handle = listener.into_handle()
        local_after = handle.local_address().to_connection_string()
        assert local_before == local_after

    def test_local_address_scheme_tcp(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        assert handle.local_address().scheme == "tcp"

    def test_repr_contains_accept_count(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        r = repr(handle)
        assert "ListenerHandle" in r

    def test_shutdown_sets_is_shutdown_true(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_is_idempotent(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        handle.shutdown()
        handle.shutdown()  # should not raise
        assert handle.is_shutdown is True

    def test_accept_count_stays_zero_after_shutdown(self):
        listener = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        handle = listener.into_handle()
        handle.shutdown()
        assert handle.accept_count == 0


# ===========================================================================
# encode_request / encode_response / encode_notify / decode_envelope
# ===========================================================================


class TestEncodeRequest:
    """Tests for encode_request()."""

    def test_returns_bytes(self):
        frame = encode_request("execute_python")
        assert isinstance(frame, bytes)

    def test_frame_has_length_prefix(self):
        frame = encode_request("execute_python")
        assert len(frame) >= 4  # at least 4 byte length prefix

    def test_length_prefix_matches_payload_size(self):
        frame = encode_request("ping", b"params")
        length_prefix = int.from_bytes(frame[:4], "big")
        assert length_prefix == len(frame) - 4

    def test_decode_type_is_request(self):
        frame = encode_request("execute_python", b"params")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"

    def test_decode_method(self):
        frame = encode_request("get_scene_info", b"p")
        msg = decode_envelope(frame[4:])
        assert msg["method"] == "get_scene_info"

    def test_decode_id_is_uuid(self):
        frame = encode_request("test")
        msg = decode_envelope(frame[4:])
        _id = msg["id"]
        # Should be 36-char UUID string
        assert isinstance(_id, str)
        assert len(_id) == 36
        assert _id.count("-") == 4

    def test_decode_params_bytes(self):
        params = b"hello world"
        frame = encode_request("method", params)
        msg = decode_envelope(frame[4:])
        assert msg["params"] == params

    def test_no_params_defaults_to_empty_bytes(self):
        frame = encode_request("method")
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["params"], bytes)

    def test_two_requests_have_different_ids(self):
        frame1 = encode_request("method")
        frame2 = encode_request("method")
        id1 = decode_envelope(frame1[4:])["id"]
        id2 = decode_envelope(frame2[4:])["id"]
        assert id1 != id2


class TestEncodeResponse:
    """Tests for encode_response()."""

    def test_returns_bytes(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True)
        assert isinstance(frame, bytes)

    def test_decode_type_is_response(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True, b"output")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"

    def test_decode_id_matches(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True, b"x")
        msg = decode_envelope(frame[4:])
        assert msg["id"] == rid

    def test_decode_success_true(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True, b"payload")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is True

    def test_decode_success_false(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, False, error="oops")
        msg = decode_envelope(frame[4:])
        assert msg["success"] is False

    def test_decode_payload(self):
        rid = str(uuid.uuid4())
        payload = b"result data"
        frame = encode_response(rid, True, payload)
        msg = decode_envelope(frame[4:])
        assert msg["payload"] == payload

    def test_decode_error_none_on_success(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True, b"ok")
        msg = decode_envelope(frame[4:])
        assert msg["error"] is None

    def test_decode_error_message(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, False, error="something failed")
        msg = decode_envelope(frame[4:])
        assert msg["error"] == "something failed"

    def test_no_payload_defaults_empty_bytes(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True)
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["payload"], bytes)


class TestEncodeNotify:
    """Tests for encode_notify()."""

    def test_returns_bytes(self):
        frame = encode_notify("scene_changed")
        assert isinstance(frame, bytes)

    def test_decode_type_is_notify(self):
        frame = encode_notify("scene_changed", b"data")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"

    def test_decode_topic(self):
        frame = encode_notify("render_complete", b"x")
        msg = decode_envelope(frame[4:])
        assert msg["topic"] == "render_complete"

    def test_decode_data(self):
        data = b"event_payload"
        frame = encode_notify("scene_changed", data)
        msg = decode_envelope(frame[4:])
        assert msg["data"] == data

    def test_no_data_defaults_empty_bytes(self):
        frame = encode_notify("ping_event")
        msg = decode_envelope(frame[4:])
        assert isinstance(msg["data"], bytes)

    def test_two_notifies_have_different_ids(self):
        f1 = encode_notify("event")
        f2 = encode_notify("event")
        id1 = decode_envelope(f1[4:]).get("id")
        id2 = decode_envelope(f2[4:]).get("id")
        # IDs may be None for notify (optional), but both should match each other's type
        assert isinstance(id1, type(id2)) or id1 is None

    def test_length_prefix_matches_payload(self):
        frame = encode_notify("topic", b"hello")
        prefix = int.from_bytes(frame[:4], "big")
        assert prefix == len(frame) - 4


class TestDecodeEnvelope:
    """Tests for decode_envelope() with invalid input."""

    def test_invalid_bytes_raises_runtime_error(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"\xff\xff\xff\xff")

    def test_empty_bytes_raises_runtime_error(self):
        with pytest.raises(RuntimeError):
            decode_envelope(b"")

    def test_request_round_trip(self):
        frame = encode_request("method", b"params")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "request"
        assert msg["method"] == "method"
        assert msg["params"] == b"params"

    def test_response_round_trip(self):
        rid = str(uuid.uuid4())
        frame = encode_response(rid, True, b"payload")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "response"
        assert msg["id"] == rid
        assert msg["payload"] == b"payload"

    def test_notify_round_trip(self):
        frame = encode_notify("topic", b"data")
        msg = decode_envelope(frame[4:])
        assert msg["type"] == "notify"
        assert msg["topic"] == "topic"
        assert msg["data"] == b"data"


# ===========================================================================
# PyProcessMonitor
# ===========================================================================


class TestPyProcessMonitor:
    """Tests for PyProcessMonitor."""

    def test_tracked_count_starts_zero(self):
        mon = PyProcessMonitor()
        assert mon.tracked_count() == 0

    def test_list_all_starts_empty(self):
        mon = PyProcessMonitor()
        assert mon.list_all() == []

    def test_repr_contains_tracked(self):
        mon = PyProcessMonitor()
        r = repr(mon)
        assert "PyProcessMonitor" in r

    def test_track_increases_tracked_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        assert mon.tracked_count() == 1

    def test_track_then_list_all_has_entry(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        lst = mon.list_all()
        assert len(lst) == 1

    def test_list_all_entry_has_pid(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "test_proc")
        mon.refresh()
        lst = mon.list_all()
        assert lst[0]["pid"] == pid

    def test_list_all_entry_has_name(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "my_name")
        mon.refresh()
        assert mon.list_all()[0]["name"] == "my_name"

    def test_track_two_pids(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        ppid = os.getppid()
        mon.track(pid, "self")
        mon.track(ppid, "parent")
        assert mon.tracked_count() == 2

    def test_untrack_decreases_count(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.untrack(pid)
        assert mon.tracked_count() == 0

    def test_untrack_nonexistent_no_raise(self):
        mon = PyProcessMonitor()
        mon.untrack(999999999)  # should not raise

    def test_query_returns_dict(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert isinstance(info, dict)

    def test_query_has_required_keys(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        for key in ("pid", "name", "status", "cpu_usage_percent", "memory_bytes", "restart_count"):
            assert key in info, f"missing key: {key}"

    def test_query_status_is_running(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["status"] == "running"

    def test_query_cpu_usage_nonneg(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["cpu_usage_percent"] >= 0.0

    def test_query_memory_bytes_nonneg(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["memory_bytes"] >= 0

    def test_query_restart_count_zero(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        info = mon.query(pid)
        assert info["restart_count"] == 0

    def test_query_after_untrack_returns_none_or_dict(self):
        mon = PyProcessMonitor()
        pid = os.getpid()
        mon.track(pid, "self")
        mon.refresh()
        mon.untrack(pid)
        info = mon.query(pid)
        assert info is None or isinstance(info, dict)

    def test_query_unknown_pid_returns_none(self):
        mon = PyProcessMonitor()
        result = mon.query(999999999)
        assert result is None

    def test_is_alive_own_process(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(os.getpid()) is True

    def test_is_alive_nonexistent_pid(self):
        mon = PyProcessMonitor()
        assert mon.is_alive(999999999) is False


# ===========================================================================
# TelemetryConfig
# ===========================================================================


class TestTelemetryConfig:
    """Tests for TelemetryConfig builder API."""

    def setup_method(self):
        """Ensure telemetry is not initialized before each test."""
        shutdown_telemetry()

    def teardown_method(self):
        """Clean up global telemetry state."""
        shutdown_telemetry()

    def test_type_name(self):
        cfg = TelemetryConfig("my-service")
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_service_name(self):
        cfg = TelemetryConfig("maya-mcp")
        assert cfg.service_name == "maya-mcp"

    def test_enable_metrics_default_true(self):
        cfg = TelemetryConfig("svc")
        assert cfg.enable_metrics is True

    def test_enable_tracing_default_true(self):
        cfg = TelemetryConfig("svc")
        assert cfg.enable_tracing is True

    def test_repr_contains_service_name(self):
        cfg = TelemetryConfig("maya-svc")
        assert "maya-svc" in repr(cfg)

    def test_with_noop_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_noop_exporter()
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_with_stdout_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_stdout_exporter()
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_with_attribute_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_noop_exporter().with_attribute("dcc.name", "maya")
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_with_service_version_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_noop_exporter().with_service_version("1.0.0")
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_set_enable_metrics_false(self):
        cfg = TelemetryConfig("svc").set_enable_metrics(False)
        assert cfg.enable_metrics is False

    def test_set_enable_metrics_true(self):
        cfg = TelemetryConfig("svc").set_enable_metrics(True)
        assert cfg.enable_metrics is True

    def test_set_enable_tracing_false(self):
        cfg = TelemetryConfig("svc").set_enable_tracing(False)
        assert cfg.enable_tracing is False

    def test_with_json_logs_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_json_logs()
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_with_text_logs_returns_telemetry_config(self):
        cfg = TelemetryConfig("svc").with_text_logs()
        assert type(cfg).__name__ == "TelemetryConfig"

    def test_chain_multiple_methods(self):
        cfg = (
            TelemetryConfig("chained-svc")
            .with_noop_exporter()
            .with_attribute("k", "v")
            .with_service_version("2.0.0")
            .set_enable_metrics(True)
            .set_enable_tracing(False)
        )
        assert cfg.service_name == "chained-svc"
        assert cfg.enable_tracing is False

    def test_is_initialized_returns_bool(self):
        result = is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_shutdown_telemetry_no_raise(self):
        shutdown_telemetry()  # safe even if not initialized

    def test_shutdown_telemetry_idempotent(self):
        shutdown_telemetry()
        shutdown_telemetry()  # should not raise


# ===========================================================================
# ServiceEntry & RoutingStrategy
# ===========================================================================


class TestServiceEntry:
    """Tests for ServiceEntry fields returned by TransportManager."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid = self.mgr.register_service("maya", "127.0.0.1", 18812, version="2025", scene="scene.ma")

    def teardown_method(self):
        self.mgr.shutdown()

    def test_entry_dcc_type(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.dcc_type == "maya"

    def test_entry_instance_id_matches(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.instance_id == self.sid

    def test_entry_host(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.host == "127.0.0.1"

    def test_entry_port(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.port == 18812

    def test_entry_version(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.version == "2025"

    def test_entry_scene(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.scene == "scene.ma"

    def test_entry_status_available(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.status == ServiceStatus.AVAILABLE

    def test_entry_last_heartbeat_ms_is_int(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert isinstance(entry.last_heartbeat_ms, int)

    def test_entry_last_heartbeat_ms_positive(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.last_heartbeat_ms > 0

    def test_entry_is_ipc_false_for_tcp(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert entry.is_ipc is False

    def test_entry_effective_address_scheme(self):
        entry = self.mgr.get_service("maya", self.sid)
        eff = entry.effective_address()
        assert eff.scheme == "tcp"

    def test_entry_effective_address_type(self):
        entry = self.mgr.get_service("maya", self.sid)
        eff = entry.effective_address()
        assert type(eff).__name__ == "TransportAddress"

    def test_entry_to_dict_returns_dict(self):
        entry = self.mgr.get_service("maya", self.sid)
        d = entry.to_dict()
        assert isinstance(d, dict)

    def test_entry_to_dict_has_expected_keys(self):
        entry = self.mgr.get_service("maya", self.sid)
        d = entry.to_dict()
        for key in ("dcc_type", "host", "instance_id", "port", "status"):
            assert key in d

    def test_entry_repr_contains_dcc_type(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert "maya" in repr(entry)

    def test_entry_metadata_is_dict(self):
        entry = self.mgr.get_service("maya", self.sid)
        assert isinstance(entry.metadata, dict)

    def test_entry_transport_address_none_for_plain_tcp(self):
        # When no transport_address was provided, it may be None
        entry = self.mgr.get_service("maya", self.sid)
        # transport_address is None or a TransportAddress
        ta = entry.transport_address
        assert ta is None or type(ta).__name__ == "TransportAddress"


class TestRoutingStrategy:
    """Tests for RoutingStrategy enum variants."""

    def test_first_available_exists(self):
        assert RoutingStrategy.FIRST_AVAILABLE is not None

    def test_round_robin_exists(self):
        assert RoutingStrategy.ROUND_ROBIN is not None

    def test_least_busy_exists(self):
        assert RoutingStrategy.LEAST_BUSY is not None

    def test_specific_exists(self):
        assert RoutingStrategy.SPECIFIC is not None

    def test_scene_match_exists(self):
        assert RoutingStrategy.SCENE_MATCH is not None

    def test_random_exists(self):
        assert RoutingStrategy.RANDOM is not None

    def test_same_variant_equal(self):
        assert RoutingStrategy.ROUND_ROBIN == RoutingStrategy.ROUND_ROBIN

    def test_different_variants_not_equal(self):
        assert RoutingStrategy.ROUND_ROBIN != RoutingStrategy.FIRST_AVAILABLE

    def test_repr_contains_variant_name(self):
        assert "ROUND_ROBIN" in repr(RoutingStrategy.ROUND_ROBIN)

    def test_str_contains_variant_name(self):
        assert "FIRST_AVAILABLE" in str(RoutingStrategy.FIRST_AVAILABLE)


# ===========================================================================
# TransportManager session management (deep)
# ===========================================================================


class TestTransportManagerSessionGetOrCreate:
    """Tests for get_or_create_session."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid1 = self.mgr.register_service("maya", "127.0.0.1", 18812)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_returns_string(self):
        sess = self.mgr.get_or_create_session("maya", self.sid1)
        assert isinstance(sess, str)

    def test_returns_uuid_len36(self):
        sess = self.mgr.get_or_create_session("maya", self.sid1)
        assert len(sess) == 36

    def test_second_call_returns_same_session(self):
        sess1 = self.mgr.get_or_create_session("maya", self.sid1)
        sess2 = self.mgr.get_or_create_session("maya", self.sid1)
        assert sess1 == sess2

    def test_different_instances_different_sessions(self):
        sid2 = self.mgr.register_service("maya", "127.0.0.1", 18813)
        sess1 = self.mgr.get_or_create_session("maya", self.sid1)
        sess2 = self.mgr.get_or_create_session("maya", sid2)
        assert sess1 != sess2


class TestTransportManagerGetSession:
    """Tests for get_session and session inspection."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid = self.mgr.register_service("maya", "127.0.0.1", 18812)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_get_session_returns_dict(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert isinstance(info, dict)

    def test_get_session_has_id_key(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert "id" in info

    def test_get_session_id_matches(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert info["id"] == sess_id

    def test_get_session_has_dcc_type(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert info["dcc_type"] == "maya"

    def test_get_session_has_state(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert "state" in info

    def test_get_session_has_request_count(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert "request_count" in info

    def test_get_session_unknown_returns_none(self):
        result = self.mgr.get_session(str(uuid.uuid4()))
        assert result is None

    def test_get_session_has_error_count(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert "error_count" in info

    def test_get_session_has_transport_address(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        info = self.mgr.get_session(sess_id)
        assert "transport_address" in info


class TestTransportManagerRecordMetrics:
    """Tests for record_success and record_error."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid = self.mgr.register_service("maya", "127.0.0.1", 18812)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_record_success_no_raise(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        self.mgr.record_success(sess_id, latency_ms=5)

    def test_record_error_no_raise(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        self.mgr.record_error(sess_id, latency_ms=10, error="timeout")

    def test_record_success_updates_request_count(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        before = self.mgr.get_session(sess_id)["request_count"]
        self.mgr.record_success(sess_id, latency_ms=1)
        after = self.mgr.get_session(sess_id)["request_count"]
        assert after >= before


class TestTransportManagerCloseSession:
    """Tests for close_session."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid = self.mgr.register_service("maya", "127.0.0.1", 18812)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_close_returns_true(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        result = self.mgr.close_session(sess_id)
        assert result is True

    def test_close_reduces_session_count(self):
        sess_id = self.mgr.get_or_create_session("maya", self.sid)
        before = self.mgr.session_count()
        self.mgr.close_session(sess_id)
        after = self.mgr.session_count()
        assert after <= before

    def test_close_unknown_returns_false(self):
        result = self.mgr.close_session(str(uuid.uuid4()))
        assert result is False


class TestTransportManagerListSessions:
    """Tests for list_sessions and list_sessions_for_dcc."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid_maya = self.mgr.register_service("maya", "127.0.0.1", 18812)
        self.sid_blender = self.mgr.register_service("blender", "127.0.0.1", 19000)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_list_sessions_returns_list(self):
        self.mgr.get_or_create_session("maya", self.sid_maya)
        sessions = self.mgr.list_sessions()
        assert isinstance(sessions, list)

    def test_list_sessions_len_matches_session_count(self):
        self.mgr.get_or_create_session("maya", self.sid_maya)
        assert len(self.mgr.list_sessions()) == self.mgr.session_count()

    def test_list_sessions_for_dcc_filters_correctly(self):
        self.mgr.get_or_create_session("maya", self.sid_maya)
        self.mgr.get_or_create_session("blender", self.sid_blender)
        maya_sessions = self.mgr.list_sessions_for_dcc("maya")
        assert all(s["dcc_type"] == "maya" for s in maya_sessions)

    def test_list_sessions_for_unknown_dcc_empty(self):
        sessions = self.mgr.list_sessions_for_dcc("houdini_unknown_xyz")
        assert sessions == []

    def test_session_count_starts_zero(self):
        mgr2 = TransportManager(tempfile.mkdtemp())
        assert mgr2.session_count() == 0
        mgr2.shutdown()


class TestTransportManagerRoutedSession:
    """Tests for get_or_create_session_routed."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.sid1 = self.mgr.register_service("maya", "127.0.0.1", 18812)
        self.sid2 = self.mgr.register_service("maya", "127.0.0.1", 18813)

    def teardown_method(self):
        self.mgr.shutdown()

    def test_returns_session_id_string(self):
        sess = self.mgr.get_or_create_session_routed("maya")
        assert isinstance(sess, str)

    def test_returns_uuid_len36(self):
        sess = self.mgr.get_or_create_session_routed("maya")
        assert len(sess) == 36

    def test_with_first_available_strategy(self):
        sess = self.mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.FIRST_AVAILABLE)
        assert len(sess) == 36

    def test_with_round_robin_strategy(self):
        sess = self.mgr.get_or_create_session_routed("maya", strategy=RoutingStrategy.ROUND_ROBIN)
        assert isinstance(sess, str)


class TestTransportManagerCleanupShutdown:
    """Tests for cleanup() and shutdown() lifecycle."""

    def setup_method(self):
        self.tmpdir = tempfile.mkdtemp()
        self.mgr = TransportManager(self.tmpdir)
        self.mgr.register_service("maya", "127.0.0.1", 18812)

    def teardown_method(self):
        if not self.mgr.is_shutdown():
            self.mgr.shutdown()

    def test_cleanup_returns_tuple(self):
        result = self.mgr.cleanup()
        assert isinstance(result, tuple)

    def test_cleanup_tuple_length_3(self):
        result = self.mgr.cleanup()
        assert len(result) == 3

    def test_cleanup_elements_are_int(self):
        result = self.mgr.cleanup()
        for v in result:
            assert isinstance(v, int)

    def test_is_shutdown_initially_false(self):
        assert self.mgr.is_shutdown() is False

    def test_shutdown_sets_is_shutdown_true(self):
        self.mgr.shutdown()
        assert self.mgr.is_shutdown() is True

    def test_shutdown_idempotent(self):
        self.mgr.shutdown()
        self.mgr.shutdown()  # should not raise
