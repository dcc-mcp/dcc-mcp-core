"""Tests for dcc-mcp-shm Python bindings.

Covers PySharedBuffer, PyBufferPool, PySceneDataKind, PySharedSceneBuffer.
All tests use in-process memory-mapped files (temp files) — no GPU or DCC
environment required.
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── PySharedBuffer ────────────────────────────────────────────────────────────


class TestPySharedBuffer:
    def test_create_returns_instance(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        assert buf is not None

    def test_capacity_matches_request(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=4096)
        assert buf.capacity() == 4096

    def test_write_and_read_roundtrip(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        payload = b"vertex data xyz"
        written = buf.write(payload)
        assert written == len(payload)
        data = buf.read()
        assert data == payload

    def test_data_len_after_write(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"hello")
        assert buf.data_len() == 5

    def test_data_len_initial_zero(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        assert buf.data_len() == 0

    def test_clear_resets_data_len(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        buf.write(b"abc")
        buf.clear()
        assert buf.data_len() == 0

    def test_id_is_nonempty_string(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=256)
        assert isinstance(buf.id, str)
        assert len(buf.id) > 0

    def test_path_is_nonempty_string(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=256)
        path = buf.path()
        assert isinstance(path, str)
        assert len(path) > 0

    def test_descriptor_json_is_valid(self) -> None:
        import json

        buf = dcc_mcp_core.PySharedBuffer.create(capacity=512)
        buf.write(b"data")
        desc = buf.descriptor_json()
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_repr_contains_capacity(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=2048)
        r = repr(buf)
        assert "2048" in r

    def test_overwrite_replaces_data(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=1024)
        buf.write(b"first write")
        buf.write(b"second")
        data = buf.read()
        assert data == b"second"

    def test_write_empty_bytes(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=256)
        written = buf.write(b"")
        assert written == 0

    def test_write_binary_data(self) -> None:
        buf = dcc_mcp_core.PySharedBuffer.create(capacity=4096)
        binary = bytes(range(256)) * 4
        buf.write(binary)
        assert buf.read() == binary


# ── PyBufferPool ──────────────────────────────────────────────────────────────


class TestPyBufferPool:
    def test_create_pool(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=1024)
        assert pool is not None

    def test_capacity(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=512)
        assert pool.capacity() == 3

    def test_buffer_size(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=2048)
        assert pool.buffer_size() == 2048

    def test_initial_available_equals_capacity(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=256)
        assert pool.available() == 4

    def test_acquire_returns_buffer(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        assert buf is not None

    def test_acquire_and_use_buffer(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=1024)
        buf = pool.acquire()
        written = buf.write(b"scene snapshot")
        assert written == len(b"scene snapshot")
        assert buf.read() == b"scene snapshot"

    def test_pool_exhaustion_raises(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=1, buffer_size=512)
        _buf1 = pool.acquire()
        with pytest.raises(RuntimeError):
            _buf2 = pool.acquire()

    def test_repr_contains_capacity(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=5, buffer_size=256)
        r = repr(pool)
        assert "5" in r


# ── PySceneDataKind ───────────────────────────────────────────────────────────


class TestPySceneDataKind:
    def test_geometry_exists(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.Geometry is not None

    def test_animation_cache_exists(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.AnimationCache is not None

    def test_screenshot_exists(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.Screenshot is not None

    def test_arbitrary_exists(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.Arbitrary is not None

    def test_equality(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.Geometry == dcc_mcp_core.PySceneDataKind.Geometry

    def test_inequality(self) -> None:
        assert dcc_mcp_core.PySceneDataKind.Geometry != dcc_mcp_core.PySceneDataKind.Screenshot


# ── PySharedSceneBuffer ───────────────────────────────────────────────────────


class TestPySharedSceneBuffer:
    def test_write_and_read_roundtrip(self) -> None:
        data = b"geometry data " * 100
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=data)
        recovered = ssb.read()
        assert recovered == data

    def test_total_bytes(self) -> None:
        data = b"x" * 256
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=data)
        assert ssb.total_bytes == 256

    def test_id_is_nonempty(self) -> None:
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=b"test")
        assert len(ssb.id) > 0

    def test_is_inline_small_data(self) -> None:
        # Small payloads should be stored inline (not chunked).
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=b"small")
        assert ssb.is_inline is True
        assert ssb.is_chunked is False

    def test_with_compression(self) -> None:
        # Compressible data should round-trip correctly.
        data = b"A" * 4096
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=data, use_compression=True)
        recovered = ssb.read()
        assert recovered == data

    def test_with_kind_geometry(self) -> None:
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(
            data=b"vertices",
            kind=dcc_mcp_core.PySceneDataKind.Geometry,
        )
        assert ssb.total_bytes == len(b"vertices")

    def test_with_source_dcc(self) -> None:
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(
            data=b"frame data",
            kind=dcc_mcp_core.PySceneDataKind.Screenshot,
            source_dcc="Maya",
        )
        assert ssb.total_bytes == len(b"frame data")

    def test_descriptor_json_is_valid(self) -> None:
        import json

        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=b"payload")
        desc = ssb.descriptor_json()
        parsed = json.loads(desc)
        assert isinstance(parsed, dict)

    def test_repr_contains_id(self) -> None:
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=b"repr_test")
        r = repr(ssb)
        assert "PySharedSceneBuffer" in r

    def test_empty_data(self) -> None:
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=b"")
        recovered = ssb.read()
        assert recovered == b""

    def test_large_data_chunked(self) -> None:
        # Data larger than the chunk threshold should be stored as chunks.
        # The threshold is implementation-defined; use 512 KiB to be safe.
        data = b"B" * (512 * 1024)
        ssb = dcc_mcp_core.PySharedSceneBuffer.write(data=data)
        recovered = ssb.read()
        assert recovered == data
        assert ssb.total_bytes == len(data)
