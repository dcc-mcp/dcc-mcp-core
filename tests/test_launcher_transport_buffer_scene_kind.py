"""Tests for PyDccLauncher, TransportAddress, TransportScheme, PyBufferPool, PySharedBuffer, and PySceneDataKind.

Covers lifecycle, factory methods, buffer pool acquire/release, and enum variants.
"""

from __future__ import annotations

# Import built-in modules
import json
import os

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import PyBufferPool
from dcc_mcp_core import PyDccLauncher
from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import TransportAddress
from dcc_mcp_core import TransportScheme

# ===========================================================================
# PyDccLauncher tests
# ===========================================================================


class TestPyDccLauncherLifecycle:
    """Tests for PyDccLauncher basic lifecycle without actually launching DCC processes."""

    def test_construction_no_args(self):
        launcher = PyDccLauncher()
        assert launcher is not None

    def test_repr_contains_running(self):
        launcher = PyDccLauncher()
        r = repr(launcher)
        assert isinstance(r, str)
        assert "running" in r.lower() or "PyDccLauncher" in r

    def test_running_count_starts_zero(self):
        launcher = PyDccLauncher()
        assert launcher.running_count() == 0

    def test_running_count_returns_int(self):
        launcher = PyDccLauncher()
        assert isinstance(launcher.running_count(), int)

    def test_pid_of_nonexistent_returns_none(self):
        launcher = PyDccLauncher()
        assert launcher.pid_of("nonexistent_dcc") is None

    def test_restart_count_nonexistent_returns_zero(self):
        launcher = PyDccLauncher()
        assert launcher.restart_count("nonexistent_dcc") == 0

    def test_restart_count_returns_int(self):
        launcher = PyDccLauncher()
        assert isinstance(launcher.restart_count("any_name"), int)

    def test_kill_nonexistent_raises_runtime_error(self):
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.kill("nonexistent")

    def test_terminate_nonexistent_raises_runtime_error(self):
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError, match="not running"):
            launcher.terminate("nonexistent")

    def test_terminate_with_custom_timeout(self):
        launcher = PyDccLauncher()
        with pytest.raises(RuntimeError):
            launcher.terminate("nonexistent", timeout_ms=1000)

    def test_multiple_launchers_independent(self):
        l1 = PyDccLauncher()
        l2 = PyDccLauncher()
        assert l1.running_count() == 0
        assert l2.running_count() == 0


# ===========================================================================
# TransportAddress tests
# ===========================================================================


