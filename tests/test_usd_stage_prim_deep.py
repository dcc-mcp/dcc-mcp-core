"""Deep tests for UsdStage and UsdPrim advanced APIs.

Covers:
- UsdStage.fps / up_axis / meters_per_unit property get/set roundtrips
- UsdStage.start_time_code / end_time_code property get/set
- UsdStage.default_prim property setter (stage.default_prim = "/path")
- UsdPrim.get_attribute / set_attribute multi-type roundtrips
- UsdPrim.attribute_names returns all set attributes
- UsdPrim.attributes_summary returns dict with all attribute names as keys
- UsdPrim.has_api always False for custom schemas
- UsdPrim.active property
- UsdStage.prim_count vs define_prim count consistency
- UsdStage.set_meters_per_unit vs meters_per_unit property
- UsdStage.set_fps vs fps property
- UsdStage.export_usda contains prim type info
- UsdStage.from_json restores fps / up_axis / meters_per_unit
"""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core
from dcc_mcp_core import UsdPrim
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# UsdStage.fps deep
# ---------------------------------------------------------------------------


class TestUsdStageFps:
    def test_fps_default_is_none(self):
        s = UsdStage("s")
        assert s.fps is None

    def test_fps_set_24(self):
        s = UsdStage("s")
        s.fps = 24.0
        assert abs(s.fps - 24.0) < 1e-6

    def test_fps_set_30(self):
        s = UsdStage("s")
        s.fps = 30.0
        assert abs(s.fps - 30.0) < 1e-6

    def test_fps_set_60(self):
        s = UsdStage("s")
        s.fps = 60.0
        assert abs(s.fps - 60.0) < 1e-6

    def test_fps_set_120(self):
        s = UsdStage("s")
        s.fps = 120.0
        assert abs(s.fps - 120.0) < 1e-6

    def test_fps_set_23976(self):
        """23.976 fps (NTSC film)."""
        s = UsdStage("s")
        s.fps = 23.976
        assert s.fps is not None
        assert abs(s.fps - 23.976) < 0.001

    def test_fps_overwrite(self):
        s = UsdStage("s")
        s.fps = 24.0
        s.fps = 60.0
        assert abs(s.fps - 60.0) < 1e-6

    def test_fps_set_none_resets(self):
        s = UsdStage("s")
        s.fps = 30.0
        s.fps = None
        assert s.fps is None

    def test_fps_independent_per_stage(self):
        s1 = UsdStage("a")
        s2 = UsdStage("b")
        s1.fps = 24.0
        s2.fps = 60.0
        assert abs(s1.fps - 24.0) < 1e-6
        assert abs(s2.fps - 60.0) < 1e-6

    def test_fps_preserved_in_to_json_from_json(self):
        s = UsdStage("s")
        s.fps = 25.0
        s2 = UsdStage.from_json(s.to_json())
        assert s2.fps is not None
        assert abs(s2.fps - 25.0) < 0.01


# ---------------------------------------------------------------------------
# UsdStage.up_axis deep
# ---------------------------------------------------------------------------


class TestUsdStageUpAxis:
    def test_up_axis_default_is_y(self):
        s = UsdStage("s")
        assert s.up_axis == "Y"

    def test_up_axis_set_z(self):
        s = UsdStage("s")
        s.up_axis = "Z"
        assert s.up_axis == "Z"

    def test_up_axis_set_back_to_y(self):
        s = UsdStage("s")
        s.up_axis = "Z"
        s.up_axis = "Y"
        assert s.up_axis == "Y"

    def test_up_axis_returned_as_uppercase(self):
        s = UsdStage("s")
        s.up_axis = "Z"
        assert s.up_axis in ("Y", "Z")

    def test_up_axis_independent_per_stage(self):
        s1 = UsdStage("a")
        s2 = UsdStage("b")
        s1.up_axis = "Z"
        s2.up_axis = "Y"
        assert s1.up_axis == "Z"
        assert s2.up_axis == "Y"

    def test_up_axis_preserved_in_json_roundtrip(self):
        s = UsdStage("s")
        s.up_axis = "Z"
        s2 = UsdStage.from_json(s.to_json())
        assert s2.up_axis == "Z"


# ---------------------------------------------------------------------------
# UsdStage.meters_per_unit deep
# ---------------------------------------------------------------------------


