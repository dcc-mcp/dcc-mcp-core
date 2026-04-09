"""Deep tests for Capturer.stats accumulation and UsdStage to_json/from_json roundtrip.

Covers:
- Capturer.new_mock() / new_auto() / backend_name
- Capturer.capture() returning CaptureFrame
- Capturer.stats() - (capture_count, total_bytes, error_count) tuple accumulation
- UsdStage.to_json() / from_json() roundtrip with prims and attributes
- UsdStage.set_attribute / get_attribute with VtValue
- UsdStage.metrics() fields
- UsdStage.name / id / prim_count / has_prim / list_prims
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import Capturer
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# Capturer
# ---------------------------------------------------------------------------


class TestCapturerBackend:
    def test_new_mock_creates_capturer(self):
        c = Capturer.new_mock()
        assert c is not None

    def test_new_auto_creates_capturer(self):
        c = Capturer.new_auto()
        assert c is not None

    def test_mock_backend_name(self):
        c = Capturer.new_mock()
        # backend_name is a method, not a property
        name = c.backend_name()
        assert isinstance(name, str)
        assert len(name) > 0

    def test_auto_backend_name_is_string(self):
        c = Capturer.new_auto()
        name = c.backend_name()
        assert isinstance(name, str)


class TestCapturerCapture:
    def test_capture_returns_frame(self):
        c = Capturer.new_mock()
        frame = c.capture()
        assert frame is not None

    def test_capture_frame_has_positive_size(self):
        c = Capturer.new_mock()
        frame = c.capture()
        # CaptureFrame.__repr__ shows size info
        repr_str = repr(frame)
        assert "1920" in repr_str or "x" in repr_str

    def test_capture_returns_distinct_objects(self):
        c = Capturer.new_mock()
        f1 = c.capture()
        f2 = c.capture()
        # Both should be valid frames (no crash)
        assert f1 is not None
        assert f2 is not None


class TestCapturerStats:
    def test_initial_stats_all_zero(self):
        c = Capturer.new_mock()
        capture_count, total_bytes, error_count = c.stats()
        assert capture_count == 0
        assert total_bytes == 0
        assert error_count == 0

    def test_stats_capture_count_increments(self):
        c = Capturer.new_mock()
        c.capture()
        capture_count, _, _ = c.stats()
        assert capture_count == 1

    def test_stats_capture_count_accumulates(self):
        c = Capturer.new_mock()
        for _ in range(5):
            c.capture()
        capture_count, _, _ = c.stats()
        assert capture_count == 5

    def test_stats_total_bytes_increases_after_capture(self):
        c = Capturer.new_mock()
        c.capture()
        _, total_bytes, _ = c.stats()
        assert total_bytes > 0

    def test_stats_total_bytes_accumulates(self):
        c = Capturer.new_mock()
        c.capture()
        _, bytes_after_1, _ = c.stats()
        c.capture()
        _, bytes_after_2, _ = c.stats()
        assert bytes_after_2 >= bytes_after_1

    def test_stats_error_count_zero_on_success(self):
        c = Capturer.new_mock()
        c.capture()
        _, _, error_count = c.stats()
        assert error_count == 0

    def test_stats_is_tuple_of_three(self):
        c = Capturer.new_mock()
        stats = c.stats()
        assert len(stats) == 3

    def test_stats_multiple_captures_accumulate_bytes(self):
        c = Capturer.new_mock()
        n = 3
        for _ in range(n):
            c.capture()
        count, total, errors = c.stats()
        assert count == n
        assert total > 0
        assert errors == 0

    def test_stats_per_capturer_instance_independent(self):
        c1 = Capturer.new_mock()
        c2 = Capturer.new_mock()
        c1.capture()
        c1.capture()
        c1.capture()
        count1, _, _ = c1.stats()
        count2, _, _ = c2.stats()
        assert count1 == 3
        assert count2 == 0


# ---------------------------------------------------------------------------
# UsdStage
# ---------------------------------------------------------------------------


class TestUsdStageBasics:
    def test_create_stage_with_name(self):
        s = UsdStage("test_stage")
        assert s.name == "test_stage"

    def test_id_is_string(self):
        s = UsdStage("my_stage")
        assert isinstance(s.id, str)
        assert len(s.id) > 0

    def test_initial_prim_count_is_zero(self):
        s = UsdStage("empty")
        assert s.prim_count() == 0

    def test_define_prim_increments_count(self):
        s = UsdStage("stage")
        s.define_prim("/World", "Scope")
        assert s.prim_count() == 1

    def test_has_prim_true_after_define(self):
        s = UsdStage("stage")
        s.define_prim("/Cube", "Cube")
        assert s.has_prim("/Cube") is True

    def test_has_prim_false_for_nonexistent(self):
        s = UsdStage("stage")
        assert s.has_prim("/NoSuchPrim") is False

    def test_list_prims_contains_defined_prim(self):
        s = UsdStage("stage")
        s.define_prim("/MyMesh", "Mesh")
        prims = s.list_prims()
        # list_prims() returns UsdPrim objects; use str(p.path) to get the path string
        paths = [str(p.path) for p in prims]
        assert "/MyMesh" in paths


class TestUsdStageSetGetAttribute:
    def test_set_and_get_string_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Cube", "Cube")
        s.set_attribute("/Cube", "display_name", VtValue.from_string("MyCube"))
        attr = s.get_attribute("/Cube", "display_name")
        assert attr is not None
        assert attr.to_python() == "MyCube"
        assert attr.type_name == "string"

    def test_set_and_get_bool_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Light", "SphereLight")
        s.set_attribute("/Light", "enabled", VtValue.from_bool(True))
        attr = s.get_attribute("/Light", "enabled")
        assert attr is not None
        assert attr.to_python() is True
        assert attr.type_name == "bool"

    def test_set_and_get_int_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Obj", "Mesh")
        s.set_attribute("/Obj", "subdivision_level", VtValue.from_int(3))
        attr = s.get_attribute("/Obj", "subdivision_level")
        assert attr is not None
        assert attr.to_python() == 3
        assert attr.type_name == "int"

    def test_set_and_get_float_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Ball", "Sphere")
        s.set_attribute("/Ball", "radius", VtValue.from_float(2.5))
        attr = s.get_attribute("/Ball", "radius")
        assert attr is not None
        assert isinstance(attr.to_python(), float)
        assert attr.type_name == "float"

    def test_set_and_get_token_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Cam", "Camera")
        s.set_attribute("/Cam", "projection", VtValue.from_token("perspective"))
        attr = s.get_attribute("/Cam", "projection")
        assert attr is not None
        assert attr.type_name == "token"

    def test_get_nonexistent_attribute_returns_none(self):
        s = UsdStage("stage")
        s.define_prim("/Empty", "Scope")
        attr = s.get_attribute("/Empty", "nonexistent_attr")
        assert attr is None

    def test_overwrite_attribute_updates_value(self):
        s = UsdStage("stage")
        s.define_prim("/Obj", "Mesh")
        s.set_attribute("/Obj", "count", VtValue.from_int(1))
        s.set_attribute("/Obj", "count", VtValue.from_int(99))
        attr = s.get_attribute("/Obj", "count")
        assert attr.to_python() == 99


class TestUsdStageToFromJson:
    def test_to_json_returns_string(self):
        s = UsdStage("stage")
        result = s.to_json()
        assert isinstance(result, str)

    def test_to_json_is_valid_json(self):
        s = UsdStage("stage")
        parsed = json.loads(s.to_json())
        assert isinstance(parsed, dict)

    def test_from_json_restores_stage(self):
        s = UsdStage("my_stage")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        assert s2 is not None

    def test_from_json_restores_name(self):
        s = UsdStage("roundtrip_stage")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        assert s2.name == "roundtrip_stage"

    def test_from_json_restores_prim_count(self):
        s = UsdStage("stage")
        s.define_prim("/World", "Scope")
        s.define_prim("/World/Cube", "Cube")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        assert s2.prim_count() == s.prim_count()

    def test_from_json_restores_prim_existence(self):
        s = UsdStage("stage")
        s.define_prim("/World", "Scope")
        s.define_prim("/World/Cube", "Cube")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        assert s2.has_prim("/World") is True
        assert s2.has_prim("/World/Cube") is True

    def test_from_json_restores_string_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Cube", "Cube")
        s.set_attribute("/Cube", "display_name", VtValue.from_string("TestCube"))
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        attr = s2.get_attribute("/Cube", "display_name")
        assert attr is not None
        assert attr.to_python() == "TestCube"

    def test_from_json_restores_int_attribute(self):
        s = UsdStage("stage")
        s.define_prim("/Obj", "Mesh")
        s.set_attribute("/Obj", "level", VtValue.from_int(7))
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        attr = s2.get_attribute("/Obj", "level")
        assert attr is not None
        assert attr.to_python() == 7

    def test_from_json_empty_stage_roundtrip(self):
        s = UsdStage("empty_stage")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        assert s2.prim_count() == 0

    def test_json_contains_stage_name(self):
        s = UsdStage("named_stage")
        data = json.loads(s.to_json())
        # Stage name should appear somewhere in the serialized JSON
        assert json.dumps(data).find("named_stage") != -1

    def test_from_json_with_multiple_prims(self):
        s = UsdStage("complex")
        prim_paths = ["/A", "/B", "/C", "/D", "/E"]
        for path in prim_paths:
            s.define_prim(path, "Scope")
        j = s.to_json()
        s2 = UsdStage.from_json(j)
        for path in prim_paths:
            assert s2.has_prim(path) is True


class TestUsdStageMetrics:
    def test_metrics_returns_dict(self):
        s = UsdStage("stage")
        m = s.metrics()
        assert isinstance(m, dict)

    def test_metrics_has_prim_count(self):
        s = UsdStage("stage")
        m = s.metrics()
        assert "prim_count" in m

    def test_metrics_has_mesh_count(self):
        s = UsdStage("stage")
        m = s.metrics()
        assert "mesh_count" in m

    def test_metrics_has_camera_count(self):
        s = UsdStage("stage")
        m = s.metrics()
        assert "camera_count" in m

    def test_metrics_has_light_count(self):
        s = UsdStage("stage")
        m = s.metrics()
        assert "light_count" in m

    def test_metrics_prim_count_matches_stage(self):
        s = UsdStage("stage")
        s.define_prim("/A", "Scope")
        s.define_prim("/B", "Scope")
        m = s.metrics()
        assert m["prim_count"] == 2

    def test_metrics_mesh_count_increments(self):
        s = UsdStage("stage")
        s.define_prim("/Mesh1", "Mesh")
        s.define_prim("/Mesh2", "Mesh")
        m = s.metrics()
        assert m["mesh_count"] == 2

    def test_metrics_camera_count_increments(self):
        s = UsdStage("stage")
        s.define_prim("/Cam", "Camera")
        m = s.metrics()
        assert m["camera_count"] == 1

    def test_metrics_empty_stage_all_zero(self):
        s = UsdStage("empty")
        m = s.metrics()
        assert m["prim_count"] == 0
        assert m["mesh_count"] == 0
        assert m["camera_count"] == 0
        assert m["light_count"] == 0
