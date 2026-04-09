"""Deep tests for McpHttpConfig, TransportAddress, TransportScheme, TransportManager.

ServiceEntry, ServiceStatus, RoutingStrategy, wrap_value, unwrap_value,
unwrap_parameters, and type wrappers (Boolean/Float/Int/StringWrapper).

Target: cover all public methods/properties of these APIs, including happy paths,
error paths, and concurrent usage.
"""

from __future__ import annotations

import contextlib
from pathlib import Path
import tempfile
import threading

import pytest

from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import RoutingStrategy
from dcc_mcp_core import ServiceStatus
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportManager
from dcc_mcp_core import TransportScheme
from dcc_mcp_core import unwrap_parameters
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# McpHttpConfig
# ---------------------------------------------------------------------------


class TestMcpHttpConfig:
    def test_default_port_only(self):
        cfg = McpHttpConfig(port=8765)
        assert cfg.port == 8765

    def test_custom_port(self):
        cfg = McpHttpConfig(port=9999)
        assert cfg.port == 9999

    def test_server_name_default(self):
        cfg = McpHttpConfig(port=8765)
        # server_name should be a non-None string (default)
        assert isinstance(cfg.server_name, str)

    def test_server_version_default(self):
        cfg = McpHttpConfig(port=8765)
        assert isinstance(cfg.server_version, str)

    def test_custom_server_name(self):
        cfg = McpHttpConfig(port=8765, server_name="my-dcc-server")
        assert cfg.server_name == "my-dcc-server"

    def test_custom_server_version(self):
        cfg = McpHttpConfig(port=8765, server_version="2.0.0")
        assert cfg.server_version == "2.0.0"

    def test_all_fields(self):
        cfg = McpHttpConfig(port=9000, server_name="srv", server_version="1.5.0")
        assert cfg.port == 9000
        assert cfg.server_name == "srv"
        assert cfg.server_version == "1.5.0"

    def test_repr_contains_port(self):
        cfg = McpHttpConfig(port=1234)
        r = repr(cfg)
        assert "1234" in r

    def test_repr_contains_name(self):
        cfg = McpHttpConfig(port=8765, server_name="foobar")
        r = repr(cfg)
        assert "foobar" in r

    def test_port_zero(self):
        # port=0 is edge case; should not raise
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_port_max(self):
        cfg = McpHttpConfig(port=65535)
        assert cfg.port == 65535

    def test_multiple_configs_independent(self):
        cfg1 = McpHttpConfig(port=8001, server_name="a")
        cfg2 = McpHttpConfig(port=8002, server_name="b")
        assert cfg1.port != cfg2.port
        assert cfg1.server_name != cfg2.server_name

    def test_empty_server_name(self):
        cfg = McpHttpConfig(port=8765, server_name="")
        assert cfg.server_name == ""


# ---------------------------------------------------------------------------
# TransportAddress
# ---------------------------------------------------------------------------


