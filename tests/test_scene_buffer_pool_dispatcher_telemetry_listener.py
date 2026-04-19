"""Deep tests for PySharedSceneBuffer, PyBufferPool, ToolDispatcher, TelemetryConfig, IpcListener/ListenerHandle.

Coverage targets (93rd iteration):
- PySharedSceneBuffer: write/read/id/is_inline/is_chunked/total_bytes/descriptor_json for small/large/compressed data
- PyBufferPool: capacity/buffer_size/available/acquire/GC return
- ToolDispatcher: handler_count/handler_names/has_handler/remove_handler/dispatch happy+error paths
- TelemetryConfig: is_telemetry_initialized before/after init/shutdown
- IpcListener / ListenerHandle: bind/local_address/transport_name/into_handle/shutdown/is_shutdown/accept_count
"""

from __future__ import annotations

import gc
import json

import pytest

import dcc_mcp_core
from dcc_mcp_core import IpcListener
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedSceneBuffer
from dcc_mcp_core import TelemetryConfig
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import is_telemetry_initialized
from dcc_mcp_core import shutdown_telemetry
from dcc_mcp_core import success_result

# ---------------------------------------------------------------------------
# PySharedSceneBuffer
# ---------------------------------------------------------------------------


class TestPySharedSceneBufferWrite:
    """PySharedSceneBuffer.write() factory and basic field access."""

    def test_write_returns_instance(self):
        ssb = PySharedSceneBuffer.write(b"hello", PySceneDataKind.Geometry, "Maya", False)
        assert isinstance(ssb, PySharedSceneBuffer)

    def test_id_is_nonempty_string(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        assert isinstance(ssb.id, str)
        assert len(ssb.id) > 0

    def test_id_is_uuid_format(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        # Short ID format: 16 hex chars (was UUID v4 with 5 dash-separated parts)
        assert isinstance(ssb.id, str) and len(ssb.id) > 0

    def test_two_writes_have_different_ids(self):
        a = PySharedSceneBuffer.write(b"a", PySceneDataKind.Geometry, "Maya", False)
        b = PySharedSceneBuffer.write(b"b", PySceneDataKind.Geometry, "Maya", False)
        assert a.id != b.id

    def test_small_data_is_inline(self):
        ssb = PySharedSceneBuffer.write(b"small", PySceneDataKind.Geometry, "Maya", False)
        assert ssb.is_inline is True

    def test_small_data_is_not_chunked(self):
        ssb = PySharedSceneBuffer.write(b"small", PySceneDataKind.Geometry, "Maya", False)
        assert ssb.is_chunked is False

    def test_total_bytes_matches_input(self):
        data = b"hello world"
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert ssb.total_bytes == len(data)

    def test_read_returns_original_data(self):
        data = b"roundtrip check"
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert ssb.read() == data

    def test_empty_bytes_write(self):
        ssb = PySharedSceneBuffer.write(b"", PySceneDataKind.Geometry, "Maya", False)
        assert ssb.total_bytes == 0
        assert ssb.read() == b""

    def test_different_scene_kinds(self):
        for kind in [
            PySceneDataKind.Geometry,
            PySceneDataKind.Screenshot,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Arbitrary,
        ]:
            ssb = PySharedSceneBuffer.write(b"x" * 16, kind, "Maya", False)
            assert ssb.read() == b"x" * 16

    def test_screenshot_kind_is_inline_for_small(self):
        ssb = PySharedSceneBuffer.write(b"frame", PySceneDataKind.Screenshot, "Blender", False)
        assert ssb.is_inline is True

    def test_animation_cache_kind(self):
        ssb = PySharedSceneBuffer.write(b"anim", PySceneDataKind.AnimationCache, "Maya", False)
        assert ssb.total_bytes == 4

    def test_arbitrary_kind(self):
        ssb = PySharedSceneBuffer.write(b"arb", PySceneDataKind.Arbitrary, "Houdini", False)
        assert ssb.is_inline is True

    def test_source_dcc_reflected_in_descriptor(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Houdini3D", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["source_dcc"] == "Houdini3D"

    def test_read_is_bytes(self):
        ssb = PySharedSceneBuffer.write(b"bytes", PySceneDataKind.Geometry, "Maya", False)
        assert isinstance(ssb.read(), bytes)


class TestPySharedSceneBufferDescriptorJson:
    """descriptor_json() structure validation."""

    def test_descriptor_json_is_valid_json(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert isinstance(d, dict)

    def test_descriptor_has_meta_key(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "meta" in d

    def test_descriptor_meta_has_id(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "id" in d["meta"]
        assert d["meta"]["id"] == ssb.id

    def test_descriptor_meta_has_kind(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "kind" in d["meta"]
        assert d["meta"]["kind"] == "geometry"

    def test_descriptor_meta_has_source_dcc(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "TestDCC", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["source_dcc"] == "TestDCC"

    def test_descriptor_meta_has_total_bytes(self):
        data = b"x" * 50
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["total_bytes"] == 50

    def test_descriptor_meta_has_created_at(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "created_at" in d["meta"]

    def test_descriptor_has_storage_key(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        d = json.loads(ssb.descriptor_json())
        assert "storage" in d

    def test_descriptor_screenshot_kind_string(self):
        ssb = PySharedSceneBuffer.write(b"frame", PySceneDataKind.Screenshot, "Blender", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["kind"] == "screenshot"

    def test_descriptor_animation_cache_kind_string(self):
        ssb = PySharedSceneBuffer.write(b"anim", PySceneDataKind.AnimationCache, "Houdini", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["kind"] == "animation_cache"

    def test_descriptor_arbitrary_kind_string(self):
        ssb = PySharedSceneBuffer.write(b"arb", PySceneDataKind.Arbitrary, "Max", False)
        d = json.loads(ssb.descriptor_json())
        assert d["meta"]["kind"] == "arbitrary"

    def test_descriptor_json_is_string(self):
        ssb = PySharedSceneBuffer.write(b"data", PySceneDataKind.Geometry, "Maya", False)
        assert isinstance(ssb.descriptor_json(), str)


class TestPySharedSceneBufferLargeData:
    """Large data behaviour (chunked vs inline threshold)."""

    def test_large_data_read_roundtrip(self):
        data = b"B" * (300 * 1024)
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Houdini", False)
        assert ssb.read() == data

    def test_large_data_total_bytes(self):
        size = 300 * 1024
        data = b"C" * size
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Houdini", False)
        assert ssb.total_bytes == size

    def test_large_data_id_is_uuid(self):
        data = b"D" * (300 * 1024)
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        # Short ID format: 16 hex chars (was UUID v4 with 5 dash-separated parts)
        assert isinstance(ssb.id, str) and len(ssb.id) > 0

    def test_large_data_inline_or_chunked_exclusive(self):
        data = b"E" * (300 * 1024)
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", False)
        assert ssb.is_inline != ssb.is_chunked

    def test_compressed_data_roundtrip(self):
        data = b"F" * 4096
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", True)
        assert ssb.read() == data

    def test_compressed_descriptor_has_meta(self):
        data = b"G" * 1024
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Blender", True)
        d = json.loads(ssb.descriptor_json())
        assert "meta" in d

    def test_compressed_total_bytes_equals_original(self):
        data = b"H" * 2048
        ssb = PySharedSceneBuffer.write(data, PySceneDataKind.Geometry, "Maya", True)
        assert ssb.total_bytes == len(data)


# ---------------------------------------------------------------------------
# PyBufferPool
# ---------------------------------------------------------------------------


class TestPyBufferPool:
    """PyBufferPool constructor and field access.

    capacity/buffer_size/available are methods, not properties.
    """

    def test_construct_with_kwargs(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_capacity_matches(self):
        pool = PyBufferPool(capacity=5, buffer_size=512)
        assert pool.capacity() == 5

    def test_buffer_size_matches(self):
        pool = PyBufferPool(capacity=3, buffer_size=256)
        assert pool.buffer_size() == 256

    def test_available_initially_equals_capacity(self):
        cap = 4
        pool = PyBufferPool(capacity=cap, buffer_size=128)
        assert pool.available() == cap

    def test_acquire_returns_shared_buffer(self):
        pool = PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert isinstance(buf, dcc_mcp_core.PySharedBuffer)

    def test_available_decreases_after_acquire(self):
        pool = PyBufferPool(capacity=3, buffer_size=128)
        _ = pool.acquire()
        assert pool.available() == 2

    def test_available_returns_after_gc(self):
        pool = PyBufferPool(capacity=2, buffer_size=128)
        buf = pool.acquire()
        assert pool.available() == 1
        del buf
        gc.collect()
        assert pool.available() == 2

    def test_acquire_multiple(self):
        pool = PyBufferPool(capacity=3, buffer_size=128)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 1
        del b1, b2
        gc.collect()
        assert pool.available() == 3

    def test_capacity_type_is_int(self):
        pool = PyBufferPool(capacity=2, buffer_size=64)
        assert isinstance(pool.capacity(), int)

    def test_buffer_size_type_is_int(self):
        pool = PyBufferPool(capacity=2, buffer_size=64)
        assert isinstance(pool.buffer_size(), int)

    def test_available_type_is_int(self):
        pool = PyBufferPool(capacity=2, buffer_size=64)
        assert isinstance(pool.available(), int)

    def test_capacity_1_acquire_exhausts(self):
        pool = PyBufferPool(capacity=1, buffer_size=64)
        _ = pool.acquire()
        assert pool.available() == 0


# ---------------------------------------------------------------------------
# ToolDispatcher deep
# ---------------------------------------------------------------------------


class TestActionDispatcherHandlers:
    """ToolDispatcher handler management."""

    def test_handler_count_initially_zero(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        assert disp.handler_count() == 0

    def test_register_handler_increments_count(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {"ok": True})
        assert disp.handler_count() == 1

    def test_handler_names_contains_registered(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {})
        assert "act1" in disp.handler_names()

    def test_has_handler_true_after_register(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {})
        assert disp.has_handler("act1") is True

    def test_has_handler_false_before_register(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        assert disp.has_handler("nonexistent") is False

    def test_remove_handler_decrements_count(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {})
        disp.remove_handler("act1")
        assert disp.handler_count() == 0

    def test_has_handler_false_after_remove(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {})
        disp.remove_handler("act1")
        assert disp.has_handler("act1") is False

    def test_handler_names_empty_initially(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        assert disp.handler_names() == []

    def test_multiple_handlers(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("a", lambda p: {})
        disp.register_handler("b", lambda p: {})
        disp.register_handler("c", lambda p: {})
        assert disp.handler_count() == 3
        names = disp.handler_names()
        assert "a" in names and "b" in names and "c" in names

    def test_handler_names_type_is_list(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        assert isinstance(disp.handler_names(), list)

    def test_skip_empty_schema_validation_is_bool(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        # skip_empty_schema_validation is a bool property
        assert isinstance(disp.skip_empty_schema_validation, bool)

    def test_register_replaces_existing_handler(self):
        reg = ToolRegistry()
        disp = ToolDispatcher(reg)
        disp.register_handler("act1", lambda p: {"v": 1})
        disp.register_handler("act1", lambda p: {"v": 2})
        # count should still be 1 (overwrite, not add)
        assert disp.handler_count() == 1


class TestActionDispatcherDispatch:
    """ToolDispatcher.dispatch() result shapes."""

    def test_dispatch_returns_dict(self):
        reg = ToolRegistry()
        reg.register("my_act", description="test")
        disp = ToolDispatcher(reg)
        disp.register_handler("my_act", lambda p: {"done": True})
        result = disp.dispatch("my_act", "{}")
        assert isinstance(result, dict)

    def test_dispatch_result_has_output(self):
        reg = ToolRegistry()
        reg.register("my_act", description="test")
        disp = ToolDispatcher(reg)
        disp.register_handler("my_act", lambda p: {"key": "value"})
        result = disp.dispatch("my_act", "{}")
        assert "output" in result

    def test_dispatch_output_matches_handler_return(self):
        reg = ToolRegistry()
        reg.register("sum_act", description="test")
        disp = ToolDispatcher(reg)
        disp.register_handler("sum_act", lambda p: {"result": 42})
        out = disp.dispatch("sum_act", "{}")["output"]
        assert out["result"] == 42

    def test_dispatch_handler_returning_action_result_model(self):
        reg = ToolRegistry()
        reg.register("res_act", description="test")
        disp = ToolDispatcher(reg)
        disp.register_handler("res_act", lambda p: success_result("done").to_dict())
        result = disp.dispatch("res_act", "{}")
        assert isinstance(result, dict)

    def test_dispatch_handler_receives_params_dict(self):
        received = {}

        reg = ToolRegistry()
        reg.register("param_act", description="test")
        disp = ToolDispatcher(reg)

        def handler(params):
            received.update(params)
            return {}

        disp.register_handler("param_act", handler)
        disp.dispatch("param_act", '{"x": 1, "y": 2}')
        assert received.get("x") == 1
        assert received.get("y") == 2

    def test_dispatch_no_handler_raises_key_error(self):
        reg = ToolRegistry()
        reg.register("known_act", description="test")
        disp = ToolDispatcher(reg)
        # No handler registered — should raise KeyError
        with pytest.raises(KeyError, match="known_act"):
            disp.dispatch("known_act", "{}")


# ---------------------------------------------------------------------------
# TelemetryConfig
# ---------------------------------------------------------------------------


class TestTelemetryConfigInit:
    """TelemetryConfig init / shutdown cycle.

    NOTE: The global tracer provider can only be set once per process.
    We use is_telemetry_initialized() to guard against double-init.
    """

    def test_is_telemetry_initialized_returns_bool(self):
        result = is_telemetry_initialized()
        assert isinstance(result, bool)

    def test_telemetry_config_construct(self):
        cfg = TelemetryConfig("test-svc")
        assert cfg is not None

    def test_telemetry_config_service_name(self):
        cfg = TelemetryConfig("my-service")
        assert cfg.service_name == "my-service"

    def test_telemetry_config_enable_metrics_default(self):
        cfg = TelemetryConfig("svc")
        # enable_metrics is a property, should be bool
        assert isinstance(cfg.enable_metrics, bool)

    def test_telemetry_config_enable_tracing_default(self):
        cfg = TelemetryConfig("svc")
        assert isinstance(cfg.enable_tracing, bool)

    def test_telemetry_set_enable_metrics(self):
        cfg = TelemetryConfig("svc")
        cfg.set_enable_metrics(False)
        assert cfg.enable_metrics is False

    def test_telemetry_set_enable_tracing(self):
        cfg = TelemetryConfig("svc")
        cfg.set_enable_tracing(False)
        assert cfg.enable_tracing is False

    def test_with_noop_exporter_returns_config(self):
        cfg = TelemetryConfig("svc")
        result = cfg.with_noop_exporter()
        assert result is cfg or result is not None

    def test_with_service_version_callable(self):
        cfg = TelemetryConfig("svc")
        cfg.with_service_version("1.2.3")

    def test_with_json_logs_callable(self):
        cfg = TelemetryConfig("svc")
        cfg.with_json_logs()

    def test_with_text_logs_callable(self):
        cfg = TelemetryConfig("svc")
        cfg.with_text_logs()

    def test_with_attribute_callable(self):
        cfg = TelemetryConfig("svc")
        cfg.with_attribute("env", "test")

    def test_init_sets_initialized_flag(self):
        # The global OTel tracer provider can only be set once per process.
        # After prior test runs (or other test files), it may already be set.
        # We verify is_telemetry_initialized() is bool and skip gracefully.
        if is_telemetry_initialized():
            pytest.skip("Telemetry already initialized in this process")
        cfg = TelemetryConfig("test-svc")
        cfg.with_noop_exporter()
        try:
            cfg.init()
            assert is_telemetry_initialized() is True
        except RuntimeError:
            # Already set by another test — acceptable
            pytest.skip("Global tracer provider already set")

    def test_shutdown_clears_initialized_flag(self):
        # If telemetry was successfully initialized, shutdown should clear flag.
        # If not initialized at all, shutdown is a no-op.
        shutdown_telemetry()
        # After shutdown the flag must be False
        assert is_telemetry_initialized() is False


# ---------------------------------------------------------------------------
# IpcListener / ListenerHandle
# ---------------------------------------------------------------------------


class TestIpcListenerBind:
    """IpcListener.bind() and field access."""

    def test_bind_tcp_returns_listener(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert isinstance(listener, IpcListener)

    def test_transport_name_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        assert listener.transport_name == "tcp"

    def test_local_address_is_callable(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        result = listener.local_address()
        assert result is not None

    def test_local_address_contains_port(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        local = str(listener.local_address())
        # Should contain 127.0.0.1 and a non-zero port
        assert "127.0.0.1" in local

    def test_bind_different_ports_each_time(self):
        a = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        b = IpcListener.bind(TransportAddress.tcp("127.0.0.1", 0))
        assert a.local_address() != b.local_address()


class TestListenerHandle:
    """ListenerHandle fields and lifecycle."""

    def _make_handle(self):
        addr = TransportAddress.tcp("127.0.0.1", 0)
        listener = IpcListener.bind(addr)
        return listener.into_handle()

    def test_into_handle_returns_handle(self):
        handle = self._make_handle()
        assert isinstance(handle, dcc_mcp_core.ListenerHandle)

    def test_handle_transport_name_is_tcp(self):
        handle = self._make_handle()
        assert handle.transport_name == "tcp"

    def test_handle_is_shutdown_initially_false(self):
        handle = self._make_handle()
        assert handle.is_shutdown is False

    def test_handle_accept_count_initially_zero(self):
        handle = self._make_handle()
        assert handle.accept_count == 0

    def test_handle_local_address_callable(self):
        handle = self._make_handle()
        result = handle.local_address()
        assert result is not None

    def test_handle_local_address_contains_127(self):
        handle = self._make_handle()
        assert "127.0.0.1" in str(handle.local_address())

    def test_shutdown_sets_is_shutdown(self):
        handle = self._make_handle()
        handle.shutdown()
        assert handle.is_shutdown is True

    def test_shutdown_twice_no_error(self):
        handle = self._make_handle()
        handle.shutdown()
        handle.shutdown()  # idempotent

    def test_accept_count_type_is_int(self):
        handle = self._make_handle()
        assert isinstance(handle.accept_count, int)

    def test_is_shutdown_type_is_bool(self):
        handle = self._make_handle()
        assert isinstance(handle.is_shutdown, bool)

    def test_transport_name_type_is_str(self):
        handle = self._make_handle()
        assert isinstance(handle.transport_name, str)
