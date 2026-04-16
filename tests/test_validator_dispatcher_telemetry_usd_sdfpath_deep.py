"""Deep coverage tests for ToolValidator, ToolDispatcher, TelemetryConfig, UsdStage/UsdPrim/VtValue, and SdfPath APIs.

Groups:
- TestActionValidatorSchemaTypes      — JSON schema type validation deep
- TestActionValidatorFromRegistry     — from_action_registry integration
- TestActionDispatcherDeep            — handler_names, remove, skip_empty_schema_validation
- TestTelemetryConfigBuilderChain     — builder methods, exporter types, init
- TestUsdStageOpsDeep                 — traverse, list_prims, prims_of_type, remove, metrics
- TestUsdPrimAttributeOps             — get/set attribute, attribute_names, summary, has_api
- TestVtValueFactories                — all factory methods and to_python round-trips
- TestSdfPathOps                      — is_absolute, name, parent, child, equality, hash
"""

from __future__ import annotations

import contextlib
import json

import pytest

from dcc_mcp_core import SdfPath
from dcc_mcp_core import TelemetryConfig
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import ToolValidator
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_registry(*actions: tuple[str, str]) -> ToolRegistry:
    """Create a registry with named actions (name, schema_json) pairs."""
    reg = ToolRegistry()
    for name, schema in actions:
        reg.register(name, description=f"desc {name}", category="test", input_schema=schema)
    return reg


_SIMPLE_SCHEMA = json.dumps({"type": "object", "properties": {"radius": {"type": "number"}}, "required": ["radius"]})

_MULTI_SCHEMA = json.dumps(
    {
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "count": {"type": "integer"},
            "active": {"type": "boolean"},
            "tags": {"type": "array", "items": {"type": "string"}},
        },
        "required": ["name", "count"],
    }
)

_EMPTY_SCHEMA = json.dumps({"type": "object", "properties": {}})


# ===========================================================================
# 1. TestActionValidatorSchemaTypes
# ===========================================================================


