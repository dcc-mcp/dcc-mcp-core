"""Tests for PySharedSceneBuffer deep coverage.

Covers: write/read roundtrip, all PySceneDataKind values, source_dcc,
use_compression, descriptor_json structure, id/is_inline/is_chunked/total_bytes.
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import PySceneDataKind
from dcc_mcp_core import PySharedSceneBuffer


class TestPySharedSceneBufferWrite:
    """Tests for PySharedSceneBuffer.write basic behavior."""

    def test_write_returns_shared_scene_buffer(self):
        buf = PySharedSceneBuffer.write(b"hello")
        assert isinstance(buf, PySharedSceneBuffer)

    def test_write_empty_bytes(self):
        buf = PySharedSceneBuffer.write(b"")
        assert isinstance(buf, PySharedSceneBuffer)

    def test_write_small_data(self):
        data = b"small data"
        buf = PySharedSceneBuffer.write(data)
        assert buf is not None

    def test_write_large_data(self):
        data = b"x" * 100_000
        buf = PySharedSceneBuffer.write(data)
        assert buf is not None


class TestPySharedSceneBufferRead:
    """Tests for PySharedSceneBuffer.read roundtrip."""

    def test_read_returns_original_bytes(self):
        data = b"hello world"
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_read_empty_bytes(self):
        buf = PySharedSceneBuffer.write(b"")
        assert buf.read() == b""

    def test_read_binary_data(self):
        data = bytes(range(256))
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_read_large_data(self):
        data = b"y" * 50_000
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_read_first_call_returns_data(self):
        """read() returns original data on first call."""
        data = b"consistent"
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data

    def test_read_unicode_encoded(self):
        data = "こんにちは".encode()
        buf = PySharedSceneBuffer.write(data)
        assert buf.read() == data


class TestPySharedSceneBufferProperties:
    """Tests for PySharedSceneBuffer id/is_inline/is_chunked/total_bytes."""

    def test_id_is_string(self):
        buf = PySharedSceneBuffer.write(b"test")
        assert isinstance(buf.id, str)

    def test_id_is_nonempty(self):
        buf = PySharedSceneBuffer.write(b"test")
        assert len(buf.id) > 0

    def test_id_is_uuid_format(self):
        buf = PySharedSceneBuffer.write(b"test")
        parts = buf.id.split("-")
        assert len(parts) == 5

    def test_two_writes_have_different_ids(self):
        buf1 = PySharedSceneBuffer.write(b"data1")
        buf2 = PySharedSceneBuffer.write(b"data2")
        assert buf1.id != buf2.id

    def test_is_inline_small_data(self):
        buf = PySharedSceneBuffer.write(b"small")
        assert buf.is_inline is True

    def test_is_chunked_small_data(self):
        buf = PySharedSceneBuffer.write(b"small")
        assert buf.is_chunked is False

    def test_total_bytes_matches_data_length(self):
        data = b"hello world"
        buf = PySharedSceneBuffer.write(data)
        assert buf.total_bytes == len(data)

    def test_total_bytes_empty(self):
        buf = PySharedSceneBuffer.write(b"")
        assert buf.total_bytes == 0

    def test_total_bytes_large(self):
        data = b"z" * 1000
        buf = PySharedSceneBuffer.write(data)
        assert buf.total_bytes == 1000


class TestPySharedSceneBufferKinds:
    """Tests for PySharedSceneBuffer with different PySceneDataKind values."""

    def test_write_geometry_kind(self):
        buf = PySharedSceneBuffer.write(b"geo data", kind=PySceneDataKind.Geometry)
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["kind"] == "geometry"

    def test_write_screenshot_kind(self):
        buf = PySharedSceneBuffer.write(b"screenshot bytes", kind=PySceneDataKind.Screenshot)
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["kind"] == "screenshot"

    def test_write_animation_cache_kind(self):
        buf = PySharedSceneBuffer.write(b"anim data", kind=PySceneDataKind.AnimationCache)
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["kind"] == "animation_cache"

    def test_write_arbitrary_kind(self):
        buf = PySharedSceneBuffer.write(b"arbitrary data", kind=PySceneDataKind.Arbitrary)
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["kind"] == "arbitrary"

    def test_default_kind_is_arbitrary(self):
        buf = PySharedSceneBuffer.write(b"default")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["kind"] == "arbitrary"

    def test_all_kinds_read_correctly(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.Screenshot,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Arbitrary,
        ]
        data = b"test data"
        for kind in kinds:
            buf = PySharedSceneBuffer.write(data, kind=kind)
            assert buf.read() == data


class TestPySharedSceneBufferSourceDcc:
    """Tests for PySharedSceneBuffer source_dcc parameter."""

    def test_source_dcc_maya(self):
        buf = PySharedSceneBuffer.write(b"data", source_dcc="maya")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["source_dcc"] == "maya"

    def test_source_dcc_blender(self):
        buf = PySharedSceneBuffer.write(b"data", source_dcc="blender")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["source_dcc"] == "blender"

    def test_source_dcc_houdini(self):
        buf = PySharedSceneBuffer.write(b"data", source_dcc="houdini")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["source_dcc"] == "houdini"

    def test_source_dcc_none_by_default(self):
        buf = PySharedSceneBuffer.write(b"data")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["source_dcc"] is None

    def test_source_dcc_does_not_affect_read(self):
        data = b"scene data"
        buf = PySharedSceneBuffer.write(data, source_dcc="maya")
        assert buf.read() == data


class TestPySharedSceneBufferCompression:
    """Tests for PySharedSceneBuffer use_compression parameter."""

    def test_compressed_read_returns_original(self):
        data = b"x" * 10000
        buf = PySharedSceneBuffer.write(data, use_compression=True)
        assert buf.read() == data

    def test_compressed_small_data_read_ok(self):
        data = b"small"
        buf = PySharedSceneBuffer.write(data, use_compression=True)
        assert buf.read() == data

    def test_compressed_total_bytes(self):
        data = b"y" * 1000
        buf = PySharedSceneBuffer.write(data, use_compression=True)
        assert buf.total_bytes == len(data)

    def test_compressed_id_is_string(self):
        buf = PySharedSceneBuffer.write(b"data", use_compression=True)
        assert isinstance(buf.id, str)

    def test_uncompressed_and_compressed_have_different_ids(self):
        data = b"same data"
        buf1 = PySharedSceneBuffer.write(data, use_compression=False)
        buf2 = PySharedSceneBuffer.write(data, use_compression=True)
        assert buf1.id != buf2.id

    def test_compressed_binary_data(self):
        data = bytes(range(256)) * 100
        buf = PySharedSceneBuffer.write(data, use_compression=True)
        assert buf.read() == data


class TestPySharedSceneBufferDescriptorJson:
    """Tests for PySharedSceneBuffer.descriptor_json structure."""

    def test_descriptor_json_returns_string(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = buf.descriptor_json()
        assert isinstance(dj, str)

    def test_descriptor_json_is_valid_json(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert isinstance(dj, dict)

    def test_descriptor_has_meta_key(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "meta" in dj

    def test_descriptor_has_storage_key(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "storage" in dj

    def test_meta_has_id(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "id" in dj["meta"]

    def test_meta_id_matches_buf_id(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert dj["meta"]["id"] == buf.id

    def test_meta_has_kind(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "kind" in dj["meta"]

    def test_meta_has_total_bytes(self):
        buf = PySharedSceneBuffer.write(b"test data")
        dj = json.loads(buf.descriptor_json())
        assert "total_bytes" in dj["meta"]
        assert dj["meta"]["total_bytes"] == len(b"test data")

    def test_meta_has_source_dcc(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "source_dcc" in dj["meta"]

    def test_meta_has_created_at(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "created_at" in dj["meta"]

    def test_storage_has_id(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "id" in dj["storage"]

    def test_storage_has_path(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "name" in dj["storage"]

    def test_storage_has_capacity(self):
        buf = PySharedSceneBuffer.write(b"test")
        dj = json.loads(buf.descriptor_json())
        assert "capacity" in dj["storage"]

    def test_different_kinds_descriptor_different_meta_kind(self):
        buf1 = PySharedSceneBuffer.write(b"geo", kind=PySceneDataKind.Geometry)
        buf2 = PySharedSceneBuffer.write(b"screenshot", kind=PySceneDataKind.Screenshot)
        dj1 = json.loads(buf1.descriptor_json())
        dj2 = json.loads(buf2.descriptor_json())
        assert dj1["meta"]["kind"] != dj2["meta"]["kind"]

    def test_descriptor_kind_and_enum_consistent(self):
        kind_map = [
            (PySceneDataKind.Geometry, "geometry"),
            (PySceneDataKind.Screenshot, "screenshot"),
            (PySceneDataKind.AnimationCache, "animation_cache"),
            (PySceneDataKind.Arbitrary, "arbitrary"),
        ]
        for kind, expected_str in kind_map:
            buf = PySharedSceneBuffer.write(b"d", kind=kind)
            dj = json.loads(buf.descriptor_json())
            assert dj["meta"]["kind"] == expected_str


class TestPySceneDataKindEnum:
    """Tests for PySceneDataKind enum values."""

    def test_geometry_exists(self):
        assert hasattr(PySceneDataKind, "Geometry")

    def test_screenshot_exists(self):
        assert hasattr(PySceneDataKind, "Screenshot")

    def test_animation_cache_exists(self):
        assert hasattr(PySceneDataKind, "AnimationCache")

    def test_arbitrary_exists(self):
        assert hasattr(PySceneDataKind, "Arbitrary")

    def test_all_kinds_distinct(self):
        kinds = [
            PySceneDataKind.Geometry,
            PySceneDataKind.Screenshot,
            PySceneDataKind.AnimationCache,
            PySceneDataKind.Arbitrary,
        ]
        for i, a in enumerate(kinds):
            for j, b in enumerate(kinds):
                if i != j:
                    assert a != b