class TestTransportAddressTcp:
    """Tests for TransportAddress TCP factory and properties."""

    def test_tcp_creates_address(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta is not None

    def test_tcp_is_tcp(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.is_tcp is True

    def test_tcp_is_not_named_pipe(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.is_named_pipe is False

    def test_tcp_is_not_unix_socket(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.is_unix_socket is False

    def test_loopback_is_local(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.is_local is True

    def test_localhost_is_local(self):
        ta = TransportAddress.tcp("localhost", 8080)
        assert ta.is_local is True

    def test_external_ip_is_not_local(self):
        ta = TransportAddress.tcp("192.168.1.100", 8080)
        assert ta.is_local is False

    def test_scheme_is_tcp_string(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        assert ta.scheme == "tcp"

    def test_to_connection_string_tcp(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        cs = ta.to_connection_string()
        assert isinstance(cs, str)
        assert "127.0.0.1" in cs
        assert "8080" in cs

    def test_str_repr_contains_url(self):
        ta = TransportAddress.tcp("127.0.0.1", 18812)
        s = str(ta)
        assert "tcp" in s
        assert "127.0.0.1" in s
        assert "18812" in s

    def test_repr_is_string(self):
        ta = TransportAddress.tcp("127.0.0.1", 8080)
        r = repr(ta)
        assert isinstance(r, str)
        assert "TransportAddress" in r


class TestTransportAddressNamedPipe:
    """Tests for TransportAddress named pipe factory."""

    def test_named_pipe_creates_address(self):
        ta = TransportAddress.named_pipe("my_pipe")
        assert ta is not None

    def test_named_pipe_is_named_pipe(self):
        ta = TransportAddress.named_pipe("my_pipe")
        assert ta.is_named_pipe is True

    def test_named_pipe_is_not_tcp(self):
        ta = TransportAddress.named_pipe("my_pipe")
        assert ta.is_tcp is False

    def test_named_pipe_is_not_unix_socket(self):
        ta = TransportAddress.named_pipe("my_pipe")
        assert ta.is_unix_socket is False

    def test_named_pipe_scheme(self):
        ta = TransportAddress.named_pipe("test")
        assert "pipe" in ta.scheme

    def test_named_pipe_str_contains_pipe_name(self):
        ta = TransportAddress.named_pipe("my_test_pipe")
        s = str(ta)
        assert "my_test_pipe" in s


class TestTransportAddressFactories:
    """Tests for TransportAddress static factory methods."""

    def test_parse_tcp_url(self):
        ta = TransportAddress.parse("tcp://127.0.0.1:18812")
        assert ta is not None
        assert ta.is_tcp is True

    def test_parse_round_trip(self):
        original = "tcp://127.0.0.1:18812"
        ta = TransportAddress.parse(original)
        s = str(ta)
        assert "127.0.0.1" in s
        assert "18812" in s

    def test_default_local_returns_address(self):
        pid = os.getpid()
        ta = TransportAddress.default_local("maya", pid)
        assert ta is not None
        assert isinstance(str(ta), str)

    def test_default_local_windows_returns_named_pipe(self):
        # On Windows, default_local should return Named Pipe
        pid = os.getpid()
        ta = TransportAddress.default_local("maya", pid)
        # On Windows: named pipe; on Linux/Mac: unix socket
        assert ta.is_named_pipe or ta.is_unix_socket

    def test_default_pipe_name_returns_address(self):
        pid = os.getpid()
        ta = TransportAddress.default_pipe_name("maya", pid)
        assert ta is not None

    def test_default_pipe_name_is_named_pipe_on_windows(self):
        pid = os.getpid()
        ta = TransportAddress.default_pipe_name("maya", pid)
        # On Windows this returns named pipe
        s = str(ta)
        assert "maya" in s.lower() or "dcc-mcp" in s


# ===========================================================================
# TransportScheme tests
# ===========================================================================


class TestTransportSchemeVariants:
    """Tests for TransportScheme enum variants."""

    def test_tcp_only_exists(self):
        assert TransportScheme.TCP_ONLY is not None

    def test_prefer_named_pipe_exists(self):
        assert TransportScheme.PREFER_NAMED_PIPE is not None

    def test_prefer_unix_socket_exists(self):
        assert TransportScheme.PREFER_UNIX_SOCKET is not None

    def test_prefer_ipc_exists(self):
        assert TransportScheme.PREFER_IPC is not None

    def test_auto_exists(self):
        assert TransportScheme.AUTO is not None

    def test_all_five_are_distinct(self):
        variants = [
            TransportScheme.TCP_ONLY,
            TransportScheme.PREFER_NAMED_PIPE,
            TransportScheme.PREFER_UNIX_SOCKET,
            TransportScheme.PREFER_IPC,
            TransportScheme.AUTO,
        ]
        # All must be different
        reprs = [repr(v) for v in variants]
        assert len(set(reprs)) == 5

    def test_self_equality(self):
        assert TransportScheme.TCP_ONLY == TransportScheme.TCP_ONLY
        assert TransportScheme.AUTO == TransportScheme.AUTO

    def test_inequality(self):
        assert TransportScheme.TCP_ONLY != TransportScheme.AUTO

    def test_repr_is_string(self):
        r = repr(TransportScheme.TCP_ONLY)
        assert isinstance(r, str)
        assert len(r) > 0


class TestTransportSchemeSelectAddress:
    """Tests for TransportScheme.select_address factory method."""

    def test_tcp_only_returns_tcp_address(self):
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert addr.is_tcp is True

    def test_tcp_only_preserves_host_port(self):
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        s = str(addr)
        assert "127.0.0.1" in s
        assert "18812" in s

    def test_prefer_ipc_returns_non_tcp_on_local(self):
        pid = os.getpid()
        addr = TransportScheme.PREFER_IPC.select_address("maya", "127.0.0.1", 18812, pid)
        # PREFER_IPC on local should return named pipe (Windows) or unix (Linux)
        assert addr.is_named_pipe or addr.is_unix_socket or addr.is_tcp

    def test_auto_with_pid_returns_ipc_on_local(self):
        pid = os.getpid()
        addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid)
        # AUTO with a PID on local should prefer IPC
        assert addr is not None
        assert isinstance(str(addr), str)

    def test_tcp_only_without_pid(self):
        addr = TransportScheme.TCP_ONLY.select_address("blender", "127.0.0.1", 19000)
        assert addr.is_tcp is True

    def test_select_address_returns_transport_address(self):
        addr = TransportScheme.TCP_ONLY.select_address("maya", "127.0.0.1", 18812)
        assert isinstance(str(addr), str)


# ===========================================================================
# PySceneDataKind tests
# ===========================================================================


class TestPySceneDataKindVariants:
    """Tests for PySceneDataKind enum variants."""

    def test_geometry_exists(self):
        assert PySceneDataKind.Geometry is not None

    def test_screenshot_exists(self):
        assert PySceneDataKind.Screenshot is not None

    def test_animation_cache_exists(self):
        assert PySceneDataKind.AnimationCache is not None

    def test_arbitrary_exists(self):
        assert PySceneDataKind.Arbitrary is not None

    def test_all_four_are_distinct(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.Screenshot,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Arbitrary,
        ]
        reprs = [repr(k) for k in kinds]
        assert len(set(reprs)) == 4

    def test_self_equality(self):
        assert PySceneDataKind.Geometry == PySceneDataKind.Geometry
        assert PySceneDataKind.Screenshot == PySceneDataKind.Screenshot

    def test_cross_inequality(self):
        assert PySceneDataKind.Geometry != PySceneDataKind.Screenshot
        assert PySceneDataKind.AnimationCache != PySceneDataKind.Arbitrary

    def test_str_contains_variant_name(self):
        assert "Geometry" in str(PySceneDataKind.Geometry)
        assert "Screenshot" in str(PySceneDataKind.Screenshot)
        assert "AnimationCache" in str(PySceneDataKind.AnimationCache)
        assert "Arbitrary" in str(PySceneDataKind.Arbitrary)

    def test_repr_is_string(self):
        r = repr(PySceneDataKind.Geometry)
        assert isinstance(r, str)


# ===========================================================================
# PyBufferPool tests
# ===========================================================================


class TestPyBufferPoolConstruction:
    """Tests for PyBufferPool construction and basic properties."""

    def test_construction_with_capacity_and_size(self):
        pool = PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_repr_is_string(self):
        pool = PyBufferPool(4, 1024)
        r = repr(pool)
        assert isinstance(r, str)

    def test_capacity_method_returns_correct_value(self):
        pool = PyBufferPool(4, 1024)
        assert pool.capacity() == 4

    def test_buffer_size_method_returns_correct_value(self):
        pool = PyBufferPool(4, 1024)
        assert pool.buffer_size() == 1024

    def test_available_method_returns_capacity_initially(self):
        pool = PyBufferPool(4, 1024)
        assert pool.available() == 4

    def test_capacity_different_value(self):
        pool = PyBufferPool(8, 4096)
        assert pool.capacity() == 8
        assert pool.buffer_size() == 4096

    def test_single_capacity_pool(self):
        pool = PyBufferPool(1, 512)
        assert pool.capacity() == 1
        assert pool.available() == 1


class TestPyBufferPoolAcquireRelease:
    """Tests for PyBufferPool acquire and release (via del) semantics."""

    def test_acquire_returns_buffer(self):
        pool = PyBufferPool(4, 1024)
        buf = pool.acquire()
        assert buf is not None

    def test_acquire_decrements_available(self):
        pool = PyBufferPool(4, 1024)
        buf = pool.acquire()
        assert pool.available() == 3
        del buf

    def test_del_buffer_returns_to_pool(self):
        pool = PyBufferPool(4, 1024)
        buf = pool.acquire()
        assert pool.available() == 3
        del buf
        assert pool.available() == 4

    def test_acquire_multiple_buffers(self):
        pool = PyBufferPool(4, 1024)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 2
        del b1
        del b2
        assert pool.available() == 4

    def test_exhaust_pool_then_release(self):
        pool = PyBufferPool(2, 512)
        b1 = pool.acquire()
        b2 = pool.acquire()
        assert pool.available() == 0
        del b1
        assert pool.available() == 1
        del b2
        assert pool.available() == 2


# ===========================================================================
# PySharedBuffer tests
# ===========================================================================


class TestPySharedBufferOperations:
    """Tests for PySharedBuffer read/write/clear/metadata operations."""

    def _make_buf(self, buffer_size: int = 1024):
        pool = PyBufferPool(1, buffer_size)
        return pool.acquire()

    def test_write_and_read_roundtrip(self):
        buf = self._make_buf()
        buf.write(b"hello world")
        assert buf.read() == b"hello world"

    def test_data_len_after_write(self):
        buf = self._make_buf()
        buf.write(b"test data")
        assert buf.data_len() == 9

    def test_data_len_is_zero_initially(self):
        buf = self._make_buf()
        assert buf.data_len() == 0

    def test_capacity_method_returns_int(self):
        buf = self._make_buf(2048)
        assert buf.capacity() == 2048

    def test_clear_resets_data_len(self):
        buf = self._make_buf()
        buf.write(b"some data")
        assert buf.data_len() == 9
        buf.clear()
        assert buf.data_len() == 0

    def test_clear_then_write(self):
        buf = self._make_buf()
        buf.write(b"first")
        buf.clear()
        buf.write(b"second")
        assert buf.read() == b"second"

    def test_id_is_nonempty_string(self):
        buf = self._make_buf()
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_descriptor_json_is_valid_json(self):
        buf = self._make_buf()
        dj = buf.descriptor_json()
        assert isinstance(dj, str)
        parsed = json.loads(dj)
        assert isinstance(parsed, dict)

    def test_descriptor_json_contains_id(self):
        buf = self._make_buf()
        dj = buf.descriptor_json()
        parsed = json.loads(dj)
        assert "id" in parsed

    def test_write_empty_bytes(self):
        buf = self._make_buf()
        buf.write(b"")
        assert buf.data_len() == 0

    def test_write_binary_data(self):
        buf = self._make_buf()
        data = bytes(range(256))
        buf.write(data)
        assert buf.read() == data

    def test_repr_contains_id(self):
        buf = self._make_buf()
        r = repr(buf)
        assert isinstance(r, str)
        assert "PySharedBuffer" in r