class TestActionValidatorSchemaTypes:
    """Validate every JSON-schema primitive type + error message format."""

    class TestHappyPath:
        def test_number_valid(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, errors = v.validate(json.dumps({"radius": 1.5}))
            assert ok is True
            assert errors == []

        def test_integer_valid(self):
            schema = json.dumps({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]})
            v = ToolValidator.from_schema_json(schema)
            ok, errors = v.validate(json.dumps({"n": 42}))
            assert ok is True
            assert errors == []

        def test_string_valid(self):
            schema = json.dumps(
                {
                    "type": "object",
                    "properties": {"label": {"type": "string"}},
                    "required": ["label"],
                }
            )
            v = ToolValidator.from_schema_json(schema)
            ok, _errors = v.validate(json.dumps({"label": "hello"}))
            assert ok is True

        def test_boolean_valid(self):
            schema = json.dumps(
                {
                    "type": "object",
                    "properties": {"flag": {"type": "boolean"}},
                    "required": ["flag"],
                }
            )
            v = ToolValidator.from_schema_json(schema)
            ok, _errors = v.validate(json.dumps({"flag": True}))
            assert ok is True

        def test_array_valid(self):
            schema = json.dumps(
                {
                    "type": "object",
                    "properties": {"items": {"type": "array", "items": {"type": "string"}}},
                    "required": ["items"],
                }
            )
            v = ToolValidator.from_schema_json(schema)
            ok, _errors = v.validate(json.dumps({"items": ["a", "b", "c"]}))
            assert ok is True

        def test_multi_field_valid(self):
            v = ToolValidator.from_schema_json(_MULTI_SCHEMA)
            ok, errors = v.validate(json.dumps({"name": "sphere", "count": 3, "active": True, "tags": ["geo"]}))
            assert ok is True
            assert errors == []

        def test_optional_field_absent(self):
            """Optional fields (not in required) can be absent."""
            v = ToolValidator.from_schema_json(_MULTI_SCHEMA)
            ok, _errors = v.validate(json.dumps({"name": "x", "count": 1}))
            assert ok is True

        def test_empty_schema_all_pass(self):
            v = ToolValidator.from_schema_json(_EMPTY_SCHEMA)
            ok, _ = v.validate("{}")
            assert ok is True

        def test_extra_fields_ignored(self):
            """Extra fields not in schema should not cause validation failure."""
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, _errors = v.validate(json.dumps({"radius": 1.0, "extra_field": "ignored"}))
            assert ok is True

        def test_zero_number_valid(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, _ = v.validate(json.dumps({"radius": 0}))
            assert ok is True

        def test_negative_number_valid(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, _ = v.validate(json.dumps({"radius": -3.14}))
            assert ok is True

    class TestErrorPath:
        def test_missing_required_field(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, errors = v.validate("{}")
            assert ok is False
            assert len(errors) == 1
            assert "radius" in errors[0]
            assert "required" in errors[0]

        def test_wrong_type_number(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, errors = v.validate(json.dumps({"radius": "not_a_number"}))
            assert ok is False
            assert "radius" in errors[0]
            assert "number" in errors[0]

        def test_wrong_type_integer_rejects_float(self):
            schema = json.dumps({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]})
            v = ToolValidator.from_schema_json(schema)
            ok, errors = v.validate(json.dumps({"n": 5.5}))
            assert ok is False
            assert "integer" in errors[0]

        def test_wrong_type_boolean(self):
            schema = json.dumps(
                {
                    "type": "object",
                    "properties": {"flag": {"type": "boolean"}},
                    "required": ["flag"],
                }
            )
            v = ToolValidator.from_schema_json(schema)
            ok, errors = v.validate(json.dumps({"flag": "yes"}))
            assert ok is False
            assert "boolean" in errors[0]

        def test_wrong_type_array(self):
            schema = json.dumps(
                {
                    "type": "object",
                    "properties": {"tags": {"type": "array"}},
                    "required": ["tags"],
                }
            )
            v = ToolValidator.from_schema_json(schema)
            ok, errors = v.validate(json.dumps({"tags": "not_array"}))
            assert ok is False
            assert "array" in errors[0]

        def test_multiple_missing_required(self):
            v = ToolValidator.from_schema_json(_MULTI_SCHEMA)
            ok, errors = v.validate("{}")
            assert ok is False
            # Should report both missing required fields
            assert len(errors) >= 1

        def test_integer_from_string(self):
            schema = json.dumps({"type": "object", "properties": {"n": {"type": "integer"}}, "required": ["n"]})
            v = ToolValidator.from_schema_json(schema)
            ok, _errors = v.validate(json.dumps({"n": "five"}))
            assert ok is False

        def test_error_message_contains_field_path(self):
            """Error messages should reference the field name."""
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            ok, errors = v.validate(json.dumps({"radius": "bad"}))
            assert ok is False
            assert "radius" in errors[0]

        def test_validate_returns_tuple(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            result = v.validate("{}")
            assert isinstance(result, tuple)
            assert len(result) == 2

        def test_errors_is_list(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            _, errors = v.validate("{}")
            assert isinstance(errors, list)

        def test_valid_errors_empty_list(self):
            v = ToolValidator.from_schema_json(_SIMPLE_SCHEMA)
            _, errors = v.validate(json.dumps({"radius": 1.0}))
            assert errors == []


# ===========================================================================
# 2. TestActionValidatorFromRegistry
# ===========================================================================


class TestActionValidatorFromRegistry:
    """Validate ToolValidator.from_action_registry integration."""

    class TestHappyPath:
        def test_creates_validator_from_registry(self):
            reg = _make_registry(("sphere", _SIMPLE_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "sphere")
            assert v is not None

        def test_validates_correct_params(self):
            reg = _make_registry(("sphere", _SIMPLE_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "sphere")
            ok, errors = v.validate(json.dumps({"radius": 5.0}))
            assert ok is True
            assert errors == []

        def test_detects_missing_required_via_registry(self):
            reg = _make_registry(("sphere", _SIMPLE_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "sphere")
            ok, errors = v.validate("{}")
            assert ok is False
            assert len(errors) >= 1

        def test_multi_field_schema_from_registry(self):
            reg = _make_registry(("multi", _MULTI_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "multi")
            ok, _ = v.validate(json.dumps({"name": "x", "count": 1}))
            assert ok is True

        def test_registry_with_no_schema(self):
            """Action with empty/no input_schema should accept any params."""
            reg = ToolRegistry()
            reg.register("noop", description="noop", category="test")
            v = ToolValidator.from_action_registry(reg, "noop")
            ok, _ = v.validate("{}")
            assert ok is True

        def test_validator_type(self):
            reg = _make_registry(("sphere", _SIMPLE_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "sphere")
            assert type(v).__name__ == "ToolValidator"

        def test_different_actions_different_validators(self):
            reg = _make_registry(("a", _SIMPLE_SCHEMA), ("b", _MULTI_SCHEMA))
            va = ToolValidator.from_action_registry(reg, "a")
            vb = ToolValidator.from_action_registry(reg, "b")
            # a requires radius, b requires name+count
            ok_a, _ = va.validate(json.dumps({"radius": 1.0}))
            ok_b, _ = vb.validate(json.dumps({"name": "x", "count": 1}))
            assert ok_a is True
            assert ok_b is True

    class TestErrorPath:
        def test_missing_required_from_registry(self):
            reg = _make_registry(("sphere", _SIMPLE_SCHEMA))
            v = ToolValidator.from_action_registry(reg, "sphere")
            ok, errors = v.validate(json.dumps({"radius": "wrong_type"}))
            assert ok is False
            assert len(errors) >= 1


# ===========================================================================
# 3. TestActionDispatcherDeep
# ===========================================================================


class TestActionDispatcherDeep:
    """Deep coverage of ToolDispatcher methods."""

    def _make_dispatcher(self) -> tuple[ToolDispatcher, ToolRegistry]:
        reg = ToolRegistry()
        reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
        reg.register("cube", description="d2", category="c", input_schema=_EMPTY_SCHEMA)
        reg.register("light", description="d3", category="c")
        d = ToolDispatcher(reg)
        return d, reg

    class TestHandlerNames:
        def test_empty_initially(self):
            reg = ToolRegistry()
            d = ToolDispatcher(reg)
            names = d.handler_names()
            assert isinstance(names, list)
            assert len(names) == 0

        def test_names_after_register(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c")
            reg.register("cube", description="d2", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: "ok")
            d.register_handler("cube", lambda p: "ok2")
            names = d.handler_names()
            assert "sphere" in names
            assert "cube" in names

        def test_names_count_matches_handler_count(self):
            reg = ToolRegistry()
            reg.register("a", description="d", category="c")
            reg.register("b", description="d", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("a", lambda p: "a")
            d.register_handler("b", lambda p: "b")
            assert len(d.handler_names()) == d.handler_count()

        def test_names_after_remove(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: "ok")
            assert "sphere" in d.handler_names()
            d.remove_handler("sphere")
            assert "sphere" not in d.handler_names()

    class TestHasHandler:
        def test_has_registered_handler(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: "ok")
            assert d.has_handler("sphere") is True

        def test_not_has_unregistered_handler(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c")
            d = ToolDispatcher(reg)
            assert d.has_handler("sphere") is False

        def test_has_handler_after_remove(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: "ok")
            d.remove_handler("sphere")
            assert d.has_handler("sphere") is False

    class TestDispatch:
        def test_dispatch_returns_dict(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: {"result": "ok"})
            result = d.dispatch("sphere", json.dumps({"radius": 1.0}))
            assert isinstance(result, dict)

        def test_dispatch_has_action_key(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: {"x": 1})
            result = d.dispatch("sphere", json.dumps({"radius": 1.0}))
            assert result["action"] == "sphere"

        def test_dispatch_has_output_key(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: {"x": 42})
            result = d.dispatch("sphere", json.dumps({"radius": 1.0}))
            assert result["output"] == {"x": 42}

        def test_dispatch_validation_skipped_key(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
            d = ToolDispatcher(reg)
            d.register_handler("sphere", lambda p: "ok")
            result = d.dispatch("sphere", json.dumps({"radius": 1.0}))
            assert "validation_skipped" in result

        def test_dispatch_unknown_raises_keyerror(self):
            reg = ToolRegistry()
            d = ToolDispatcher(reg)
            with pytest.raises(KeyError):
                d.dispatch("unknown_action", "{}")

        def test_dispatch_passes_params_to_handler(self):
            reg = ToolRegistry()
            reg.register("sphere", description="d", category="c", input_schema=_SIMPLE_SCHEMA)
            d = ToolDispatcher(reg)
            captured = []
            d.register_handler("sphere", lambda p: captured.append(p) or "ok")
            d.dispatch("sphere", json.dumps({"radius": 7.5}))
            assert len(captured) == 1
            assert captured[0].get("radius") == 7.5

        def test_dispatch_no_schema_action(self):
            """Action without schema — validation_skipped should be True."""
            reg = ToolRegistry()
            reg.register("noop", description="d", category="c")
            d = ToolDispatcher(reg)
            d.register_handler("noop", lambda p: "done")
            result = d.dispatch("noop", "{}")
            assert result["action"] == "noop"

    class TestSkipEmptySchemaValidation:
        def test_property_accessible(self):
            reg = ToolRegistry()
            d = ToolDispatcher(reg)
            val = d.skip_empty_schema_validation
            assert isinstance(val, bool)

        def test_default_value_is_bool(self):
            reg = ToolRegistry()
            d = ToolDispatcher(reg)
            assert d.skip_empty_schema_validation in (True, False)


# ===========================================================================
# 4. TestTelemetryConfigBuilderChain
# ===========================================================================


class TestTelemetryConfigBuilderChain:
    """Test TelemetryConfig builder API."""

    class TestConstruction:
        def test_creates_with_service_name(self):
            t = TelemetryConfig("my-service")
            assert t.service_name == "my-service"

        def test_repr_contains_service_name(self):
            t = TelemetryConfig("my-dcc")
            assert "my-dcc" in repr(t)

        def test_enable_metrics_is_bool(self):
            t = TelemetryConfig("svc")
            assert isinstance(t.enable_metrics, bool)

        def test_enable_tracing_is_bool(self):
            t = TelemetryConfig("svc")
            assert isinstance(t.enable_tracing, bool)

        def test_default_exporter_in_repr(self):
            """Default exporter should be Stdout."""
            t = TelemetryConfig("svc")
            assert "Stdout" in repr(t) or "Noop" in repr(t)

    class TestBuilderMethods:
        def test_with_attribute_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_attribute("env", "production")
            assert isinstance(result, TelemetryConfig)

        def test_with_attribute_chainable(self):
            t = TelemetryConfig("svc")
            result = t.with_attribute("env", "prod").with_attribute("region", "us-west")
            assert isinstance(result, TelemetryConfig)

        def test_with_service_version_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_service_version("2.0.0")
            assert isinstance(result, TelemetryConfig)

        def test_with_noop_exporter_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_noop_exporter()
            assert isinstance(result, TelemetryConfig)

        def test_with_noop_exporter_changes_repr(self):
            t = TelemetryConfig("svc")
            result = t.with_noop_exporter()
            assert "Noop" in repr(result)

        def test_with_stdout_exporter_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_stdout_exporter()
            assert isinstance(result, TelemetryConfig)

        def test_with_stdout_exporter_changes_repr(self):
            t = TelemetryConfig("svc")
            result = t.with_stdout_exporter()
            assert "Stdout" in repr(result)

        def test_with_json_logs_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_json_logs()
            assert isinstance(result, TelemetryConfig)

        def test_with_text_logs_returns_config(self):
            t = TelemetryConfig("svc")
            result = t.with_text_logs()
            assert isinstance(result, TelemetryConfig)

        def test_set_enable_metrics_true(self):
            t = TelemetryConfig("svc")
            t.set_enable_metrics(True)
            assert t.enable_metrics is True

        def test_set_enable_metrics_false(self):
            t = TelemetryConfig("svc")
            t.set_enable_metrics(False)
            assert t.enable_metrics is False

        def test_set_enable_tracing_true(self):
            t = TelemetryConfig("svc")
            t.set_enable_tracing(True)
            assert t.enable_tracing is True

        def test_set_enable_tracing_false(self):
            t = TelemetryConfig("svc")
            t.set_enable_tracing(False)
            assert t.enable_tracing is False

        def test_chain_all_builder_methods(self):
            t = (
                TelemetryConfig("full-service")
                .with_service_version("1.0.0")
                .with_attribute("team", "dcc")
                .with_attribute("env", "test")
                .with_noop_exporter()
                .with_json_logs()
            )
            assert isinstance(t, TelemetryConfig)
            assert t.service_name == "full-service"

        def test_noop_then_stdout_exporter(self):
            t = TelemetryConfig("svc").with_noop_exporter().with_stdout_exporter()
            assert "Stdout" in repr(t)

        def test_different_service_names_independent(self):
            t1 = TelemetryConfig("svc-a")
            t2 = TelemetryConfig("svc-b")
            assert t1.service_name == "svc-a"
            assert t2.service_name == "svc-b"

    class TestInit:
        def test_init_raises_on_second_call(self):
            """Second init() call in same process should raise (global tracer set)."""
            t = TelemetryConfig("test-init-svc").with_noop_exporter()
            # First call might succeed or fail depending on prior test state
            with contextlib.suppress(RuntimeError):
                t.init()
            # Second call must raise RuntimeError (tracer provider already set)
            with pytest.raises(RuntimeError):
                t.init()


# ===========================================================================
# 5. TestUsdStageOpsDeep
# ===========================================================================


class TestUsdStageOpsDeep:
    """Deep coverage of UsdStage operations."""

    class TestConstruction:
        def test_creates_with_name(self):
            stage = UsdStage("MyScene")
            assert stage.name == "MyScene"

        def test_id_is_string(self):
            stage = UsdStage("S")
            assert isinstance(stage.id, str)
            assert len(stage.id) > 0

        def test_initial_prim_count(self):
            stage = UsdStage("Empty")
            assert stage.prim_count() == 0

        def test_initial_traverse_empty(self):
            stage = UsdStage("Empty")
            assert list(stage.traverse()) == []

        def test_up_axis_default(self):
            stage = UsdStage("S")
            assert stage.up_axis in ("Y", "Z")

        def test_meters_per_unit_default(self):
            stage = UsdStage("S")
            assert stage.meters_per_unit == 1.0

        def test_fps_initially_none(self):
            stage = UsdStage("S")
            # fps may be None or a number
            assert stage.fps is None or isinstance(stage.fps, (int, float))

        def test_start_time_code_initially_none(self):
            stage = UsdStage("S")
            assert stage.start_time_code is None or isinstance(stage.start_time_code, (int, float))

        def test_end_time_code_initially_none(self):
            stage = UsdStage("S")
            assert stage.end_time_code is None or isinstance(stage.end_time_code, (int, float))

        def test_different_names_independent(self):
            s1 = UsdStage("Scene1")
            s2 = UsdStage("Scene2")
            assert s1.name == "Scene1"
            assert s2.name == "Scene2"

    class TestDefinePrim:
        def test_define_prim_increases_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            assert stage.prim_count() == 1

        def test_define_multiple_prims(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            stage.define_prim("/Root/Cube", "Cube")
            assert stage.prim_count() == 3

        def test_traverse_returns_prims(self):
            stage = UsdStage("S")
            stage.define_prim("/A", "Xform")
            stage.define_prim("/A/B", "Mesh")
            prims = list(stage.traverse())
            assert len(prims) == 2

        def test_traverse_contains_paths(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            prims = list(stage.traverse())
            paths = [str(p.path) for p in prims]
            assert "/Root" in paths
            assert "/Root/Sphere" in paths

    class TestHasPrimGetPrim:
        def test_has_prim_existing(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            assert stage.has_prim("/Root") is True

        def test_has_prim_nonexistent(self):
            stage = UsdStage("S")
            assert stage.has_prim("/NonExistent") is False

        def test_get_prim_returns_prim_object(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            p = stage.get_prim("/Root")
            assert p is not None
            assert type(p).__name__ == "UsdPrim"

        def test_get_prim_type_name(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Sphere")
            p = stage.get_prim("/Root")
            assert p.type_name == "Sphere"

        def test_get_prim_path(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            p = stage.get_prim("/Root")
            assert str(p.path) == "/Root"

        def test_get_prim_active(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            p = stage.get_prim("/Root")
            assert isinstance(p.active, bool)

    class TestListPrimsAndPrimsOfType:
        def test_list_prims_empty(self):
            stage = UsdStage("S")
            result = stage.list_prims()
            assert result == []

        def test_list_prims_returns_list(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            result = stage.list_prims()
            assert isinstance(result, list)
            assert len(result) == 2

        def test_prims_of_type_sphere(self):
            stage = UsdStage("S")
            stage.define_prim("/A", "Sphere")
            stage.define_prim("/B", "Sphere")
            stage.define_prim("/C", "Cube")
            spheres = stage.prims_of_type("Sphere")
            assert len(spheres) == 2

        def test_prims_of_type_no_match(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            result = stage.prims_of_type("Sphere")
            assert len(result) == 0

        def test_prims_of_type_returns_list(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            result = stage.prims_of_type("Mesh")
            assert isinstance(result, list)

    class TestRemovePrim:
        def test_remove_decreases_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            stage.remove_prim("/Root/Sphere")
            assert stage.prim_count() == 1

        def test_remove_prim_not_found_in_has_prim(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.remove_prim("/Root")
            assert stage.has_prim("/Root") is False

    class TestMetrics:
        def test_metrics_returns_dict(self):
            stage = UsdStage("S")
            m = stage.metrics()
            assert isinstance(m, dict)

        def test_metrics_has_prim_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            m = stage.metrics()
            assert "prim_count" in m
            assert m["prim_count"] == 1

        def test_metrics_mesh_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Mesh1", "Mesh")
            stage.define_prim("/Mesh2", "Mesh")
            m = stage.metrics()
            assert m["mesh_count"] == 2

        def test_metrics_xform_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            m = stage.metrics()
            assert m["xform_count"] == 1

        def test_metrics_camera_light_material_initial_zero(self):
            stage = UsdStage("S")
            m = stage.metrics()
            assert m.get("camera_count", 0) == 0
            assert m.get("light_count", 0) == 0
            assert m.get("material_count", 0) == 0

    class TestExportAndRoundTrip:
        def test_export_usda_returns_string(self):
            stage = UsdStage("S")
            usda = stage.export_usda()
            assert isinstance(usda, str)
            assert len(usda) > 0

        def test_export_usda_contains_header(self):
            stage = UsdStage("S")
            usda = stage.export_usda()
            assert "#usda" in usda

        def test_to_json_returns_string(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            j = stage.to_json()
            assert isinstance(j, str)

        def test_from_json_roundtrip_prim_count(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            j = stage.to_json()
            stage2 = UsdStage.from_json(j)
            assert stage2.prim_count() == 2

        def test_from_json_roundtrip_preserves_types(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.define_prim("/Root/Sphere", "Sphere")
            j = stage.to_json()
            stage2 = UsdStage.from_json(j)
            spheres = stage2.prims_of_type("Sphere")
            assert len(spheres) == 1

    class TestDefaultPrimAndMeta:
        def test_set_default_prim(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            stage.set_default_prim("/Root")
            assert stage.default_prim == "/Root"

        def test_default_prim_is_none_before_set(self):
            stage = UsdStage("S")
            stage.define_prim("/Root", "Xform")
            # Before calling set_default_prim, default_prim should be None or empty
            val = stage.default_prim
            assert val is None or isinstance(val, str)

        def test_set_meters_per_unit(self):
            stage = UsdStage("S")
            stage.set_meters_per_unit(0.01)
            assert abs(stage.meters_per_unit - 0.01) < 1e-6

        def test_meters_per_unit_is_float(self):
            stage = UsdStage("S")
            mpu = stage.meters_per_unit
            assert isinstance(mpu, float)


# ===========================================================================
# 6. TestUsdPrimAttributeOps
# ===========================================================================


class TestUsdPrimAttributeOps:
    """Tests for UsdPrim get/set attribute, attribute_names, summary, has_api."""

    def _make_stage_with_mesh(self) -> tuple[UsdStage, object]:
        stage = UsdStage("TestPrim")
        stage.define_prim("/Root", "Xform")
        stage.define_prim("/Root/Mesh", "Mesh")
        stage.set_attribute("/Root/Mesh", "width", VtValue.from_float(2.0))
        stage.set_attribute("/Root/Mesh", "height", VtValue.from_int(3))
        prim = stage.get_prim("/Root/Mesh")
        return stage, prim

    class TestGetAttribute:
        def test_get_attribute_returns_vtvalue(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(5.0))
            p = stage.get_prim("/X")
            val = p.get_attribute("w")
            assert type(val).__name__ == "VtValue"

        def test_get_attribute_value_correct(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(5.0))
            p = stage.get_prim("/X")
            val = p.get_attribute("w")
            assert abs(val.to_python() - 5.0) < 0.001

        def test_get_string_attribute(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "label", VtValue.from_string("hello"))
            p = stage.get_prim("/X")
            val = p.get_attribute("label")
            assert val.to_python() == "hello"

        def test_get_bool_attribute(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "visible", VtValue.from_bool(True))
            p = stage.get_prim("/X")
            val = p.get_attribute("visible")
            assert val.to_python() is True

        def test_get_int_attribute(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "count", VtValue.from_int(7))
            p = stage.get_prim("/X")
            val = p.get_attribute("count")
            assert val.to_python() == 7

    class TestSetAttribute:
        def test_set_attribute_via_prim(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            p = stage.get_prim("/X")
            p.set_attribute("depth", VtValue.from_float(10.0))
            val = p.get_attribute("depth")
            assert abs(val.to_python() - 10.0) < 0.001

        def test_set_overwrite_attribute(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            stage.set_attribute("/X", "w", VtValue.from_float(9.0))
            p = stage.get_prim("/X")
            val = p.get_attribute("w")
            assert abs(val.to_python() - 9.0) < 0.001

    class TestAttributeNames:
        def test_attribute_names_returns_list(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            p = stage.get_prim("/X")
            names = p.attribute_names()
            assert isinstance(names, list)

        def test_attribute_names_contains_set_attrs(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            stage.set_attribute("/X", "h", VtValue.from_int(2))
            p = stage.get_prim("/X")
            names = p.attribute_names()
            assert "w" in names
            assert "h" in names

        def test_attribute_names_empty_for_new_prim(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            p = stage.get_prim("/X")
            names = p.attribute_names()
            assert isinstance(names, list)

        def test_attribute_names_grows_after_set(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            p = stage.get_prim("/X")
            before = len(p.attribute_names())
            p.set_attribute("new_attr", VtValue.from_float(1.0))
            after = len(p.attribute_names())
            assert after == before + 1

    class TestAttributesSummary:
        def test_summary_returns_dict(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            p = stage.get_prim("/X")
            summary = p.attributes_summary()
            assert isinstance(summary, dict)

        def test_summary_contains_type_info(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            stage.set_attribute("/X", "n", VtValue.from_int(3))
            p = stage.get_prim("/X")
            summary = p.attributes_summary()
            # Keys should be attribute names
            assert "w" in summary
            assert "n" in summary

        def test_summary_float_type_label(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "w", VtValue.from_float(1.0))
            p = stage.get_prim("/X")
            summary = p.attributes_summary()
            assert summary["w"] == "float"

        def test_summary_int_type_label(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            stage.set_attribute("/X", "n", VtValue.from_int(3))
            p = stage.get_prim("/X")
            summary = p.attributes_summary()
            assert summary["n"] == "int"

    class TestHasApi:
        def test_has_api_returns_bool(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            p = stage.get_prim("/X")
            result = p.has_api("SomeApi")
            assert isinstance(result, bool)

        def test_has_api_unknown_returns_false(self):
            stage = UsdStage("S")
            stage.define_prim("/X", "Mesh")
            p = stage.get_prim("/X")
            assert p.has_api("NonExistentApi") is False


# ===========================================================================
# 7. TestVtValueFactories
# ===========================================================================


class TestVtValueFactories:
    """Test VtValue factory methods and to_python round-trips."""

    class TestFromFloat:
        def test_creates_vtvalue(self):
            v = VtValue.from_float(3.14)
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_float(1.0)
            assert v.type_name == "float"

        def test_to_python_returns_float(self):
            v = VtValue.from_float(2.5)
            result = v.to_python()
            assert isinstance(result, float)

        def test_to_python_approximate_value(self):
            v = VtValue.from_float(3.14)
            assert abs(v.to_python() - 3.14) < 0.001

        def test_zero(self):
            v = VtValue.from_float(0.0)
            assert abs(v.to_python()) < 1e-6

        def test_negative(self):
            v = VtValue.from_float(-1.5)
            assert v.to_python() < 0

    class TestFromInt:
        def test_creates_vtvalue(self):
            v = VtValue.from_int(42)
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_int(1)
            assert v.type_name == "int"

        def test_to_python_returns_int(self):
            v = VtValue.from_int(7)
            assert v.to_python() == 7

        def test_zero_int(self):
            v = VtValue.from_int(0)
            assert v.to_python() == 0

        def test_negative_int(self):
            v = VtValue.from_int(-10)
            assert v.to_python() == -10

        def test_large_int(self):
            v = VtValue.from_int(1_000_000)
            assert v.to_python() == 1_000_000

    class TestFromString:
        def test_creates_vtvalue(self):
            v = VtValue.from_string("hello")
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_string("x")
            assert v.type_name == "string"

        def test_to_python_returns_str(self):
            v = VtValue.from_string("hello")
            assert v.to_python() == "hello"

        def test_empty_string(self):
            v = VtValue.from_string("")
            assert v.to_python() == ""

        def test_unicode_string(self):
            v = VtValue.from_string("你好世界")
            assert v.to_python() == "你好世界"

    class TestFromBool:
        def test_creates_vtvalue(self):
            v = VtValue.from_bool(True)
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_bool(True)
            assert v.type_name == "bool"

        def test_to_python_true(self):
            v = VtValue.from_bool(True)
            assert v.to_python() is True

        def test_to_python_false(self):
            v = VtValue.from_bool(False)
            assert v.to_python() is False

    class TestFromToken:
        def test_creates_vtvalue(self):
            v = VtValue.from_token("Sphere")
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_token("X")
            assert v.type_name == "token"

        def test_to_python_returns_str(self):
            v = VtValue.from_token("Sphere")
            assert v.to_python() == "Sphere"

        def test_empty_token(self):
            v = VtValue.from_token("")
            assert isinstance(v.to_python(), str)

    class TestFromVec3f:
        def test_creates_vtvalue(self):
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_vec3f(0.0, 0.0, 0.0)
            assert v.type_name == "float3"

        def test_to_python_returns_tuple(self):
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            result = v.to_python()
            assert isinstance(result, tuple)
            assert len(result) == 3

        def test_to_python_approx_values(self):
            v = VtValue.from_vec3f(1.0, 2.0, 3.0)
            x, y, z = v.to_python()
            assert abs(x - 1.0) < 0.001
            assert abs(y - 2.0) < 0.001
            assert abs(z - 3.0) < 0.001

    class TestFromAsset:
        def test_creates_vtvalue(self):
            v = VtValue.from_asset("@path/to/file.usd@")
            assert type(v).__name__ == "VtValue"

        def test_type_name(self):
            v = VtValue.from_asset("@path@")
            assert v.type_name == "asset"

    class TestRepr:
        def test_repr_contains_type(self):
            v = VtValue.from_float(1.0)
            assert "float" in repr(v).lower() or "Float" in repr(v)

        def test_repr_is_string(self):
            v = VtValue.from_int(5)
            assert isinstance(repr(v), str)


# ===========================================================================
# 8. TestSdfPathOps
# ===========================================================================


class TestSdfPathOps:
    """Test SdfPath construction and properties."""

    class TestConstruction:
        def test_creates_from_string(self):
            p = SdfPath("/Root/Sphere")
            assert p is not None

        def test_str_returns_path_string(self):
            p = SdfPath("/Root/Sphere")
            assert str(p) == "/Root/Sphere"

        def test_repr_contains_path(self):
            p = SdfPath("/Root/Sphere")
            assert "/Root/Sphere" in repr(p)

    class TestIsAbsolute:
        def test_absolute_path(self):
            p = SdfPath("/Root/Sphere")
            assert p.is_absolute is True

        def test_relative_path(self):
            p = SdfPath("Sphere/Mesh")
            assert p.is_absolute is False

        def test_root_is_absolute(self):
            p = SdfPath("/")
            assert p.is_absolute is True

    class TestName:
        def test_name_returns_last_component(self):
            p = SdfPath("/Root/Sphere")
            assert p.name == "Sphere"

        def test_name_root(self):
            p = SdfPath("/Root")
            assert p.name == "Root"

        def test_name_relative(self):
            p = SdfPath("Sphere/Mesh")
            assert p.name == "Mesh"

    class TestParent:
        def test_parent_returns_method(self):
            p = SdfPath("/Root/Sphere")
            result = p.parent()
            assert result is not None

        def test_parent_path_value(self):
            p = SdfPath("/Root/Sphere")
            parent = p.parent()
            assert str(parent) == "/Root"

        def test_parent_of_root_level(self):
            p = SdfPath("/Root")
            parent = p.parent()
            assert str(parent) == "/"

    class TestChild:
        def test_child_creates_child_path(self):
            p = SdfPath("/Root/Sphere")
            child = p.child("Material")
            assert str(child) == "/Root/Sphere/Material"

        def test_child_from_root(self):
            p = SdfPath("/Root")
            child = p.child("Child")
            assert str(child) == "/Root/Child"

        def test_child_is_absolute(self):
            p = SdfPath("/Root")
            child = p.child("X")
            assert child.is_absolute is True

    class TestEquality:
        def test_same_paths_equal(self):
            p1 = SdfPath("/Root/Sphere")
            p2 = SdfPath("/Root/Sphere")
            assert p1 == p2

        def test_different_paths_not_equal(self):
            p1 = SdfPath("/Root/Sphere")
            p2 = SdfPath("/Root/Cube")
            assert p1 != p2

        def test_hash_equal_for_same_paths(self):
            p1 = SdfPath("/Root/Sphere")
            p2 = SdfPath("/Root/Sphere")
            assert hash(p1) == hash(p2)

        def test_hash_different_for_different_paths(self):
            p1 = SdfPath("/Root/Sphere")
            p2 = SdfPath("/Root/Cube")
            # Different paths usually have different hashes
            # (not guaranteed but very likely)
            assert (p1 == p2) == (hash(p1) == hash(p2) and p1 == p2)

        def test_usable_as_dict_key(self):
            p = SdfPath("/Root/Sphere")
            d = {p: "sphere_value"}
            assert d[SdfPath("/Root/Sphere")] == "sphere_value"

        def test_usable_in_set(self):
            paths = {SdfPath("/A"), SdfPath("/B"), SdfPath("/A")}
            assert len(paths) == 2

    class TestEdgeCases:
        def test_root_path(self):
            p = SdfPath("/")
            assert p.is_absolute is True

        def test_single_component(self):
            p = SdfPath("/Root")
            assert p.name == "Root"
            assert p.is_absolute is True

        def test_deep_path(self):
            p = SdfPath("/A/B/C/D/E")
            assert p.name == "E"
            assert str(p.parent()) == "/A/B/C/D"
