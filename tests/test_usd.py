"""Tests for dcc-mcp-usd Python bindings.

Covers SdfPath, VtValue, UsdPrim, UsdStage, and bridge functions
(stage_to_scene_info_json, scene_info_json_to_stage, units_to_mpu, mpu_to_units).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── SdfPath ───────────────────────────────────────────────────────────────────


class TestSdfPath:
    def test_create_absolute_path(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/Cube")
        assert str(p) == "/World/Cube"

    def test_is_absolute(self) -> None:
        p = dcc_mcp_core.SdfPath("/World")
        assert p.is_absolute is True

    def test_name(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/Cube")
        assert p.name == "Cube"

    def test_parent(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/Cube")
        parent = p.parent()
        assert parent is not None
        assert str(parent) == "/World"

    def test_root_parent_is_none(self) -> None:
        p = dcc_mcp_core.SdfPath("/")
        assert p.parent() is None

    def test_child(self) -> None:
        p = dcc_mcp_core.SdfPath("/World")
        child = p.child("Cube")
        assert str(child) == "/World/Cube"

    def test_repr(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/Cube")
        assert "SdfPath" in repr(p)
        assert "/World/Cube" in repr(p)

    def test_equality(self) -> None:
        p1 = dcc_mcp_core.SdfPath("/World/A")
        p2 = dcc_mcp_core.SdfPath("/World/A")
        assert p1 == p2

    def test_inequality(self) -> None:
        p1 = dcc_mcp_core.SdfPath("/World/A")
        p2 = dcc_mcp_core.SdfPath("/World/B")
        assert p1 != p2

    def test_hash_consistent(self) -> None:
        p1 = dcc_mcp_core.SdfPath("/World/Cube")
        p2 = dcc_mcp_core.SdfPath("/World/Cube")
        assert hash(p1) == hash(p2)

    def test_can_use_as_dict_key(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/Cube")
        d = {p: "cube"}
        assert d[p] == "cube"

    def test_invalid_path_raises(self) -> None:
        with pytest.raises((ValueError, RuntimeError)):
            dcc_mcp_core.SdfPath("")


# ── VtValue ───────────────────────────────────────────────────────────────────


class TestVtValue:
    def test_from_bool_true(self) -> None:
        v = dcc_mcp_core.VtValue.from_bool(True)
        assert v.type_name == "bool"

    def test_from_bool_false(self) -> None:
        v = dcc_mcp_core.VtValue.from_bool(False)
        assert v.type_name == "bool"

    def test_from_int(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(42)
        assert v.type_name == "int"

    def test_from_float(self) -> None:
        v = dcc_mcp_core.VtValue.from_float(3.14)
        assert v.type_name == "float"

    def test_from_string(self) -> None:
        v = dcc_mcp_core.VtValue.from_string("hello")
        assert v.type_name == "string"

    def test_from_token(self) -> None:
        v = dcc_mcp_core.VtValue.from_token("catmullClark")
        assert v.type_name == "token"

    def test_from_asset(self) -> None:
        v = dcc_mcp_core.VtValue.from_asset("./textures/diffuse.png")
        assert v.type_name == "asset"

    def test_from_vec3f(self) -> None:
        v = dcc_mcp_core.VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v.type_name == "float3"

    def test_repr_contains_type_name(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(7)
        assert "int" in repr(v)

    def test_to_python_bool(self) -> None:
        v = dcc_mcp_core.VtValue.from_bool(True)
        assert v.to_python() is True

    def test_to_python_int(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(99)
        assert v.to_python() == 99

    def test_to_python_float(self) -> None:
        v = dcc_mcp_core.VtValue.from_float(1.5)
        assert abs(v.to_python() - 1.5) < 1e-4

    def test_to_python_string(self) -> None:
        v = dcc_mcp_core.VtValue.from_string("hello")
        assert v.to_python() == "hello"

    def test_to_python_vec3f_is_tuple(self) -> None:
        v = dcc_mcp_core.VtValue.from_vec3f(1.0, 2.0, 3.0)
        result = v.to_python()
        assert isinstance(result, tuple)
        assert len(result) == 3


# ── UsdStage ──────────────────────────────────────────────────────────────────


class TestUsdStage:
    def test_create_stage(self) -> None:
        stage = dcc_mcp_core.UsdStage("my_scene")
        assert stage.name == "my_scene"

    def test_id_nonempty(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert len(stage.id) > 0

    def test_default_prim_none_initially(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.default_prim is None

    def test_define_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        prim = stage.define_prim("/World/Cube", "Mesh")
        assert prim is not None

    def test_get_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/World/Cube", "Mesh")
        prim = stage.get_prim("/World/Cube")
        assert prim is not None

    def test_get_prim_nonexistent_is_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        prim = stage.get_prim("/NonExistent")
        assert prim is None

    def test_prim_count_increases(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        assert stage.prim_count() >= 2

    def test_set_and_get_attribute(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Cube", "Mesh")
        val = dcc_mcp_core.VtValue.from_float(1.0)
        stage.set_attribute("/Cube", "radius", val)
        result = stage.get_attribute("/Cube", "radius")
        assert result is not None
        assert abs(result.to_python() - 1.0) < 1e-4

    def test_get_attribute_missing_is_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Cube", "Mesh")
        assert stage.get_attribute("/Cube", "missing_attr") is None

    def test_list_prims(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        prims = stage.list_prims()
        paths = [p.path for p in prims]
        # paths should include /A and /B
        path_strings = [str(p) for p in paths]
        assert any("/A" in s for s in path_strings)

    def test_set_default_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Root", "Xform")
        stage.set_default_prim("/Root")
        assert stage.default_prim == "/Root"

    def test_set_meters_per_unit(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.set_meters_per_unit(0.01)
        assert abs(stage.meters_per_unit - 0.01) < 1e-6

    def test_repr_contains_name(self) -> None:
        stage = dcc_mcp_core.UsdStage("my_scene")
        assert "my_scene" in repr(stage)

    def test_export_usda_is_string(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        usda = stage.export_usda()
        assert isinstance(usda, str)
        assert len(usda) > 0

    def test_export_usda_contains_header(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        usda = stage.export_usda()
        # USDA files start with #usda
        assert "#usda" in usda

    def test_prim_attributes_roundtrip(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        prim = stage.define_prim("/Sphere", "Sphere")
        val = dcc_mcp_core.VtValue.from_float(2.5)
        prim.set_attribute("radius", val)
        retrieved = prim.get_attribute("radius")
        assert retrieved is not None
        assert abs(retrieved.to_python() - 2.5) < 1e-4


# ── UsdPrim ───────────────────────────────────────────────────────────────────


class TestUsdPrim:
    def _make_prim(self, path: str = "/World/Cube", type_name: str = "Mesh") -> dcc_mcp_core.UsdPrim:
        stage = dcc_mcp_core.UsdStage("scene")
        return stage.define_prim(path, type_name)

    def test_path(self) -> None:
        prim = self._make_prim("/World/Cube")
        assert str(prim.path) == "/World/Cube"

    def test_type_name(self) -> None:
        prim = self._make_prim("/World/Cube", "Mesh")
        assert prim.type_name == "Mesh"

    def test_active_default_true(self) -> None:
        prim = self._make_prim()
        assert prim.active is True

    def test_name(self) -> None:
        prim = self._make_prim("/World/Cube")
        assert prim.name == "Cube"

    def test_set_and_get_attribute(self) -> None:
        prim = self._make_prim("/World/Cube")
        val = dcc_mcp_core.VtValue.from_string("smooth")
        prim.set_attribute("subdivisionScheme", val)
        result = prim.get_attribute("subdivisionScheme")
        assert result is not None
        assert result.to_python() == "smooth"

    def test_attribute_names_contains_set_attr(self) -> None:
        prim = self._make_prim()
        prim.set_attribute("color", dcc_mcp_core.VtValue.from_string("red"))
        assert "color" in prim.attribute_names()

    def test_attributes_summary_is_dict(self) -> None:
        prim = self._make_prim()
        prim.set_attribute("size", dcc_mcp_core.VtValue.from_float(1.0))
        summary = prim.attributes_summary()
        assert isinstance(summary, dict)
        assert "size" in summary

    def test_repr_contains_path(self) -> None:
        prim = self._make_prim("/World/Cube")
        assert "/World/Cube" in repr(prim)

    def test_has_api_false_by_default(self) -> None:
        prim = self._make_prim()
        assert prim.has_api("SomeAPI") is False


# ── Bridge functions ──────────────────────────────────────────────────────────


class TestBridgeFunctions:
    def test_stage_to_scene_info_json_returns_json(self) -> None:
        stage = dcc_mcp_core.UsdStage("bridge_test")
        json_str = dcc_mcp_core.stage_to_scene_info_json(stage)
        assert isinstance(json_str, str)
        parsed = json.loads(json_str)
        assert isinstance(parsed, dict)

    def test_scene_info_json_to_stage_roundtrip(self) -> None:
        # Build a scene info dict and round-trip through stage
        scene_info = json.dumps(
            {
                "name": "rt_scene",
                "dcc_name": "Maya",
                "objects": [{"path": "/World/Cube", "type": "Mesh"}],
                "frames": {"start": 1, "end": 100, "current": 1, "fps": 24.0},
            }
        )
        stage = dcc_mcp_core.scene_info_json_to_stage(scene_info)
        assert stage is not None
        assert isinstance(stage, dcc_mcp_core.UsdStage)

    def test_scene_info_json_invalid_raises(self) -> None:
        with pytest.raises((RuntimeError, ValueError)):
            dcc_mcp_core.scene_info_json_to_stage("{ not json }")

    def test_units_to_mpu_centimeters(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("cm")
        assert abs(mpu - 0.01) < 1e-6

    def test_units_to_mpu_meters(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("m")
        assert abs(mpu - 1.0) < 1e-6

    def test_mpu_to_units_01(self) -> None:
        unit = dcc_mcp_core.mpu_to_units(0.01)
        assert unit.lower() in ("cm", "centimeters", "centimeter")

    def test_mpu_to_units_1(self) -> None:
        unit = dcc_mcp_core.mpu_to_units(1.0)
        assert unit.lower() in ("m", "meters", "meter")
