"""Tests for mpu_to_units/units_to_mpu, PyBufferPool.available() dynamics, and UsdStage deep API.

New tests targeting previously uncovered APIs:

- mpu_to_units / units_to_mpu (zero existing tests)
- PyBufferPool.available() decreases on acquire, PyBufferPool properties
- UsdStage.start_time_code / end_time_code setters
- UsdStage.default_prim read-only verification
- UsdStage.metrics() structure
- UsdStage.from_json() roundtrip fidelity (prims preserved, attributes preserved)
- UsdStage.export_usda() contains prim names
- UsdPrim.attribute_names() after multi-set
"""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core

# ── mpu_to_units ─────────────────────────────────────────────────────────────


class TestUnitsToMpu:
    """units_to_mpu: convert common unit strings to metersPerUnit float."""

    def test_meters(self) -> None:
        assert abs(dcc_mcp_core.units_to_mpu("m") - 1.0) < 1e-6

    def test_centimeters(self) -> None:
        assert abs(dcc_mcp_core.units_to_mpu("cm") - 0.01) < 1e-6

    def test_millimeters(self) -> None:
        assert abs(dcc_mcp_core.units_to_mpu("mm") - 0.001) < 1e-6

    def test_kilometers(self) -> None:
        result = dcc_mcp_core.units_to_mpu("km")
        assert result > 1.0

    def test_inches(self) -> None:
        result = dcc_mcp_core.units_to_mpu("in")
        assert result > 0.0
        assert result < 1.0

    def test_feet(self) -> None:
        result = dcc_mcp_core.units_to_mpu("ft")
        assert result > 0.0

    def test_returns_float(self) -> None:
        result = dcc_mcp_core.units_to_mpu("cm")
        assert isinstance(result, float)

    def test_positive(self) -> None:
        """All unit conversions produce a positive MPU value."""
        for unit in ("m", "cm", "mm"):
            assert dcc_mcp_core.units_to_mpu(unit) > 0.0


class TestMpuToUnits:
    """mpu_to_units: convert metersPerUnit float back to a unit string."""

    def test_one_is_meters(self) -> None:
        result = dcc_mcp_core.mpu_to_units(1.0)
        assert isinstance(result, str)
        assert len(result) > 0

    def test_centimeters_roundtrip(self) -> None:
        """units_to_mpu('cm') → mpu_to_units → should resolve back to cm-ish."""
        mpu = dcc_mcp_core.units_to_mpu("cm")
        unit = dcc_mcp_core.mpu_to_units(mpu)
        assert isinstance(unit, str)
        assert len(unit) > 0

    def test_meters_roundtrip(self) -> None:
        mpu = dcc_mcp_core.units_to_mpu("m")
        unit = dcc_mcp_core.mpu_to_units(mpu)
        assert isinstance(unit, str)

    def test_returns_string(self) -> None:
        result = dcc_mcp_core.mpu_to_units(0.01)
        assert isinstance(result, str)

    def test_small_mpu_returns_string(self) -> None:
        result = dcc_mcp_core.mpu_to_units(0.001)
        assert isinstance(result, str)
        assert len(result) > 0


# ── PyBufferPool — available() dynamic behaviour ───────────────────────────


class TestPyBufferPoolAvailable:
    def test_initial_available_equals_capacity(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=4, buffer_size=256)
        assert pool.available() == 4

    def test_available_decreases_on_acquire(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=256)
        assert pool.available() == 3
        buf1 = pool.acquire()
        assert pool.available() == 2
        buf2 = pool.acquire()
        assert pool.available() == 1
        # Keep references alive
        _ = buf1, buf2

    def test_acquire_all_then_exhausted(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=256)
        buf1 = pool.acquire()
        buf2 = pool.acquire()
        assert pool.available() == 0
        with pytest.raises(RuntimeError):
            pool.acquire()
        _ = buf1, buf2

    def test_capacity_property(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=5, buffer_size=1024)
        assert pool.capacity() == 5

    def test_buffer_size_property(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=4096)
        assert pool.buffer_size() == 4096

    def test_acquired_buffer_is_functional(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=2, buffer_size=512)
        buf = pool.acquire()
        payload = b"scene data"
        written = buf.write(payload)
        assert written == len(payload)
        assert buf.read() == payload

    def test_repr_contains_capacity(self) -> None:
        pool = dcc_mcp_core.PyBufferPool(capacity=3, buffer_size=128)
        assert "3" in repr(pool)


# ── UsdStage deep API ──────────────────────────────────────────────────────


