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

    # ── fps ──────────────────────────────────────────────────────────────────

    def test_fps_default_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.fps is None

    def test_fps_setter(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.fps = 24.0
        assert abs(stage.fps - 24.0) < 1e-6

    def test_fps_setter_60(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.fps = 60.0
        assert abs(stage.fps - 60.0) < 1e-6

    def test_fps_setter_25(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.fps = 25.0
        assert abs(stage.fps - 25.0) < 1e-6

    # ── up_axis ───────────────────────────────────────────────────────────────

    def test_up_axis_default_y(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.up_axis == "Y"

    def test_up_axis_setter_z(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.up_axis = "Z"
        assert stage.up_axis == "Z"

    def test_up_axis_setter_y(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.up_axis = "Y"
        assert stage.up_axis == "Y"

    # ── traverse ─────────────────────────────────────────────────────────────

    def test_traverse_returns_list(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        result = stage.traverse()
        assert isinstance(result, list)

    def test_traverse_empty_stage(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.traverse() == []

    def test_traverse_includes_defined_prims(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Mesh")
        traversed = stage.traverse()
        paths = {str(p.path) for p in traversed}
        assert "/A" in paths
        assert "/B" in paths

    # ── has_prim ──────────────────────────────────────────────────────────────

    def test_has_prim_existing(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/World", "Xform")
        assert stage.has_prim("/World") is True

    def test_has_prim_missing(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.has_prim("/NonExistent") is False

    def test_has_prim_after_remove(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Cube", "Mesh")
        assert stage.has_prim("/Cube") is True
        stage.remove_prim("/Cube")
        assert stage.has_prim("/Cube") is False

    # ── remove_prim ───────────────────────────────────────────────────────────

    def test_remove_prim_returns_true(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/ToRemove", "Xform")
        result = stage.remove_prim("/ToRemove")
        assert result is True

    def test_remove_prim_reduces_count(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        count_before = stage.prim_count()
        stage.remove_prim("/A")
        assert stage.prim_count() < count_before

    def test_remove_prim_nonexistent_returns_false(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        result = stage.remove_prim("/DoesNotExist")
        assert result is False

    # ── prims_of_type ─────────────────────────────────────────────────────────

    def test_prims_of_type_returns_list(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        result = stage.prims_of_type("Mesh")
        assert isinstance(result, list)

    def test_prims_of_type_empty_for_unknown(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        assert stage.prims_of_type("UnknownType") == []

    def test_prims_of_type_finds_matching(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Mesh1", "Mesh")
        stage.define_prim("/Mesh2", "Mesh")
        stage.define_prim("/Xf1", "Xform")
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 2
        for prim in meshes:
            assert prim.type_name == "Mesh"

    def test_prims_of_type_does_not_include_other_types(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        stage.define_prim("/Mesh1", "Mesh")
        stage.define_prim("/Xf1", "Xform")
        xforms = stage.prims_of_type("Xform")
        assert len(xforms) == 1
        assert xforms[0].type_name == "Xform"

    # ── to_json ───────────────────────────────────────────────────────────────

    def test_to_json_returns_string(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        result = stage.to_json()
        assert isinstance(result, str)

    def test_to_json_is_valid_json(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_test")
        json_str = stage.to_json()
        parsed = json.loads(json_str)
        assert isinstance(parsed, dict)

    def test_to_json_contains_id(self) -> None:
        stage = dcc_mcp_core.UsdStage("scene")
        parsed = json.loads(stage.to_json())
        assert "id" in parsed

    def test_to_json_name_matches(self) -> None:
        stage = dcc_mcp_core.UsdStage("my_unique_scene")
        parsed = json.loads(stage.to_json())
        assert parsed.get("name") == "my_unique_scene"


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

    def test_units_to_mpu_millimeters(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("mm")
        assert abs(mpu - 0.001) < 1e-9

    def test_units_to_mpu_kilometers(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("km")
        assert abs(mpu - 1000.0) < 1e-6

    def test_units_to_mpu_inch(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("inch")
        assert abs(mpu - 0.0254) < 1e-7

    def test_units_to_mpu_foot(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("ft")
        assert abs(mpu - 0.3048) < 1e-6

    def test_units_to_mpu_foot_alias(self) -> None:
        assert dcc_mcp_core.units_to_mpu("foot") == dcc_mcp_core.units_to_mpu("ft")

    def test_units_to_mpu_yard(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("yd")
        assert abs(mpu - 0.9144) < 1e-6

    def test_units_to_mpu_yard_alias(self) -> None:
        assert dcc_mcp_core.units_to_mpu("yard") == dcc_mcp_core.units_to_mpu("yd")

    def test_units_to_mpu_unknown_returns_default(self) -> None:
        """Unknown unit strings should return a default (centimeter = 0.01)."""
        mpu = dcc_mcp_core.units_to_mpu("unknown_unit")
        assert isinstance(mpu, float)
        assert mpu > 0.0

    def test_units_to_mpu_empty_returns_default(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("")
        assert isinstance(mpu, float)
        assert mpu > 0.0

    def test_mpu_to_units_roundtrip_cm(self) -> None:
        """mpu_to_units(units_to_mpu('cm')) should yield 'cm' or equivalent."""
        mpu = dcc_mcp_core.units_to_mpu("cm")
        unit = dcc_mcp_core.mpu_to_units(mpu)
        assert unit.lower() in ("cm", "centimeters", "centimeter")

    def test_units_to_mpu_case_sensitivity(self) -> None:
        """Check whether unit strings are case-sensitive."""
        mpu_lower = dcc_mcp_core.units_to_mpu("cm")
        mpu_upper_result = dcc_mcp_core.units_to_mpu("CM")
        # Both should return a valid float regardless of the case handling
        assert isinstance(mpu_upper_result, float)
        assert mpu_lower > 0.0


# ── VtValue edge cases ────────────────────────────────────────────────────────


class TestVtValueEdgeCases:
    def test_from_bool_false_to_python(self) -> None:
        v = dcc_mcp_core.VtValue.from_bool(False)
        assert v.to_python() is False
        assert v.type_name == "bool"

    def test_from_int_zero(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(0)
        assert v.to_python() == 0
        assert v.type_name == "int"

    def test_from_int_negative(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(-999)
        assert v.to_python() == -999

    def test_from_int_max_int32(self) -> None:
        v = dcc_mcp_core.VtValue.from_int(2**31 - 1)
        assert v.to_python() == 2**31 - 1

    def test_from_float_negative(self) -> None:
        v = dcc_mcp_core.VtValue.from_float(-3.14)
        assert v.to_python() < 0.0

    def test_from_float_zero(self) -> None:
        v = dcc_mcp_core.VtValue.from_float(0.0)
        assert abs(v.to_python()) < 1e-9

    def test_from_string_empty(self) -> None:
        v = dcc_mcp_core.VtValue.from_string("")
        assert v.to_python() == ""
        assert v.type_name == "string"

    def test_from_token_empty(self) -> None:
        v = dcc_mcp_core.VtValue.from_token("")
        assert v.to_python() == ""
        assert v.type_name == "token"

    def test_from_vec3f_zero(self) -> None:
        v = dcc_mcp_core.VtValue.from_vec3f(0.0, 0.0, 0.0)
        result = v.to_python()
        assert isinstance(result, tuple)
        assert all(abs(x) < 1e-9 for x in result)

    def test_from_vec3f_negative(self) -> None:
        v = dcc_mcp_core.VtValue.from_vec3f(-1.0, -2.0, -3.0)
        result = v.to_python()
        assert result[0] < 0 and result[1] < 0 and result[2] < 0

    def test_from_asset_empty(self) -> None:
        v = dcc_mcp_core.VtValue.from_asset("")
        assert v.to_python() == ""
        assert v.type_name == "asset"


# ── SdfPath edge cases ────────────────────────────────────────────────────────


class TestSdfPathEdgeCases:
    def test_deep_path_name(self) -> None:
        p = dcc_mcp_core.SdfPath("/A/B/C")
        assert p.name == "C"

    def test_deep_path_parent_chain(self) -> None:
        p = dcc_mcp_core.SdfPath("/A/B/C")
        assert str(p.parent()) == "/A/B"
        assert str(p.parent().parent()) == "/A"

    def test_parent_of_parent_name(self) -> None:
        p = dcc_mcp_core.SdfPath("/World/geo")
        parent = p.parent()
        assert parent.name == "World"

    def test_child_creates_deeper_path(self) -> None:
        p = dcc_mcp_core.SdfPath("/World")
        child = p.child("Cube")
        grand = child.child("mesh")
        assert str(grand) == "/World/Cube/mesh"

    def test_is_absolute_single_component(self) -> None:
        p = dcc_mcp_core.SdfPath("/World")
        assert p.is_absolute is True

    def test_relative_path_is_not_absolute(self) -> None:
        try:
            p = dcc_mcp_core.SdfPath("relative")
            assert p.is_absolute is False
        except (ValueError, RuntimeError):
            pass  # Some implementations may reject relative paths

    def test_equality_after_child_parent_roundtrip(self) -> None:
        p = dcc_mcp_core.SdfPath("/World")
        child = p.child("Cube")
        back = child.parent()
        assert back == p

    def test_hash_stability(self) -> None:
        p1 = dcc_mcp_core.SdfPath("/World/geo/mesh")
        p2 = dcc_mcp_core.SdfPath("/World/geo/mesh")
        assert hash(p1) == hash(p2)
        d = {p1: "value"}
        assert d[p2] == "value"