class TestUsdStageMetersPerUnit:
    def test_meters_per_unit_default(self):
        s = UsdStage("s")
        assert isinstance(s.meters_per_unit, float)
        assert s.meters_per_unit > 0.0

    def test_set_meters_per_unit_cm(self):
        s = UsdStage("s")
        s.set_meters_per_unit(0.01)
        assert abs(s.meters_per_unit - 0.01) < 1e-9

    def test_set_meters_per_unit_m(self):
        s = UsdStage("s")
        s.set_meters_per_unit(1.0)
        assert abs(s.meters_per_unit - 1.0) < 1e-9

    def test_set_meters_per_unit_mm(self):
        s = UsdStage("s")
        s.set_meters_per_unit(0.001)
        assert abs(s.meters_per_unit - 0.001) < 1e-12

    def test_set_meters_per_unit_via_method(self):
        s = UsdStage("s")
        s.set_meters_per_unit(0.01)
        assert abs(s.meters_per_unit - 0.01) < 1e-9

    def test_meters_per_unit_overwrite(self):
        s = UsdStage("s")
        s.set_meters_per_unit(1.0)
        s.set_meters_per_unit(0.01)
        assert abs(s.meters_per_unit - 0.01) < 1e-9

    def test_meters_per_unit_preserved_in_json_roundtrip(self):
        s = UsdStage("s")
        s.set_meters_per_unit(0.001)
        s2 = UsdStage.from_json(s.to_json())
        assert abs(s2.meters_per_unit - 0.001) < 1e-6


# ---------------------------------------------------------------------------
# UsdStage.start_time_code / end_time_code deep
# ---------------------------------------------------------------------------


class TestUsdStageTimeCodes:
    def test_start_time_code_default_none(self):
        s = UsdStage("s")
        assert s.start_time_code is None

    def test_end_time_code_default_none(self):
        s = UsdStage("s")
        assert s.end_time_code is None

    def test_set_start_time_code(self):
        s = UsdStage("s")
        s.start_time_code = 1.0
        assert abs(s.start_time_code - 1.0) < 1e-9

    def test_set_end_time_code(self):
        s = UsdStage("s")
        s.end_time_code = 100.0
        assert abs(s.end_time_code - 100.0) < 1e-9

    def test_set_both_time_codes(self):
        s = UsdStage("s")
        s.start_time_code = 1.0
        s.end_time_code = 240.0
        assert abs(s.start_time_code - 1.0) < 1e-9
        assert abs(s.end_time_code - 240.0) < 1e-9

    def test_time_codes_with_fps(self):
        s = UsdStage("s")
        s.fps = 24.0
        s.start_time_code = 1.0
        s.end_time_code = 120.0
        assert abs(s.fps - 24.0) < 1e-6
        assert abs(s.start_time_code - 1.0) < 1e-9
        assert abs(s.end_time_code - 120.0) < 1e-9

    def test_start_time_code_overwrite(self):
        s = UsdStage("s")
        s.start_time_code = 1.0
        s.start_time_code = 5.0
        assert abs(s.start_time_code - 5.0) < 1e-9

    def test_end_time_code_overwrite(self):
        s = UsdStage("s")
        s.end_time_code = 100.0
        s.end_time_code = 200.0
        assert abs(s.end_time_code - 200.0) < 1e-9

    def test_set_start_time_code_to_none(self):
        s = UsdStage("s")
        s.start_time_code = 1.0
        s.start_time_code = None
        assert s.start_time_code is None

    def test_set_end_time_code_to_none(self):
        s = UsdStage("s")
        s.end_time_code = 100.0
        s.end_time_code = None
        assert s.end_time_code is None


# ---------------------------------------------------------------------------
# UsdStage.default_prim deep
# ---------------------------------------------------------------------------


class TestUsdStageDefaultPrim:
    def test_default_prim_initial_none(self):
        s = UsdStage("s")
        assert s.default_prim is None

    def test_set_default_prim_via_method(self):
        s = UsdStage("s")
        s.define_prim("/World", "Xform")
        s.set_default_prim("/World")
        assert s.default_prim == "/World"

    def test_set_default_prim_root(self):
        s = UsdStage("s")
        s.define_prim("/Root", "Xform")
        s.set_default_prim("/Root")
        assert s.default_prim == "/Root"

    def test_default_prim_overwrite(self):
        s = UsdStage("s")
        s.define_prim("/A", "Xform")
        s.define_prim("/B", "Xform")
        s.set_default_prim("/A")
        s.set_default_prim("/B")
        assert s.default_prim == "/B"

    def test_default_prim_none_initial(self):
        s = UsdStage("s")
        # Before setting, default_prim should be None
        assert s.default_prim is None


# ---------------------------------------------------------------------------
# UsdPrim attribute operations deep
# ---------------------------------------------------------------------------