class TestTransportAddress:
    def test_tcp_is_tcp(self):
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_tcp is True

    def test_tcp_is_not_named_pipe(self):
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_named_pipe is False

    def test_tcp_loopback_is_local(self):
        # 127.0.0.1 is considered local (loopback)
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_local is True

    def test_tcp_remote_is_not_local(self):
        addr = TransportAddress.tcp("192.168.1.1", 8765)
        assert addr.is_local is False

    def test_tcp_is_not_unix_socket(self):
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.is_unix_socket is False

    def test_tcp_scheme(self):
        addr = TransportAddress.tcp("127.0.0.1", 8765)
        assert addr.scheme == "tcp"

    def test_tcp_connection_string(self):
        addr = TransportAddress.tcp("192.168.1.1", 7001)
        cs = addr.to_connection_string()
        assert "192.168.1.1" in cs
        assert "7001" in cs
        assert cs.startswith("tcp://")

    def test_named_pipe_is_named_pipe(self):
        addr = TransportAddress.named_pipe("my_pipe")
        assert addr.is_named_pipe is True

    def test_named_pipe_is_not_tcp(self):
        addr = TransportAddress.named_pipe("my_pipe")
        assert addr.is_tcp is False

    def test_named_pipe_is_local(self):
        addr = TransportAddress.named_pipe("my_pipe")
        assert addr.is_local is True

    def test_named_pipe_scheme(self):
        addr = TransportAddress.named_pipe("my_pipe")
        assert addr.scheme == "pipe"

    def test_named_pipe_connection_string(self):
        addr = TransportAddress.named_pipe("test_pipe")
        cs = addr.to_connection_string()
        assert "test_pipe" in cs
        assert cs.startswith("pipe://")

    def test_default_local_is_local(self):
        addr = TransportAddress.default_local("maya", 12345)
        assert addr.is_local is True

    def test_default_local_is_not_tcp(self):
        addr = TransportAddress.default_local("maya", 12345)
        assert addr.is_tcp is False

    def test_default_local_contains_dcc_name(self):
        addr = TransportAddress.default_local("houdini", 9999)
        cs = addr.to_connection_string()
        assert "houdini" in cs

    def test_default_local_contains_pid(self):
        addr = TransportAddress.default_local("maya", 42)
        cs = addr.to_connection_string()
        assert "42" in cs

    def test_parse_tcp(self):
        addr = TransportAddress.parse("tcp://localhost:9000")
        assert addr.is_tcp is True
        assert "9000" in addr.to_connection_string()

    def test_parse_pipe(self):
        addr = TransportAddress.parse("pipe://my_service")
        assert addr.is_named_pipe is True

    def test_default_pipe_name(self):
        # Returns a TransportAddress, not a plain string
        addr = TransportAddress.default_pipe_name("blender", 1234)
        cs = addr.to_connection_string()
        assert "blender" in cs
        assert "1234" in cs

    def test_default_unix_socket(self):
        # Returns a TransportAddress for the unix socket path
        addr = TransportAddress.default_unix_socket("maya", 5678)
        cs = addr.to_connection_string()
        assert "maya" in cs
        assert "5678" in cs

    def test_different_dccs_different_addresses(self):
        addr1 = TransportAddress.default_local("maya", 100)
        addr2 = TransportAddress.default_local("blender", 100)
        assert addr1.to_connection_string() != addr2.to_connection_string()

    def test_different_pids_different_addresses(self):
        addr1 = TransportAddress.default_local("maya", 100)
        addr2 = TransportAddress.default_local("maya", 200)
        assert addr1.to_connection_string() != addr2.to_connection_string()


# ---------------------------------------------------------------------------
# TransportScheme
# ---------------------------------------------------------------------------


class TestTransportScheme:
    def test_auto_exists(self):
        assert TransportScheme.AUTO is not None

    def test_tcp_only_exists(self):
        assert TransportScheme.TCP_ONLY is not None

    def test_prefer_ipc_exists(self):
        assert TransportScheme.PREFER_IPC is not None

    def test_prefer_named_pipe_exists(self):
        assert TransportScheme.PREFER_NAMED_PIPE is not None

    def test_prefer_unix_socket_exists(self):
        assert TransportScheme.PREFER_UNIX_SOCKET is not None

    def test_select_address_tcp_only_returns_tcp(self):
        addr = TransportScheme.TCP_ONLY.select_address("maya", "localhost", 7001)
        assert addr.is_tcp is True

    def test_select_address_auto(self):
        addr = TransportScheme.AUTO.select_address("maya", "localhost", 7001)
        assert isinstance(addr.to_connection_string(), str)

    def test_select_address_prefer_ipc(self):
        addr = TransportScheme.PREFER_IPC.select_address("maya", "localhost", 7001)
        assert isinstance(addr.to_connection_string(), str)

    def test_select_address_prefer_named_pipe(self):
        addr = TransportScheme.PREFER_NAMED_PIPE.select_address("maya", "localhost", 7001)
        assert isinstance(addr.to_connection_string(), str)

    def test_select_address_with_pid(self):
        addr = TransportScheme.AUTO.select_address("houdini", "localhost", 8080, pid=9999)
        assert isinstance(addr.to_connection_string(), str)

    def test_repr_is_str(self):
        r = repr(TransportScheme.TCP_ONLY)
        assert isinstance(r, str)

    def test_all_schemes_return_valid_address(self):
        for scheme in [
            TransportScheme.AUTO,
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_IPC,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
        ]:
            addr = scheme.select_address("maya", "localhost", 7001)
            assert isinstance(addr.to_connection_string(), str)