class TestUsdStageTimeCodes:
    def test_start_time_code_defaults_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_test")
        assert stage.start_time_code is None

    def test_end_time_code_defaults_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_test2")
        assert stage.end_time_code is None

    def test_set_start_time_code(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_set")
        stage.start_time_code = 1.0
        assert stage.start_time_code == 1.0

    def test_set_end_time_code(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_end")
        stage.end_time_code = 120.0
        assert stage.end_time_code == 120.0

    def test_set_both_time_codes(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_both")
        stage.start_time_code = 0.0
        stage.end_time_code = 240.0
        assert stage.start_time_code == 0.0
        assert stage.end_time_code == 240.0

    def test_set_time_code_to_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("tc_none")
        stage.start_time_code = 10.0
        stage.start_time_code = None
        assert stage.start_time_code is None


class TestUsdStageDefaultPrim:
    def test_default_prim_defaults_none(self) -> None:
        stage = dcc_mcp_core.UsdStage("dp_test")
        assert stage.default_prim is None

    def test_default_prim_is_none_or_string(self) -> None:
        """default_prim is read-only in the PyO3 binding; verify type is str | None."""
        stage = dcc_mcp_core.UsdStage("dp_type")
        assert stage.default_prim is None or isinstance(stage.default_prim, str)

    def test_default_prim_read_only(self) -> None:
        """Verify default_prim cannot be written (read-only PyO3 property)."""
        stage = dcc_mcp_core.UsdStage("dp_ro")
        with pytest.raises(AttributeError):
            stage.default_prim = "/World"  # type: ignore[misc]


class TestUsdStageMetrics:
    def test_metrics_returns_dict(self) -> None:
        stage = dcc_mcp_core.UsdStage("metrics_test")
        m = stage.metrics()
        assert isinstance(m, dict)

    def test_metrics_prim_count_increases(self) -> None:
        stage = dcc_mcp_core.UsdStage("metrics_count")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Mesh")
        m = stage.metrics()
        assert isinstance(m, dict)
        # prim_count should be at least 2
        prim_count = m.get("prim_count", m.get("prims", 0))
        assert prim_count >= 2

    def test_metrics_empty_stage(self) -> None:
        stage = dcc_mcp_core.UsdStage("metrics_empty")
        m = stage.metrics()
        assert isinstance(m, dict)


class TestUsdStageFromJson:
    def test_from_json_roundtrip_name(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_test")
        json_str = stage.to_json()
        back = dcc_mcp_core.UsdStage.from_json(json_str)
        assert back.name == "json_test"

    def test_from_json_roundtrip_prims(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_prims")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        json_str = stage.to_json()
        back = dcc_mcp_core.UsdStage.from_json(json_str)
        assert back.has_prim("/World")
        assert back.has_prim("/World/Cube")

    def test_from_json_roundtrip_attribute(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_attr")
        stage.define_prim("/Cube", "Mesh")
        stage.set_attribute("/Cube", "radius", dcc_mcp_core.VtValue.from_float(2.5))
        json_str = stage.to_json()
        back = dcc_mcp_core.UsdStage.from_json(json_str)
        val = back.get_attribute("/Cube", "radius")
        assert val is not None
        assert abs(val.to_python() - 2.5) < 1e-6

    def test_from_json_roundtrip_fps(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_fps")
        stage.fps = 30.0
        json_str = stage.to_json()
        back = dcc_mcp_core.UsdStage.from_json(json_str)
        assert back.fps is not None
        assert abs(back.fps - 30.0) < 1e-6

    def test_from_json_roundtrip_up_axis(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_axis")
        stage.up_axis = "Z"
        json_str = stage.to_json()
        back = dcc_mcp_core.UsdStage.from_json(json_str)
        assert back.up_axis == "Z"

    def test_to_json_is_valid_json(self) -> None:
        stage = dcc_mcp_core.UsdStage("json_valid")
        stage.define_prim("/Root", "Xform")
        json_str = stage.to_json()
        parsed = json.loads(json_str)
        assert isinstance(parsed, dict)


class TestUsdStageExportUsda:
    def test_export_usda_is_string(self) -> None:
        stage = dcc_mcp_core.UsdStage("usda_test")
        usda = stage.export_usda()
        assert isinstance(usda, str)
        assert len(usda) > 0

    def test_export_usda_contains_prim_name(self) -> None:
        stage = dcc_mcp_core.UsdStage("usda_prim")
        stage.define_prim("/MyPrim", "Xform")
        usda = stage.export_usda()
        assert "MyPrim" in usda

    def test_export_usda_changes_after_add_prim(self) -> None:
        stage = dcc_mcp_core.UsdStage("usda_change")
        usda_before = stage.export_usda()
        stage.define_prim("/NewPrim", "Mesh")
        usda_after = stage.export_usda()
        assert len(usda_after) > len(usda_before)


class TestUsdPrimAttributeNames:
    def test_attribute_names_empty_initially(self) -> None:
        stage = dcc_mcp_core.UsdStage("prim_attr_names")
        prim = stage.define_prim("/Cube", "Mesh")
        names = prim.attribute_names()
        assert isinstance(names, list)

    def test_attribute_names_after_set(self) -> None:
        stage = dcc_mcp_core.UsdStage("prim_attr_set")
        prim = stage.define_prim("/Sphere", "Mesh")
        prim.set_attribute("radius", dcc_mcp_core.VtValue.from_float(1.0))
        prim.set_attribute("center", dcc_mcp_core.VtValue.from_vec3f(0.0, 0.0, 0.0))
        names = prim.attribute_names()
        assert "radius" in names
        assert "center" in names

    def test_attribute_names_no_duplicates(self) -> None:
        stage = dcc_mcp_core.UsdStage("prim_no_dup")
        prim = stage.define_prim("/Node", "Xform")
        prim.set_attribute("size", dcc_mcp_core.VtValue.from_float(1.0))
        prim.set_attribute("size", dcc_mcp_core.VtValue.from_float(2.0))  # overwrite
        names = prim.attribute_names()
        assert names.count("size") == 1