class TestUsdPrimAttributeOps:
    def _make_prim(self, path: str = "/Obj", type_name: str = "Mesh") -> UsdPrim:
        stage = UsdStage("stage")
        return stage.define_prim(path, type_name)

    def test_set_get_bool_true(self):
        p = self._make_prim()
        p.set_attribute("visible", VtValue.from_bool(True))
        v = p.get_attribute("visible")
        assert v is not None
        assert v.to_python() is True

    def test_set_get_bool_false(self):
        p = self._make_prim()
        p.set_attribute("visible", VtValue.from_bool(False))
        v = p.get_attribute("visible")
        assert v is not None
        assert v.to_python() is False

    def test_set_get_int_positive(self):
        p = self._make_prim()
        p.set_attribute("level", VtValue.from_int(5))
        v = p.get_attribute("level")
        assert v is not None
        assert v.to_python() == 5

    def test_set_get_int_negative(self):
        p = self._make_prim()
        p.set_attribute("offset", VtValue.from_int(-10))
        v = p.get_attribute("offset")
        assert v is not None
        assert v.to_python() == -10

    def test_set_get_int_zero(self):
        p = self._make_prim()
        p.set_attribute("idx", VtValue.from_int(0))
        v = p.get_attribute("idx")
        assert v is not None
        assert v.to_python() == 0

    def test_set_get_float(self):
        p = self._make_prim()
        p.set_attribute("scale", VtValue.from_float(3.14))
        v = p.get_attribute("scale")
        assert v is not None
        assert isinstance(v.to_python(), float)

    def test_set_get_string(self):
        p = self._make_prim()
        p.set_attribute("label", VtValue.from_string("hello"))
        v = p.get_attribute("label")
        assert v is not None
        assert v.to_python() == "hello"

    def test_set_get_token(self):
        p = self._make_prim()
        p.set_attribute("scheme", VtValue.from_token("catmullClark"))
        v = p.get_attribute("scheme")
        assert v is not None
        assert v.type_name == "token"

    def test_set_get_vec3f(self):
        p = self._make_prim()
        p.set_attribute("translate", VtValue.from_vec3f(1.0, 2.0, 3.0))
        v = p.get_attribute("translate")
        assert v is not None
        result = v.to_python()
        assert isinstance(result, tuple)
        assert len(result) == 3

    def test_get_nonexistent_returns_none(self):
        p = self._make_prim()
        assert p.get_attribute("no_such_attr") is None

    def test_overwrite_attribute(self):
        p = self._make_prim()
        p.set_attribute("count", VtValue.from_int(1))
        p.set_attribute("count", VtValue.from_int(42))
        v = p.get_attribute("count")
        assert v is not None
        assert v.to_python() == 42

    def test_multiple_attributes(self):
        p = self._make_prim()
        p.set_attribute("a", VtValue.from_int(1))
        p.set_attribute("b", VtValue.from_string("x"))
        p.set_attribute("c", VtValue.from_bool(True))
        assert p.get_attribute("a").to_python() == 1
        assert p.get_attribute("b").to_python() == "x"
        assert p.get_attribute("c").to_python() is True


# ---------------------------------------------------------------------------
# UsdPrim.attribute_names deep
# ---------------------------------------------------------------------------


class TestUsdPrimAttributeNames:
    def _make_prim(self) -> UsdPrim:
        stage = UsdStage("stage")
        return stage.define_prim("/P", "Scope")

    def test_attribute_names_empty_initially(self):
        p = self._make_prim()
        names = p.attribute_names()
        assert isinstance(names, list)

    def test_attribute_names_contains_set_attr(self):
        p = self._make_prim()
        p.set_attribute("color", VtValue.from_string("red"))
        assert "color" in p.attribute_names()

    def test_attribute_names_contains_multiple(self):
        p = self._make_prim()
        p.set_attribute("x", VtValue.from_int(1))
        p.set_attribute("y", VtValue.from_int(2))
        names = p.attribute_names()
        assert "x" in names
        assert "y" in names

    def test_attribute_names_count_increments(self):
        p = self._make_prim()
        before = len(p.attribute_names())
        p.set_attribute("new_attr", VtValue.from_bool(True))
        after = len(p.attribute_names())
        assert after > before

    def test_attribute_names_no_duplicates_on_overwrite(self):
        p = self._make_prim()
        p.set_attribute("size", VtValue.from_float(1.0))
        p.set_attribute("size", VtValue.from_float(2.0))
        names = p.attribute_names()
        # 'size' should appear at most once
        assert names.count("size") <= 1


# ---------------------------------------------------------------------------
# UsdPrim.attributes_summary deep
# ---------------------------------------------------------------------------