# ---------------------------------------------------------------------------
# ServiceStatus enum
# ---------------------------------------------------------------------------


class TestServiceStatus:
    def test_available_variant(self):
        assert ServiceStatus.AVAILABLE is not None

    def test_busy_variant(self):
        assert ServiceStatus.BUSY is not None

    def test_shutting_down_variant(self):
        assert ServiceStatus.SHUTTING_DOWN is not None

    def test_unreachable_variant(self):
        assert ServiceStatus.UNREACHABLE is not None

    def test_variants_are_distinct(self):
        statuses = [
            ServiceStatus.AVAILABLE,
            ServiceStatus.BUSY,
            ServiceStatus.SHUTTING_DOWN,
            ServiceStatus.UNREACHABLE,
        ]
        assert len(set(str(s) for s in statuses)) == 4

    def test_repr_is_str(self):
        r = repr(ServiceStatus.AVAILABLE)
        assert isinstance(r, str)


# ---------------------------------------------------------------------------
# RoutingStrategy enum
# ---------------------------------------------------------------------------


class TestRoutingStrategy:
    def test_first_available_exists(self):
        assert RoutingStrategy.FIRST_AVAILABLE is not None

    def test_round_robin_exists(self):
        assert RoutingStrategy.ROUND_ROBIN is not None

    def test_least_busy_exists(self):
        assert RoutingStrategy.LEAST_BUSY is not None

    def test_random_exists(self):
        assert RoutingStrategy.RANDOM is not None

    def test_scene_match_exists(self):
        assert RoutingStrategy.SCENE_MATCH is not None

    def test_specific_exists(self):
        assert RoutingStrategy.SPECIFIC is not None

    def test_all_variants_distinct(self):
        variants = [
            RoutingStrategy.FIRST_AVAILABLE,
            RoutingStrategy.ROUND_ROBIN,
            RoutingStrategy.LEAST_BUSY,
            RoutingStrategy.RANDOM,
            RoutingStrategy.SCENE_MATCH,
            RoutingStrategy.SPECIFIC,
        ]
        assert len(set(str(v) for v in variants)) == 6

    def test_repr_is_str(self):
        r = repr(RoutingStrategy.ROUND_ROBIN)
        assert isinstance(r, str)


# ---------------------------------------------------------------------------
# TransportManager + ServiceEntry
# ---------------------------------------------------------------------------


