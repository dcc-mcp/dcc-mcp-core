"""Tests for PySharedSceneBuffer deep API and UsdPrim.has_api behavior.

Covers PySharedSceneBuffer.write with all PySceneDataKind variants, compression toggle,
descriptor_json field structure, large data handling, and UsdPrim.has_api behavior.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── PySharedSceneBuffer — all PySceneDataKind variants ───────────────────────


class TestSharedSceneBufferKinds:
    _DATA = b"test scene data for kind variants"

    def test_geometry_kind(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        assert buf is not None
        assert buf.total_bytes == len(self._DATA)

    def test_screenshot_kind(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(
            self._DATA, dcc_mcp_core.PySceneDataKind.Screenshot, "houdini", False
        )
        assert buf is not None
        assert buf.total_bytes == len(self._DATA)

    def test_animation_cache_kind(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(
            self._DATA, dcc_mcp_core.PySceneDataKind.AnimationCache, "blender", False
        )
        assert buf is not None
        assert buf.total_bytes == len(self._DATA)

    def test_arbitrary_kind(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Arbitrary, "maya", False)
        assert buf is not None
        assert buf.total_bytes == len(self._DATA)

    def test_all_kinds_return_unique_ids(self) -> None:
        kinds = [
            dcc_mcp_core.PySceneDataKind.Geometry,
            dcc_mcp_core.PySceneDataKind.Screenshot,
            dcc_mcp_core.PySceneDataKind.AnimationCache,
            dcc_mcp_core.PySceneDataKind.Arbitrary,
        ]
        ids = [dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, k, "maya", False).id for k in kinds]
        assert len(set(ids)) == len(ids)

    def test_kind_reflected_in_descriptor_json(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Screenshot, "maya", False)
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["kind"] == "screenshot"


# ── PySharedSceneBuffer — compression toggle ─────────────────────────────────


class TestSharedSceneBufferCompression:
    _DATA = b"compressible scene data " * 100  # repetitive data compresses well

    def test_uncompressed_read_roundtrip(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        assert buf.read() == self._DATA

    def test_compressed_read_roundtrip(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "maya", True)
        assert buf.read() == self._DATA

    def test_uncompressed_is_inline(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(
            self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "blender", False
        )
        assert buf.is_inline is True

    def test_compressed_is_inline(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "blender", True)
        assert buf.is_inline is True

    def test_not_chunked_small_data(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(b"small", dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        assert buf.is_chunked is False

    def test_compressed_and_uncompressed_have_different_ids(self) -> None:
        buf1 = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        buf2 = dcc_mcp_core.PySharedSceneBuffer.write(self._DATA, dcc_mcp_core.PySceneDataKind.Geometry, "maya", True)
        assert buf1.id != buf2.id

    def test_empty_bytes_uncompressed(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(b"", dcc_mcp_core.PySceneDataKind.Arbitrary, "maya", False)
        assert buf.total_bytes == 0
        assert buf.read() == b""

    def test_empty_bytes_compressed(self) -> None:
        buf = dcc_mcp_core.PySharedSceneBuffer.write(b"", dcc_mcp_core.PySceneDataKind.Arbitrary, "maya", True)
        assert buf.read() == b""


# ── PySharedSceneBuffer — descriptor_json field structure ────────────────────


class TestSharedSceneBufferDescriptorJson:
    def _make_buf(self, dcc: str = "maya") -> dcc_mcp_core.PySharedSceneBuffer:
        return dcc_mcp_core.PySharedSceneBuffer.write(
            b"test scene payload", dcc_mcp_core.PySceneDataKind.Geometry, dcc, False
        )

    def test_descriptor_json_is_valid_json(self) -> None:
        buf = self._make_buf()
        desc_str = buf.descriptor_json()
        desc = json.loads(desc_str)
        assert isinstance(desc, dict)

    def test_descriptor_json_meta_id_matches_buf_id(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["id"] == buf.id

    def test_descriptor_json_meta_kind_geometry(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["kind"] == "geometry"

    def test_descriptor_json_meta_source_dcc(self) -> None:
        buf = self._make_buf(dcc="blender")
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["source_dcc"] == "blender"

    def test_descriptor_json_meta_total_bytes(self) -> None:
        data = b"test scene payload"
        buf = dcc_mcp_core.PySharedSceneBuffer.write(data, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        desc = json.loads(buf.descriptor_json())
        assert desc["meta"]["total_bytes"] == len(data)

    def test_descriptor_json_storage_has_capacity(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert "capacity" in desc["storage"]
        assert desc["storage"]["capacity"] >= 0

    def test_descriptor_json_storage_has_id(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert "id" in desc["storage"]
        assert len(desc["storage"]["id"]) > 0

    def test_descriptor_json_storage_has_path(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert "name" in desc["storage"]
        assert len(desc["storage"]["name"]) > 0

    def test_descriptor_json_meta_has_created_at(self) -> None:
        buf = self._make_buf()
        desc = json.loads(buf.descriptor_json())
        assert "created_at" in desc["meta"]


# ── PySharedSceneBuffer — large data ─────────────────────────────────────────


class TestSharedSceneBufferLargeData:
    def test_1mb_uncompressed_roundtrip(self) -> None:
        data = b"x" * (1024 * 1024)
        buf = dcc_mcp_core.PySharedSceneBuffer.write(data, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        assert buf.total_bytes == len(data)
        assert buf.read() == data

    def test_1mb_total_bytes_correct(self) -> None:
        data = b"y" * (1024 * 1024)
        buf = dcc_mcp_core.PySharedSceneBuffer.write(data, dcc_mcp_core.PySceneDataKind.AnimationCache, "maya", False)
        assert buf.total_bytes == 1024 * 1024

    def test_different_source_dccs_independent(self) -> None:
        data = b"scene data payload"
        buf_maya = dcc_mcp_core.PySharedSceneBuffer.write(data, dcc_mcp_core.PySceneDataKind.Geometry, "maya", False)
        buf_blender = dcc_mcp_core.PySharedSceneBuffer.write(
            data, dcc_mcp_core.PySceneDataKind.Geometry, "blender", False
        )
        assert buf_maya.id != buf_blender.id
        assert buf_maya.read() == buf_blender.read()


# ── UsdPrim.has_api behavior ──────────────────────────────────────────────────


class TestUsdPrimHasApi:
    def _make_prim(self, prim_type: str = "Mesh") -> dcc_mcp_core.UsdPrim:
        stage = dcc_mcp_core.UsdStage("has_api_test")
        stage.define_prim("/TestPrim", prim_type)
        prim = stage.get_prim("/TestPrim")
        assert prim is not None
        return prim

    def test_has_api_returns_bool(self) -> None:
        prim = self._make_prim()
        result = prim.has_api("SomeAPI")
        assert isinstance(result, bool)

    def test_has_api_false_for_nonexistent_schema(self) -> None:
        prim = self._make_prim()
        assert prim.has_api("NonexistentFooBarAPI") is False

    def test_has_api_false_for_geom_model_api_on_mesh(self) -> None:
        prim = self._make_prim("Mesh")
        assert prim.has_api("GeomModelAPI") is False

    def test_has_api_false_for_physics_api(self) -> None:
        prim = self._make_prim("Xform")
        assert prim.has_api("PhysicsRigidBodyAPI") is False

    def test_has_api_empty_string(self) -> None:
        prim = self._make_prim()
        result = prim.has_api("")
        assert isinstance(result, bool)

    def test_has_api_multiple_calls_consistent(self) -> None:
        prim = self._make_prim()
        r1 = prim.has_api("SkelBindingAPI")
        r2 = prim.has_api("SkelBindingAPI")
        assert r1 == r2

    def test_has_api_different_schemas_all_false(self) -> None:
        prim = self._make_prim("Xform")
        schemas = ["GeomModelAPI", "PhysicsRigidBodyAPI", "SkelBindingAPI", "MaterialBindingAPI"]
        for schema in schemas:
            assert prim.has_api(schema) is False

    def test_has_api_on_scope_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("scope_test")
        stage.define_prim("/World", "Scope")
        prim = stage.get_prim("/World")
        assert prim.has_api("AnyAPI") is False

    def test_usd_prim_attributes_names_nonempty_after_set(self) -> None:
        stage = dcc_mcp_core.UsdStage("attr_test")
        stage.define_prim("/Cube", "Mesh")
        stage.set_attribute("/Cube", "radius", dcc_mcp_core.VtValue.from_float(1.0))
        prim = stage.get_prim("/Cube")
        names = prim.attribute_names()
        assert "radius" in names

    def test_usd_prim_attribute_names_empty_on_new_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("empty_attr_test")
        stage.define_prim("/Empty", "Xform")
        prim = stage.get_prim("/Empty")
        names = prim.attribute_names()
        assert isinstance(names, list)