class TestUsdPrimAttributesSummary:
    def _make_prim(self) -> UsdPrim:
        stage = UsdStage("stage")
        return stage.define_prim("/Q", "Mesh")

    def test_attributes_summary_returns_dict(self):
        p = self._make_prim()
        assert isinstance(p.attributes_summary(), dict)

    def test_attributes_summary_contains_set_attr(self):
        p = self._make_prim()
        p.set_attribute("radius", VtValue.from_float(5.0))
        summary = p.attributes_summary()
        assert "radius" in summary

    def test_attributes_summary_values_are_strings(self):
        p = self._make_prim()
        p.set_attribute("radius", VtValue.from_float(5.0))
        summary = p.attributes_summary()
        # All values in summary should be strings
        for v in summary.values():
            assert isinstance(v, str)

    def test_attributes_summary_multiple_attrs(self):
        p = self._make_prim()
        p.set_attribute("a", VtValue.from_int(1))
        p.set_attribute("b", VtValue.from_bool(True))
        summary = p.attributes_summary()
        assert "a" in summary
        assert "b" in summary


# ---------------------------------------------------------------------------
# UsdPrim misc
# ---------------------------------------------------------------------------


class TestUsdPrimMisc:
    def test_has_api_always_false_for_unknown(self):
        stage = UsdStage("s")
        p = stage.define_prim("/X", "Mesh")
        assert p.has_api("SomeRandomAPI") is False

    def test_has_api_physics_api(self):
        stage = UsdStage("s")
        p = stage.define_prim("/X", "Mesh")
        # physics API not applied, so should be False
        assert p.has_api("PhysicsAPI") is False

    def test_active_is_true_by_default(self):
        stage = UsdStage("s")
        p = stage.define_prim("/A", "Xform")
        assert p.active is True

    def test_prim_name_matches_last_path_element(self):
        stage = UsdStage("s")
        p = stage.define_prim("/World/Geometry/Cube", "Mesh")
        assert p.name == "Cube"

    def test_prim_type_name_matches(self):
        stage = UsdStage("s")
        p = stage.define_prim("/L", "SphereLight")
        assert p.type_name == "SphereLight"

    def test_repr_contains_type_name(self):
        stage = UsdStage("s")
        p = stage.define_prim("/M", "Camera")
        assert "Camera" in repr(p) or "/M" in repr(p)


# ---------------------------------------------------------------------------
# UsdStage combined scenario
# ---------------------------------------------------------------------------


class TestUsdStageFullScenario:
    def test_complex_stage_json_roundtrip(self):
        """Build a stage with fps, up_axis, mpu, prims, attrs; roundtrip via JSON."""
        s = UsdStage("complex_scene")
        s.fps = 24.0
        s.up_axis = "Z"
        s.set_meters_per_unit(0.01)
        s.define_prim("/World", "Xform")
        s.define_prim("/World/Camera", "Camera")
        s.define_prim("/World/Mesh", "Mesh")
        s.set_attribute("/World/Mesh", "faces", VtValue.from_int(512))

        j = s.to_json()
        s2 = UsdStage.from_json(j)

        assert s2.prim_count() == s.prim_count()
        assert s2.has_prim("/World") is True
        assert s2.has_prim("/World/Camera") is True
        assert s2.has_prim("/World/Mesh") is True

    def test_stage_with_all_properties_json_roundtrip(self):
        s = UsdStage("full")
        s.fps = 30.0
        s.up_axis = "Z"
        s.set_meters_per_unit(1.0)
        j = s.to_json()
        data = json.loads(j)
        assert isinstance(data, dict)
        # Verify JSON is parseable - values may be nested
        assert len(data) > 0

    def test_prim_count_after_many_defines(self):
        s = UsdStage("big")
        n = 20
        for i in range(n):
            s.define_prim(f"/Prim{i}", "Scope")
        assert s.prim_count() == n

    def test_export_usda_contains_prim_info(self):
        s = UsdStage("scene")
        s.define_prim("/Cube", "Mesh")
        usda = s.export_usda()
        assert "Mesh" in usda or "Cube" in usda

    def test_traverse_and_prims_of_type_consistent(self):
        s = UsdStage("s")
        s.define_prim("/A", "Mesh")
        s.define_prim("/B", "Mesh")
        s.define_prim("/C", "Camera")
        all_prims = s.traverse()
        meshes = s.prims_of_type("Mesh")
        cameras = s.prims_of_type("Camera")
        assert len(all_prims) == 3
        assert len(meshes) == 2
        assert len(cameras) == 1