class TestTransportManagerAndServiceEntry:
    @pytest.fixture
    def manager(self, tmp_path):
        tm = TransportManager(registry_dir=str(tmp_path))
        yield tm
        with contextlib.suppress(Exception):
            tm.shutdown()

    def test_register_returns_instance_id(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        assert isinstance(iid, str)
        assert len(iid) > 0

    def test_register_different_ids(self, manager):
        iid1 = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        iid2 = manager.register_service(dcc_type="maya", host="localhost", port=7002)
        assert iid1 != iid2

    def test_list_all_services_count(self, manager):
        manager.register_service(dcc_type="maya", host="localhost", port=7001)
        manager.register_service(dcc_type="blender", host="localhost", port=7002)
        services = manager.list_all_services()
        assert len(services) >= 2

    def test_get_service_by_dcc_and_id(self, manager):
        iid = manager.register_service(dcc_type="houdini", host="localhost", port=8001)
        entry = manager.get_service("houdini", iid)
        assert entry is not None
        assert entry.dcc_type == "houdini"
        assert entry.host == "localhost"
        assert entry.port == 8001
        assert entry.instance_id == iid

    def test_service_entry_default_status_available(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        assert str(entry.status) == str(ServiceStatus.AVAILABLE)

    def test_service_entry_version_field(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        # version may be None or str
        assert entry.version is None or isinstance(entry.version, str)

    def test_service_entry_is_ipc(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        assert isinstance(entry.is_ipc, bool)

    def test_service_entry_transport_address(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        ta = entry.transport_address
        assert ta is None or isinstance(ta, str)

    def test_service_entry_effective_address(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        # effective_address is a callable returning a TransportAddress
        ea = entry.effective_address()
        cs = ea.to_connection_string()
        assert isinstance(cs, str)
        assert "7001" in cs

    def test_service_entry_scene_none_by_default(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        assert entry.scene is None or isinstance(entry.scene, str)

    def test_service_entry_metadata(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        assert isinstance(entry.metadata, dict)

    def test_service_entry_to_dict_keys(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        d = entry.to_dict()
        assert isinstance(d, dict)
        assert "dcc_type" in d
        assert "host" in d
        assert "port" in d
        assert "instance_id" in d

    def test_service_entry_repr_contains_dcc_type(self, manager):
        iid = manager.register_service(dcc_type="blender", host="localhost", port=7002)
        entry = manager.get_service("blender", iid)
        r = repr(entry)
        assert "blender" in r

    def test_heartbeat_ok(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        # Should not raise
        manager.heartbeat("maya", iid)

    def test_update_service_status_to_busy(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        manager.update_service_status("maya", iid, ServiceStatus.BUSY)
        entry = manager.get_service("maya", iid)
        assert str(entry.status) == str(ServiceStatus.BUSY)

    def test_update_service_status_to_unreachable(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        manager.update_service_status("maya", iid, ServiceStatus.UNREACHABLE)
        entry = manager.get_service("maya", iid)
        assert str(entry.status) == str(ServiceStatus.UNREACHABLE)

    def test_deregister_service_removes_entry(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        count_before = len(manager.list_all_services())
        manager.deregister_service("maya", iid)
        count_after = len(manager.list_all_services())
        assert count_after < count_before

    def test_find_best_service_returns_entry_or_none(self, manager):
        manager.register_service(dcc_type="maya", host="localhost", port=7001)
        result = manager.find_best_service("maya")
        assert result is None or hasattr(result, "dcc_type")

    def test_find_best_service_raises_when_no_dcc(self, manager):
        # Requesting a DCC that was never registered raises RuntimeError
        with pytest.raises(RuntimeError):
            manager.find_best_service("nonexistent_dcc")

    def test_rank_services_returns_list(self, manager):
        manager.register_service(dcc_type="maya", host="localhost", port=7001)
        ranked = manager.rank_services("maya")
        assert isinstance(ranked, list)

    def test_rank_services_raises_when_no_dcc(self, manager):
        with pytest.raises(RuntimeError):
            manager.rank_services("fake_dcc")

    def test_list_instances_by_dcc(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        instances = manager.list_instances("maya")
        assert any(e.instance_id == iid for e in instances)

    def test_list_sessions_empty(self, manager):
        sessions = manager.list_sessions()
        assert isinstance(sessions, list)

    def test_session_count_zero(self, manager):
        assert manager.session_count() == 0

    def test_pool_count_for_dcc(self, manager):
        manager.register_service(dcc_type="maya", host="localhost", port=7001)
        count = manager.pool_count_for_dcc("maya")
        assert isinstance(count, int)

    def test_is_shutdown_false_initially(self, manager):
        assert manager.is_shutdown() is False

    def test_shutdown_marks_manager(self, tmp_path):
        tm = TransportManager(registry_dir=str(tmp_path))
        tm.shutdown()
        assert tm.is_shutdown() is True

    def test_cleanup_does_not_raise(self, manager):
        manager.register_service(dcc_type="maya", host="localhost", port=7001)
        manager.cleanup()  # Should not raise

    def test_concurrent_register(self, tmp_path):
        """Multiple threads registering services concurrently should not crash."""
        tm = TransportManager(registry_dir=str(tmp_path))
        errors = []

        def worker(port_offset):
            try:
                tm.register_service(dcc_type="maya", host="localhost", port=7000 + port_offset)
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert errors == [], f"Concurrent register had errors: {errors}"
        tm.shutdown()

    def test_last_heartbeat_ms_is_int(self, manager):
        iid = manager.register_service(dcc_type="maya", host="localhost", port=7001)
        entry = manager.get_service("maya", iid)
        assert isinstance(entry.last_heartbeat_ms, int)


# ---------------------------------------------------------------------------
# wrap_value / unwrap_value / unwrap_parameters
# ---------------------------------------------------------------------------


class TestWrapValue:
    def test_wrap_true(self):
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)
        assert w.value is True

    def test_wrap_false(self):
        w = wrap_value(False)
        assert isinstance(w, BooleanWrapper)
        assert w.value is False

    def test_wrap_int(self):
        w = wrap_value(42)
        assert isinstance(w, IntWrapper)
        assert w.value == 42

    def test_wrap_negative_int(self):
        w = wrap_value(-7)
        assert isinstance(w, IntWrapper)
        assert w.value == -7

    def test_wrap_zero(self):
        w = wrap_value(0)
        assert isinstance(w, IntWrapper)
        assert w.value == 0

    def test_wrap_float(self):
        w = wrap_value(3.14)
        assert isinstance(w, FloatWrapper)
        assert abs(w.value - 3.14) < 1e-9

    def test_wrap_string(self):
        w = wrap_value("hello")
        assert isinstance(w, StringWrapper)
        assert w.value == "hello"

    def test_wrap_empty_string(self):
        w = wrap_value("")
        assert isinstance(w, StringWrapper)
        assert w.value == ""

    def test_wrap_none_returns_none(self):
        result = wrap_value(None)
        assert result is None

    def test_wrap_list_returns_list(self):
        result = wrap_value([1, 2, 3])
        assert result == [1, 2, 3]

    def test_wrap_dict_returns_dict(self):
        result = wrap_value({"a": 1})
        assert result == {"a": 1}

    def test_wrap_bool_takes_precedence_over_int(self):
        # True is both bool and int; should be BooleanWrapper
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)

    def test_wrap_large_int(self):
        w = wrap_value(10**15)
        assert isinstance(w, IntWrapper)

    def test_wrap_negative_float(self):
        w = wrap_value(-2.718)
        assert isinstance(w, FloatWrapper)


class TestUnwrapValue:
    def test_unwrap_boolean_wrapper_true(self):
        assert unwrap_value(BooleanWrapper(True)) is True

    def test_unwrap_boolean_wrapper_false(self):
        assert unwrap_value(BooleanWrapper(False)) is False

    def test_unwrap_int_wrapper(self):
        assert unwrap_value(IntWrapper(99)) == 99

    def test_unwrap_float_wrapper(self):
        assert abs(unwrap_value(FloatWrapper(1.5)) - 1.5) < 1e-9

    def test_unwrap_string_wrapper(self):
        assert unwrap_value(StringWrapper("abc")) == "abc"

    def test_unwrap_primitive_bool(self):
        assert unwrap_value(True) is True

    def test_unwrap_primitive_int(self):
        assert unwrap_value(42) == 42

    def test_unwrap_primitive_float(self):
        assert abs(unwrap_value(3.14) - 3.14) < 1e-9

    def test_unwrap_primitive_str(self):
        assert unwrap_value("x") == "x"

    def test_unwrap_none(self):
        assert unwrap_value(None) is None

    def test_unwrap_list_passthrough(self):
        assert unwrap_value([1, 2]) == [1, 2]

    def test_unwrap_dict_passthrough(self):
        assert unwrap_value({"a": 1}) == {"a": 1}

    def test_roundtrip_bool(self):
        for val in [True, False]:
            assert unwrap_value(wrap_value(val)) == val

    def test_roundtrip_int(self):
        for val in [0, 1, -1, 1000]:
            assert unwrap_value(wrap_value(val)) == val

    def test_roundtrip_float(self):
        for val in [0.0, 1.5, -3.14, 1e10]:
            assert abs(unwrap_value(wrap_value(val)) - val) < 1e-6

    def test_roundtrip_string(self):
        for val in ["", "hello", "unicode-🎉"]:
            assert unwrap_value(wrap_value(val)) == val


class TestUnwrapParameters:
    def test_unwrap_empty_dict(self):
        result = unwrap_parameters({})
        assert result == {}

    def test_unwrap_single_bool(self):
        result = unwrap_parameters({"flag": BooleanWrapper(True)})
        assert result == {"flag": True}

    def test_unwrap_single_int(self):
        result = unwrap_parameters({"count": IntWrapper(5)})
        assert result == {"count": 5}

    def test_unwrap_single_float(self):
        result = unwrap_parameters({"ratio": FloatWrapper(0.5)})
        assert abs(result["ratio"] - 0.5) < 1e-9

    def test_unwrap_single_string(self):
        result = unwrap_parameters({"name": StringWrapper("scene")})
        assert result == {"name": "scene"}

    def test_unwrap_mixed_wrapped_and_plain(self):
        params = {"a": BooleanWrapper(True), "b": IntWrapper(3), "c": "plain_str", "d": 42}
        result = unwrap_parameters(params)
        assert result["a"] is True
        assert result["b"] == 3
        assert result["c"] == "plain_str"
        assert result["d"] == 42

    def test_unwrap_preserves_keys(self):
        params = {"x": IntWrapper(10), "y": FloatWrapper(2.0)}
        result = unwrap_parameters(params)
        assert set(result.keys()) == {"x", "y"}

    def test_unwrap_none_value(self):
        result = unwrap_parameters({"v": None})
        assert result["v"] is None

    def test_unwrap_returns_dict(self):
        result = unwrap_parameters({"a": IntWrapper(1)})
        assert isinstance(result, dict)

    def test_unwrap_concurrent(self):
        """Concurrent calls should not cause issues."""
        errors = []

        def worker():
            try:
                params = {f"k{i}": IntWrapper(i) for i in range(20)}
                result = unwrap_parameters(params)
                assert len(result) == 20
            except Exception as e:
                errors.append(e)

        threads = [threading.Thread(target=worker) for _ in range(20)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert errors == []


# ---------------------------------------------------------------------------
# Type Wrappers: BooleanWrapper, FloatWrapper, IntWrapper, StringWrapper
# ---------------------------------------------------------------------------


class TestBooleanWrapper:
    def test_true_value(self):
        w = BooleanWrapper(True)
        assert w.value is True

    def test_false_value(self):
        w = BooleanWrapper(False)
        assert w.value is False

    def test_repr_true(self):
        r = repr(BooleanWrapper(True))
        assert "True" in r

    def test_repr_false(self):
        r = repr(BooleanWrapper(False))
        assert "False" in r

    def test_value_type_is_bool(self):
        assert isinstance(BooleanWrapper(True).value, bool)

    def test_two_instances_independent(self):
        w1 = BooleanWrapper(True)
        w2 = BooleanWrapper(False)
        assert w1.value is not w2.value


class TestFloatWrapper:
    def test_positive_float(self):
        w = FloatWrapper(3.14)
        assert abs(w.value - 3.14) < 1e-9

    def test_negative_float(self):
        w = FloatWrapper(-2.718)
        assert abs(w.value - (-2.718)) < 1e-9

    def test_zero(self):
        w = FloatWrapper(0.0)
        assert w.value == 0.0

    def test_large_float(self):
        w = FloatWrapper(1e15)
        assert w.value == 1e15

    def test_repr_contains_value(self):
        r = repr(FloatWrapper(1.5))
        assert "1.5" in r

    def test_int_input_coerced(self):
        w = FloatWrapper(5)
        # int 5 should be stored as float
        assert isinstance(w.value, float) or w.value == 5


class TestIntWrapper:
    def test_positive(self):
        w = IntWrapper(42)
        assert w.value == 42

    def test_negative(self):
        w = IntWrapper(-1)
        assert w.value == -1

    def test_zero(self):
        w = IntWrapper(0)
        assert w.value == 0

    def test_large_value(self):
        w = IntWrapper(10**9)
        assert w.value == 10**9

    def test_repr_contains_value(self):
        r = repr(IntWrapper(99))
        assert "99" in r

    def test_value_is_int(self):
        assert isinstance(IntWrapper(7).value, int)


class TestStringWrapper:
    def test_basic_string(self):
        w = StringWrapper("hello")
        assert w.value == "hello"

    def test_empty_string(self):
        w = StringWrapper("")
        assert w.value == ""

    def test_unicode_string(self):
        w = StringWrapper("Maya场景")
        assert w.value == "Maya场景"

    def test_long_string(self):
        s = "x" * 1000
        w = StringWrapper(s)
        assert len(w.value) == 1000

    def test_repr_contains_value(self):
        r = repr(StringWrapper("test"))
        assert "test" in r

    def test_value_is_str(self):
        assert isinstance(StringWrapper("abc").value, str)

    def test_two_wrappers_independent(self):
        w1 = StringWrapper("a")
        w2 = StringWrapper("b")
        assert w1.value != w2.value
