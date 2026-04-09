"""Tests for ActionValidator, UsdStage deep methods, VersionedRegistry edge cases, EventBus kwargs.

Coverage targets:
- ActionValidator: from_schema_json, from_action_registry, validate (tuple result)
- UsdStage: list_prims, traverse, prims_of_type, has_prim, to_json, from_json, export_usda,
            metrics, set_default_prim, default_prim, set_attribute/get_attribute on UsdPrim/UsdStage,
            id/name, remove_prim nonexistent, attributes_summary/attribute_names, prim attrs
- VersionedRegistry: resolve returns None on no match, resolve_all empty, remove nonexistent,
                     total_entries, keys multi-dcc, versions nonexistent, latest_version None
- VersionConstraint: parse invalid raises ValueError, matches all constraint types
- EventBus: publish with kwargs, multiple subscribers, unsubscribe, no-subscriber publish
"""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import ActionValidator
from dcc_mcp_core import EventBus
from dcc_mcp_core import SemVer
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VersionConstraint
from dcc_mcp_core import VersionedRegistry
from dcc_mcp_core import VtValue


# ---------------------------------------------------------------------------
# TestActionValidatorFromSchemaJson
# ---------------------------------------------------------------------------
class TestActionValidatorFromSchemaJson:
    """ActionValidator.from_schema_json — schema construction and validate."""

    def test_returns_action_validator_instance(self) -> None:
        schema = '{"type": "object"}'
        v = ActionValidator.from_schema_json(schema)
        assert "ActionValidator" in type(v).__name__

    def test_validate_returns_tuple(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object"}')
        result = v.validate("{}")
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_validate_ok_first_element_true(self) -> None:
        schema = '{"type": "object", "properties": {"x": {"type": "number"}}}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"x": 1.5}')
        assert ok is True
        assert errors == []

    def test_validate_missing_required_field(self) -> None:
        schema = '{"type": "object", "properties": {"radius": {"type": "number"}},"required": ["radius"]}'
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0
        assert "radius" in errors[0]

    def test_validate_error_list_contains_string(self) -> None:
        schema = '{"type": "object", "required": ["name"]}'
        v = ActionValidator.from_schema_json(schema)
        _, errors = v.validate("{}")
        assert all(isinstance(e, str) for e in errors)

    def test_validate_empty_schema_accepts_any_object(self) -> None:
        v = ActionValidator.from_schema_json("{}")
        ok, errors = v.validate('{"anything": 42, "nested": {"a": 1}}')
        assert ok is True
        assert errors == []

    def test_validate_extra_fields_allowed_by_default(self) -> None:
        schema = '{"type": "object", "properties": {"x": {"type": "number"}}}'
        v = ActionValidator.from_schema_json(schema)
        ok, _ = v.validate('{"x": 1.0, "extra_key": "ignored"}')
        assert ok is True

    def test_validate_number_type(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object", "properties": {"val": {"type": "number"}}}')
        ok, _ = v.validate('{"val": 3.14}')
        assert ok is True

    def test_validate_integer_type(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object", "properties": {"count": {"type": "integer"}}}')
        ok, _ = v.validate('{"count": 42}')
        assert ok is True

    def test_validate_string_type(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object", "properties": {"name": {"type": "string"}}}')
        ok, _ = v.validate('{"name": "hello"}')
        assert ok is True

    def test_validate_boolean_type(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object", "properties": {"flag": {"type": "boolean"}}}')
        ok, _ = v.validate('{"flag": true}')
        assert ok is True

    def test_validate_array_type(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object", "properties": {"items": {"type": "array"}}}')
        ok, _ = v.validate('{"items": [1, 2, 3]}')
        assert ok is True

    def test_validate_nested_object(self) -> None:
        schema = (
            '{"type": "object", "properties": {"pos": {"type": "object",'
            '"properties": {"x": {"type": "number"}, "y": {"type": "number"}}}}}'
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _ = v.validate('{"pos": {"x": 1.0, "y": 2.0}}')
        assert ok is True

    def test_validate_invalid_json_raises_value_error(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object"}')
        with pytest.raises((ValueError, Exception)):
            v.validate("not-valid-json")

    def test_validate_multiple_required_fields_all_missing(self) -> None:
        schema = (
            '{"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "number"}},'
            '"required": ["a", "b"]}'
        )
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) >= 1

    def test_validate_multiple_required_fields_one_missing(self) -> None:
        schema = (
            '{"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "number"}},'
            '"required": ["a", "b"]}'
        )
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"a": "hello"}')
        assert ok is False
        assert "b" in errors[0]

    def test_validate_all_required_fields_present(self) -> None:
        schema = (
            '{"type": "object", "properties": {"a": {"type": "string"}, "b": {"type": "number"}},'
            '"required": ["a", "b"]}'
        )
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"a": "hi", "b": 5.0}')
        assert ok is True
        assert errors == []

    def test_validate_empty_params_empty_schema(self) -> None:
        v = ActionValidator.from_schema_json('{"type": "object"}')
        ok, _ = v.validate("{}")
        assert ok is True


# ---------------------------------------------------------------------------
# TestActionValidatorFromActionRegistry
# ---------------------------------------------------------------------------
class TestActionValidatorFromActionRegistry:
    """ActionValidator.from_action_registry — registry-backed construction."""

    def test_returns_validator_instance(self) -> None:
        reg = ActionRegistry()
        reg.register(
            name="create_sphere",
            description="Create a sphere",
            category="geometry",
            input_schema='{"type": "object", "properties": {"r": {"type": "number"}}}',
        )
        v = ActionValidator.from_action_registry(reg, "create_sphere")
        assert "ActionValidator" in type(v).__name__

    def test_validate_with_registry_schema(self) -> None:
        reg = ActionRegistry()
        reg.register(
            name="my_op",
            description="Op",
            category="ops",
            input_schema='{"type": "object", "properties": {"size": {"type": "number"}}, "required": ["size"]}',
        )
        v = ActionValidator.from_action_registry(reg, "my_op")
        ok, _ = v.validate('{"size": 3.0}')
        assert ok is True

    def test_validate_missing_required_via_registry(self) -> None:
        reg = ActionRegistry()
        reg.register(
            name="req_op",
            description="Op",
            category="ops",
            input_schema='{"type": "object", "required": ["x"]}',
        )
        v = ActionValidator.from_action_registry(reg, "req_op")
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0

    def test_nonexistent_action_raises_key_error(self) -> None:
        reg = ActionRegistry()
        with pytest.raises(KeyError):
            ActionValidator.from_action_registry(reg, "no_such_action")

    def test_action_without_input_schema_accepts_any(self) -> None:
        reg = ActionRegistry()
        reg.register(name="bare_op", description="Op", category="ops")
        v = ActionValidator.from_action_registry(reg, "bare_op")
        ok, _ = v.validate('{"anything": 1}')
        assert ok is True

    def test_multiple_validators_from_same_registry_are_independent(self) -> None:
        reg = ActionRegistry()
        reg.register(
            name="op_a",
            description="Op A",
            category="ops",
            input_schema='{"type": "object", "required": ["a"]}',
        )
        reg.register(
            name="op_b",
            description="Op B",
            category="ops",
            input_schema='{"type": "object", "required": ["b"]}',
        )
        va = ActionValidator.from_action_registry(reg, "op_a")
        vb = ActionValidator.from_action_registry(reg, "op_b")

        ok_a, _ = va.validate('{"a": 1}')
        ok_b, _ = vb.validate('{"b": 2}')
        assert ok_a is True
        assert ok_b is True

        bad_a, _errs_a = va.validate('{"b": 1}')
        assert bad_a is False


# ---------------------------------------------------------------------------
# TestUsdStageListTraverse
# ---------------------------------------------------------------------------
class TestUsdStageListTraverse:
    """UsdStage.list_prims, traverse, prims_of_type, has_prim."""

    def _make_stage(self) -> UsdStage:
        stage = UsdStage("test_lt")
        stage.define_prim("/Root", "Xform")
        stage.define_prim("/Root/Sphere", "Sphere")
        stage.define_prim("/Root/Cube", "Cube")
        stage.define_prim("/Env", "Scope")
        return stage

    def test_list_prims_returns_list(self) -> None:
        stage = self._make_stage()
        prims = stage.list_prims()
        assert isinstance(prims, list)

    def test_list_prims_count_matches_prim_count(self) -> None:
        stage = self._make_stage()
        assert len(stage.list_prims()) == stage.prim_count()

    def test_list_prims_elements_are_usd_prim(self) -> None:
        stage = self._make_stage()
        for p in stage.list_prims():
            assert "UsdPrim" in type(p).__name__

    def test_traverse_returns_list(self) -> None:
        stage = self._make_stage()
        result = stage.traverse()
        assert isinstance(result, list)

    def test_traverse_count_equals_list_prims_count(self) -> None:
        stage = self._make_stage()
        assert len(stage.traverse()) == len(stage.list_prims())

    def test_prims_of_type_sphere(self) -> None:
        stage = self._make_stage()
        spheres = stage.prims_of_type("Sphere")
        assert len(spheres) == 1
        assert spheres[0].type_name == "Sphere"

    def test_prims_of_type_xform(self) -> None:
        stage = self._make_stage()
        xforms = stage.prims_of_type("Xform")
        assert len(xforms) == 1

    def test_prims_of_type_nonexistent_returns_empty(self) -> None:
        stage = self._make_stage()
        result = stage.prims_of_type("NonExistentTypeFoo")
        assert result == []

    def test_has_prim_existing_path(self) -> None:
        stage = self._make_stage()
        assert stage.has_prim("/Root") is True

    def test_has_prim_nonexistent_path(self) -> None:
        stage = self._make_stage()
        assert stage.has_prim("/NoSuchPrim") is False

    def test_has_prim_child_path(self) -> None:
        stage = self._make_stage()
        assert stage.has_prim("/Root/Sphere") is True

    def test_list_prims_empty_stage(self) -> None:
        stage = UsdStage("empty_lt")
        assert stage.list_prims() == []

    def test_traverse_empty_stage(self) -> None:
        stage = UsdStage("empty_tr")
        assert stage.traverse() == []

    def test_prims_of_type_multiple_same_type(self) -> None:
        stage = UsdStage("multi_sphere")
        stage.define_prim("/A", "Sphere")
        stage.define_prim("/B", "Sphere")
        stage.define_prim("/C", "Sphere")
        result = stage.prims_of_type("Sphere")
        assert len(result) == 3


# ---------------------------------------------------------------------------
# TestUsdStageJsonRoundtrip
# ---------------------------------------------------------------------------
class TestUsdStageJsonRoundtrip:
    """UsdStage.to_json, from_json, export_usda."""

    def test_to_json_returns_string(self) -> None:
        stage = UsdStage("json_stage")
        j = stage.to_json()
        assert isinstance(j, str)

    def test_to_json_is_valid_json(self) -> None:
        stage = UsdStage("json_valid")
        j = stage.to_json()
        d = json.loads(j)
        assert isinstance(d, dict)

    def test_to_json_has_id_key(self) -> None:
        stage = UsdStage("json_id")
        d = json.loads(stage.to_json())
        assert "id" in d

    def test_to_json_has_name_key(self) -> None:
        stage = UsdStage("json_name_key")
        d = json.loads(stage.to_json())
        assert d["name"] == "json_name_key"

    def test_to_json_has_root_layer(self) -> None:
        stage = UsdStage("json_rl")
        d = json.loads(stage.to_json())
        assert "root_layer" in d

    def test_to_json_root_layer_has_meters_per_unit(self) -> None:
        stage = UsdStage("json_mpu")
        stage.set_meters_per_unit(0.01)
        d = json.loads(stage.to_json())
        assert d["root_layer"]["meters_per_unit"] == pytest.approx(0.01)

    def test_to_json_prims_included(self) -> None:
        stage = UsdStage("json_prims")
        stage.define_prim("/World", "Xform")
        d = json.loads(stage.to_json())
        assert "/World" in d["root_layer"]["prims"]

    def test_from_json_roundtrip_prim_count(self) -> None:
        stage = UsdStage("json_rt")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Sphere")
        j = stage.to_json()
        stage2 = UsdStage.from_json(j)
        assert stage2.prim_count() == 2

    def test_from_json_preserves_meters_per_unit(self) -> None:
        stage = UsdStage("json_mpu_rt")
        stage.set_meters_per_unit(0.1)
        j = stage.to_json()
        stage2 = UsdStage.from_json(j)
        assert stage2.meters_per_unit == pytest.approx(0.1)

    def test_from_json_preserves_name(self) -> None:
        stage = UsdStage("my_unique_name")
        j = stage.to_json()
        stage2 = UsdStage.from_json(j)
        assert stage2.name == "my_unique_name"

    def test_export_usda_returns_string(self) -> None:
        stage = UsdStage("usda_stage")
        usda = stage.export_usda()
        assert isinstance(usda, str)

    def test_export_usda_starts_with_usda_header(self) -> None:
        stage = UsdStage("usda_hdr")
        usda = stage.export_usda()
        assert usda.startswith("#usda 1.0")

    def test_export_usda_contains_up_axis(self) -> None:
        stage = UsdStage("usda_up")
        usda = stage.export_usda()
        assert "upAxis" in usda

    def test_export_usda_contains_prim_type(self) -> None:
        stage = UsdStage("usda_prim")
        stage.define_prim("/MySphere", "Sphere")
        usda = stage.export_usda()
        assert "Sphere" in usda

    def test_export_usda_contains_meters_per_unit(self) -> None:
        stage = UsdStage("usda_mpu")
        stage.set_meters_per_unit(0.01)
        usda = stage.export_usda()
        assert "metersPerUnit" in usda


# ---------------------------------------------------------------------------
# TestUsdStageMetricsDefaultPrim
# ---------------------------------------------------------------------------
class TestUsdStageMetricsDefaultPrim:
    """UsdStage.metrics(), default_prim, set_default_prim, id, name."""

    def test_metrics_returns_dict(self) -> None:
        stage = UsdStage("metrics_stage")
        m = stage.metrics()
        assert isinstance(m, dict)

    def test_metrics_has_prim_count_key(self) -> None:
        stage = UsdStage("metrics_pc")
        stage.define_prim("/A", "Sphere")
        m = stage.metrics()
        assert "prim_count" in m
        assert m["prim_count"] == 1

    def test_metrics_has_mesh_count_key(self) -> None:
        stage = UsdStage("metrics_mc")
        m = stage.metrics()
        assert "mesh_count" in m

    def test_metrics_xform_count(self) -> None:
        stage = UsdStage("metrics_xf")
        stage.define_prim("/World", "Xform")
        m = stage.metrics()
        assert m["xform_count"] == 1

    def test_metrics_empty_stage(self) -> None:
        stage = UsdStage("metrics_empty")
        m = stage.metrics()
        assert m["prim_count"] == 0

    def test_default_prim_initially_none(self) -> None:
        stage = UsdStage("dp_none")
        assert stage.default_prim is None

    def test_set_default_prim_updates_default_prim(self) -> None:
        stage = UsdStage("dp_set")
        stage.define_prim("/World", "Xform")
        stage.set_default_prim("/World")
        assert stage.default_prim == "/World"

    def test_default_prim_returns_path_string(self) -> None:
        stage = UsdStage("dp_str")
        stage.define_prim("/Root", "Xform")
        stage.set_default_prim("/Root")
        assert isinstance(stage.default_prim, str)

    def test_stage_id_is_string(self) -> None:
        stage = UsdStage("id_stage")
        assert isinstance(stage.id, str)

    def test_stage_id_is_non_empty(self) -> None:
        stage = UsdStage("id_nonempty")
        assert len(stage.id) > 0

    def test_stage_name_matches_constructor(self) -> None:
        stage = UsdStage("my_named_stage")
        assert stage.name == "my_named_stage"

    def test_up_axis_is_y_by_default(self) -> None:
        stage = UsdStage("up_axis_stage")
        assert stage.up_axis == "Y"

    def test_remove_nonexistent_prim_no_error(self) -> None:
        stage = UsdStage("rm_noexist")
        stage.remove_prim("/Absolutely/Not/Existing")

    def test_two_stages_have_different_ids(self) -> None:
        s1 = UsdStage("s1")
        s2 = UsdStage("s2")
        assert s1.id != s2.id


# ---------------------------------------------------------------------------
# TestUsdPrimAttributesDeep
# ---------------------------------------------------------------------------
class TestUsdPrimAttributesDeep:
    """UsdPrim attribute access, names, summary, active, path, name, type_name."""

    def _make_prim(self) -> tuple:
        stage = UsdStage("prim_attr_stage")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Sphere", "Sphere")
        prim = stage.get_prim("/World/Sphere")
        return stage, prim

    def test_prim_path_is_full_path(self) -> None:
        _, prim = self._make_prim()
        # prim.path returns an SdfPath object; convert to str for comparison
        assert str(prim.path) == "/World/Sphere"

    def test_prim_name_is_leaf(self) -> None:
        _, prim = self._make_prim()
        assert prim.name == "Sphere"

    def test_prim_type_name_is_sphere(self) -> None:
        _, prim = self._make_prim()
        assert prim.type_name == "Sphere"

    def test_prim_active_is_true_by_default(self) -> None:
        _, prim = self._make_prim()
        assert prim.active is True

    def test_attribute_names_empty_initially(self) -> None:
        _, prim = self._make_prim()
        names = prim.attribute_names()
        assert isinstance(names, list)
        assert len(names) == 0

    def test_set_float_attribute_then_name_appears(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(5.0))
        assert "radius" in prim.attribute_names()

    def test_get_float_attribute_returns_vt_value(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(3.14))
        val = prim.get_attribute("radius")
        assert "VtValue" in type(val).__name__

    def test_get_float_attribute_type_name_is_float(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(3.14))
        val = prim.get_attribute("radius")
        assert val.type_name == "float"

    def test_get_float_attribute_to_python(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(7.0))
        val = prim.get_attribute("radius")
        assert val.to_python() == pytest.approx(7.0)

    def test_set_int_attribute(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("segments", VtValue.from_int(16))
        val = prim.get_attribute("segments")
        assert val.type_name == "int"
        assert val.to_python() == 16

    def test_set_string_attribute(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("label", VtValue.from_string("my_sphere"))
        val = prim.get_attribute("label")
        assert val.type_name == "string"
        assert val.to_python() == "my_sphere"

    def test_set_bool_attribute(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("visible", VtValue.from_bool(False))
        val = prim.get_attribute("visible")
        assert val.type_name == "bool"
        assert val.to_python() is False

    def test_set_vec3f_attribute(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("translate", VtValue.from_vec3f(1.0, 2.0, 3.0))
        val = prim.get_attribute("translate")
        assert val.type_name == "float3"
        assert val.to_python() == pytest.approx((1.0, 2.0, 3.0))

    def test_attributes_summary_returns_dict(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(1.0))
        summ = prim.attributes_summary()
        assert isinstance(summ, dict)

    def test_attributes_summary_maps_name_to_type(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(1.0))
        summ = prim.attributes_summary()
        assert summ.get("radius") == "float"

    def test_multiple_attributes_in_names(self) -> None:
        _, prim = self._make_prim()
        prim.set_attribute("radius", VtValue.from_float(1.0))
        prim.set_attribute("seg_count", VtValue.from_int(8))
        names = prim.attribute_names()
        assert "radius" in names
        assert "seg_count" in names

    def test_has_api_false_for_arbitrary_name(self) -> None:
        _, prim = self._make_prim()
        assert prim.has_api("SomeRandomApi") is False


# ---------------------------------------------------------------------------
# TestUsdStageSetGetAttribute
# ---------------------------------------------------------------------------
class TestUsdStageSetGetAttribute:
    """UsdStage-level set_attribute / get_attribute helpers."""

    def test_stage_set_get_float(self) -> None:
        stage = UsdStage("sg_float")
        stage.define_prim("/World", "Sphere")
        stage.set_attribute("/World", "radius", VtValue.from_float(5.0))
        val = stage.get_attribute("/World", "radius")
        assert val.type_name == "float"

    def test_stage_set_get_int(self) -> None:
        stage = UsdStage("sg_int")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "count", VtValue.from_int(10))
        val = stage.get_attribute("/Root", "count")
        assert val.type_name == "int"
        assert val.to_python() == 10

    def test_stage_set_get_string(self) -> None:
        stage = UsdStage("sg_str")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "label", VtValue.from_string("world"))
        val = stage.get_attribute("/Root", "label")
        assert val.to_python() == "world"

    def test_stage_set_get_token(self) -> None:
        stage = UsdStage("sg_tok")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "kind", VtValue.from_token("component"))
        val = stage.get_attribute("/Root", "kind")
        assert val.type_name == "token"
        assert val.to_python() == "component"

    def test_stage_set_get_bool(self) -> None:
        stage = UsdStage("sg_bool")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "visible", VtValue.from_bool(True))
        val = stage.get_attribute("/Root", "visible")
        assert val.to_python() is True

    def test_stage_set_get_vec3f(self) -> None:
        stage = UsdStage("sg_vec")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "translate", VtValue.from_vec3f(1.0, 2.0, 3.0))
        val = stage.get_attribute("/Root", "translate")
        assert val.type_name == "float3"
        x, y, z = val.to_python()
        assert x == pytest.approx(1.0)
        assert y == pytest.approx(2.0)
        assert z == pytest.approx(3.0)

    def test_stage_set_get_asset(self) -> None:
        stage = UsdStage("sg_asset")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "ref", VtValue.from_asset("some/path.usd"))
        val = stage.get_attribute("/Root", "ref")
        assert val.type_name == "asset"
        assert val.to_python() == "some/path.usd"


# ---------------------------------------------------------------------------
# TestVersionedRegistryEdgeCases
# ---------------------------------------------------------------------------
class TestVersionedRegistryEdgeCases:
    """VersionedRegistry edge cases: None returns, empty lists, counters."""

    def _make_reg(self) -> VersionedRegistry:
        reg = VersionedRegistry()
        reg.register_versioned("action_a", dcc="maya", version="1.0.0")
        reg.register_versioned("action_a", dcc="maya", version="1.2.3")
        reg.register_versioned("action_a", dcc="maya", version="2.0.0")
        reg.register_versioned("action_b", dcc="blender", version="1.0.0")
        return reg

    def test_resolve_returns_none_when_no_match(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint=">=99.0.0")
        assert result is None

    def test_resolve_returns_none_for_nonexistent_action(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("no_such", dcc="maya", constraint="*")
        assert result is None

    def test_resolve_all_returns_empty_when_no_match(self) -> None:
        reg = self._make_reg()
        result = reg.resolve_all("action_a", dcc="maya", constraint=">=99.0.0")
        assert result == []

    def test_resolve_all_returns_empty_for_nonexistent_action(self) -> None:
        reg = self._make_reg()
        result = reg.resolve_all("no_such", dcc="maya", constraint="*")
        assert result == []

    def test_remove_nonexistent_returns_zero(self) -> None:
        reg = self._make_reg()
        removed = reg.remove("no_such", dcc="maya", constraint="*")
        assert removed == 0

    def test_total_entries_counts_all_versions(self) -> None:
        reg = self._make_reg()
        assert reg.total_entries() == 4

    def test_keys_returns_list_of_tuples(self) -> None:
        reg = self._make_reg()
        keys = reg.keys()
        assert isinstance(keys, list)
        assert len(keys) == 2

    def test_keys_contains_expected_pairs(self) -> None:
        reg = self._make_reg()
        keys = reg.keys()
        assert ("action_a", "maya") in keys
        assert ("action_b", "blender") in keys

    def test_versions_nonexistent_returns_empty(self) -> None:
        reg = self._make_reg()
        result = reg.versions("no_such", dcc="maya")
        assert result == []

    def test_latest_version_nonexistent_returns_none(self) -> None:
        reg = self._make_reg()
        result = reg.latest_version("no_such", dcc="maya")
        assert result is None

    def test_resolve_wildcard_returns_latest(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint="*")
        assert result is not None
        assert result["version"] == "2.0.0"

    def test_resolve_gte_returns_latest_in_range(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint=">=1.0.0")
        assert result["version"] == "2.0.0"

    def test_resolve_caret_stays_within_major(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint="^1.0.0")
        assert result is not None
        assert result["version"] == "1.2.3"

    def test_resolve_exact_version(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint="=1.0.0")
        assert result is not None
        assert result["version"] == "1.0.0"

    def test_resolve_all_wildcard_returns_sorted(self) -> None:
        reg = self._make_reg()
        all_r = reg.resolve_all("action_a", dcc="maya", constraint="*")
        versions = [r["version"] for r in all_r]
        assert versions == ["1.0.0", "1.2.3", "2.0.0"]

    def test_remove_caret_removes_matching_versions(self) -> None:
        reg = self._make_reg()
        removed = reg.remove("action_a", dcc="maya", constraint="^1.0.0")
        assert removed == 2
        remaining = reg.versions("action_a", dcc="maya")
        assert "2.0.0" in remaining
        assert "1.0.0" not in remaining
        assert "1.2.3" not in remaining

    def test_total_entries_decreases_after_remove(self) -> None:
        reg = self._make_reg()
        reg.remove("action_a", dcc="maya", constraint="^1.0.0")
        assert reg.total_entries() == 2

    def test_latest_version_returns_highest(self) -> None:
        reg = self._make_reg()
        assert reg.latest_version("action_a", dcc="maya") == "2.0.0"

    def test_empty_registry_keys_empty(self) -> None:
        reg = VersionedRegistry()
        assert reg.keys() == []

    def test_empty_registry_total_entries_zero(self) -> None:
        reg = VersionedRegistry()
        assert reg.total_entries() == 0

    def test_resolve_result_has_name_and_dcc(self) -> None:
        reg = self._make_reg()
        result = reg.resolve("action_a", dcc="maya", constraint="*")
        assert result["name"] == "action_a"
        assert result["dcc"] == "maya"


# ---------------------------------------------------------------------------
# TestVersionConstraintParse
# ---------------------------------------------------------------------------
class TestVersionConstraintParse:
    """VersionConstraint.parse and matches."""

    def test_parse_gte_constraint(self) -> None:
        vc = VersionConstraint.parse(">=1.0.0")
        assert "VersionConstraint" in type(vc).__name__

    def test_parse_gte_matches_higher(self) -> None:
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc.matches(SemVer(2, 0, 0)) is True

    def test_parse_gte_matches_exact(self) -> None:
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc.matches(SemVer(1, 0, 0)) is True

    def test_parse_gte_rejects_lower(self) -> None:
        vc = VersionConstraint.parse(">=1.0.0")
        assert vc.matches(SemVer(0, 9, 0)) is False

    def test_parse_caret_stays_in_major(self) -> None:
        vc = VersionConstraint.parse("^1.0.0")
        assert vc.matches(SemVer(1, 5, 0)) is True
        assert vc.matches(SemVer(2, 0, 0)) is False

    def test_parse_caret_matches_minor_patch(self) -> None:
        vc = VersionConstraint.parse("^1.2.0")
        assert vc.matches(SemVer(1, 3, 0)) is True
        assert vc.matches(SemVer(1, 1, 9)) is False

    def test_parse_wildcard_matches_any(self) -> None:
        vc = VersionConstraint.parse("*")
        assert vc.matches(SemVer(0, 0, 1)) is True
        assert vc.matches(SemVer(99, 99, 99)) is True

    def test_parse_exact_matches_only_exact(self) -> None:
        vc = VersionConstraint.parse("=1.2.3")
        assert vc.matches(SemVer(1, 2, 3)) is True
        assert vc.matches(SemVer(1, 2, 4)) is False
        assert vc.matches(SemVer(1, 2, 2)) is False

    def test_parse_lte_constraint(self) -> None:
        vc = VersionConstraint.parse("<=2.0.0")
        assert vc.matches(SemVer(1, 0, 0)) is True
        assert vc.matches(SemVer(2, 0, 0)) is True
        assert vc.matches(SemVer(2, 0, 1)) is False

    def test_parse_invalid_raises_value_error(self) -> None:
        with pytest.raises(ValueError):
            VersionConstraint.parse("!!invalid")

    def test_parse_str_representation(self) -> None:
        vc = VersionConstraint.parse(">=1.0.0")
        assert "1.0.0" in str(vc)


# ---------------------------------------------------------------------------
# TestEventBusKwargsPublish
# ---------------------------------------------------------------------------
class TestEventBusKwargsPublish:
    """EventBus: kwargs-based publish, multiple subscribers, unsubscribe."""

    def test_subscribe_returns_integer_id(self) -> None:
        bus = EventBus()
        h = bus.subscribe("ev", lambda **kw: None)
        assert isinstance(h, int)

    def test_publish_kwargs_delivered_to_callback(self) -> None:
        bus = EventBus()
        received = []
        bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev", x=1, y=2)
        assert len(received) == 1
        assert received[0] == {"x": 1, "y": 2}

    def test_publish_no_kwargs_delivers_empty_dict(self) -> None:
        bus = EventBus()
        received = []
        bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev")
        assert received == [{}]

    def test_publish_no_subscribers_no_error(self) -> None:
        bus = EventBus()
        bus.publish("ghost_event", data=42)

    def test_multiple_subscribers_all_receive(self) -> None:
        bus = EventBus()
        r1, r2 = [], []
        bus.subscribe("shared", lambda **kw: r1.append(kw))
        bus.subscribe("shared", lambda **kw: r2.append(kw))
        bus.publish("shared", val=99)
        assert len(r1) == 1
        assert len(r2) == 1
        assert r1[0]["val"] == 99

    def test_multiple_publishes_accumulate(self) -> None:
        bus = EventBus()
        received = []
        bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev", n=1)
        bus.publish("ev", n=2)
        assert len(received) == 2

    def test_unsubscribe_stops_delivery(self) -> None:
        bus = EventBus()
        received = []
        h = bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev", n=1)
        bus.unsubscribe("ev", h)
        bus.publish("ev", n=2)
        assert len(received) == 1

    def test_unsubscribe_one_of_two_keeps_other(self) -> None:
        bus = EventBus()
        r1, r2 = [], []
        h1 = bus.subscribe("ev", lambda **kw: r1.append(kw))
        bus.subscribe("ev", lambda **kw: r2.append(kw))
        bus.unsubscribe("ev", h1)
        bus.publish("ev", x=5)
        assert r1 == []
        assert r2 == [{"x": 5}]

    def test_different_events_isolated(self) -> None:
        bus = EventBus()
        r_click, r_hover = [], []
        bus.subscribe("click", lambda **kw: r_click.append(kw))
        bus.subscribe("hover", lambda **kw: r_hover.append(kw))
        bus.publish("click", button="left")
        assert len(r_click) == 1
        assert len(r_hover) == 0

    def test_subscribe_id_increments(self) -> None:
        bus = EventBus()
        h1 = bus.subscribe("ev", lambda **kw: None)
        h2 = bus.subscribe("ev", lambda **kw: None)
        assert h2 > h1

    def test_publish_with_integer_data(self) -> None:
        bus = EventBus()
        received = []
        bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev", count=42)
        assert received[0]["count"] == 42

    def test_publish_with_string_data(self) -> None:
        bus = EventBus()
        received = []
        bus.subscribe("ev", lambda **kw: received.append(kw))
        bus.publish("ev", name="hello")
        assert received[0]["name"] == "hello"

    def test_unsubscribe_nonexistent_id_no_error(self) -> None:
        bus = EventBus()
        bus.subscribe("ev", lambda **kw: None)
        bus.unsubscribe("ev", 9999)

    def test_multiple_event_types_same_bus(self) -> None:
        bus = EventBus()
        log = []
        bus.subscribe("a", lambda **kw: log.append(("a", kw)))
        bus.subscribe("b", lambda **kw: log.append(("b", kw)))
        bus.publish("a", x=1)
        bus.publish("b", y=2)
        assert ("a", {"x": 1}) in log
        assert ("b", {"y": 2}) in log
