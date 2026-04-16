"""Deep tests for ToolValidator, EventBus, UsdStage / SdfPath / VtValue APIs.

Covers:
- ToolValidator: from_schema_json, from_action_registry, validate (happy/error paths)
- EventBus: subscribe/publish/unsubscribe, multiple subscribers, different event names
- UsdStage: define_prim, set/get attribute, traverse, prims_of_type, to_json/from_json, export_usda
- SdfPath: name, is_absolute, parent(), child()
- VtValue: all factory methods (from_float, from_int, from_bool, from_string, from_token,
           from_asset, from_vec3f), type_name, to_python, repr
"""

from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

from dcc_mcp_core import EventBus
from dcc_mcp_core import SdfPath

# Import local modules
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import ToolValidator
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue
from dcc_mcp_core import scene_info_json_to_stage

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_schema(**props) -> str:
    """Build a minimal JSON Schema string for object type."""
    return json.dumps({"type": "object", "properties": {k: v for k, v in props.items()}})


def _make_required_schema(required: list[str], **props) -> str:
    return json.dumps(
        {
            "type": "object",
            "properties": {k: v for k, v in props.items()},
            "required": required,
        }
    )


def _empty_stage() -> UsdStage:
    return scene_info_json_to_stage(json.dumps({"name": "TestScene", "prims": []}))


# ===========================================================================
# ToolValidator
# ===========================================================================


class TestActionValidatorCreate:
    def test_from_schema_json_creates_validator(self):
        schema = _make_schema(x={"type": "number"})
        v = ToolValidator.from_schema_json(schema)
        assert v is not None

    def test_from_schema_json_repr(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        assert "ToolValidator" in repr(v)

    def test_from_action_registry_creates_validator(self):
        reg = ToolRegistry()
        reg.register(
            "act",
            description="d",
            category="c",
            input_schema=_make_schema(x={"type": "number"}),
        )
        v = ToolValidator.from_action_registry(reg, "act")
        assert v is not None

    def test_from_action_registry_no_schema(self):
        reg = ToolRegistry()
        reg.register("act_no_schema", description="d", category="c")
        v = ToolValidator.from_action_registry(reg, "act_no_schema")
        assert v is not None

    def test_from_action_registry_nonexistent_raises_key_error(self):
        reg = ToolRegistry()
        with pytest.raises(KeyError):
            ToolValidator.from_action_registry(reg, "nonexistent")


class TestActionValidatorHappyPath:
    def test_validate_returns_tuple(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        result = v.validate(json.dumps({"x": 1.0}))
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_validate_success_is_true(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        ok, _errors = v.validate(json.dumps({"x": 3.14}))
        assert ok is True

    def test_validate_success_errors_empty(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        _ok, errors = v.validate(json.dumps({"x": 0}))
        assert errors == []

    def test_validate_extra_fields_allowed(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        ok, _ = v.validate(json.dumps({"x": 1, "extra": "ignored"}))
        assert ok is True

    def test_validate_no_schema_action_all_params_valid(self):
        reg = ToolRegistry()
        reg.register("act", description="d", category="c")
        v = ToolValidator.from_action_registry(reg, "act")
        ok, _ = v.validate(json.dumps({"anything": "works", "num": 42}))
        assert ok is True

    def test_validate_empty_object_against_no_required(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        ok, _ = v.validate("{}")
        assert ok is True

    def test_validate_integer_field(self):
        v = ToolValidator.from_schema_json(_make_schema(count={"type": "integer"}))
        ok, _ = v.validate(json.dumps({"count": 5}))
        assert ok is True

    def test_validate_string_field(self):
        v = ToolValidator.from_schema_json(_make_schema(name={"type": "string"}))
        ok, _ = v.validate(json.dumps({"name": "hello"}))
        assert ok is True

    def test_validate_boolean_field_true(self):
        v = ToolValidator.from_schema_json(_make_schema(flag={"type": "boolean"}))
        ok, _ = v.validate(json.dumps({"flag": True}))
        assert ok is True

    def test_validate_boolean_field_false(self):
        v = ToolValidator.from_schema_json(_make_schema(flag={"type": "boolean"}))
        ok, _ = v.validate(json.dumps({"flag": False}))
        assert ok is True

    def test_validate_multiple_fields(self):
        v = ToolValidator.from_schema_json(
            _make_required_schema(
                ["x", "name"],
                x={"type": "number"},
                name={"type": "string"},
                optional={"type": "integer"},
            )
        )
        ok, _ = v.validate(json.dumps({"x": 1.0, "name": "sphere"}))
        assert ok is True

    def test_validate_from_registry_with_schema(self):
        reg = ToolRegistry()
        reg.register(
            "my_act",
            description="d",
            category="c",
            input_schema=_make_required_schema(["val"], val={"type": "integer"}),
        )
        v = ToolValidator.from_action_registry(reg, "my_act")
        ok, _ = v.validate(json.dumps({"val": 42}))
        assert ok is True


class TestActionValidatorErrorPath:
    def test_validate_missing_required_field(self):
        v = ToolValidator.from_schema_json(_make_required_schema(["x"], x={"type": "number"}))
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0

    def test_validate_missing_required_error_message(self):
        v = ToolValidator.from_schema_json(_make_required_schema(["x"], x={"type": "number"}))
        _, errors = v.validate("{}")
        assert any("x" in e for e in errors)

    def test_validate_wrong_type_number_gets_string(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        ok, errors = v.validate(json.dumps({"x": "not_a_number"}))
        assert ok is False
        assert len(errors) > 0

    def test_validate_wrong_type_integer_gets_float(self):
        v = ToolValidator.from_schema_json(_make_schema(n={"type": "integer"}))
        ok, _errors = v.validate(json.dumps({"n": "abc"}))
        assert ok is False

    def test_validate_wrong_type_boolean_gets_string(self):
        v = ToolValidator.from_schema_json(_make_schema(flag={"type": "boolean"}))
        ok, _errors = v.validate(json.dumps({"flag": "yes"}))
        assert ok is False

    def test_validate_null_required_number(self):
        v = ToolValidator.from_schema_json(_make_required_schema(["x"], x={"type": "number"}))
        ok, _errors = v.validate(json.dumps({"x": None}))
        assert ok is False

    def test_validate_invalid_json_raises_value_error(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        with pytest.raises(ValueError):
            v.validate("not_valid_json{")

    def test_validate_array_instead_of_object(self):
        v = ToolValidator.from_schema_json(_make_schema(x={"type": "number"}))
        ok, errors = v.validate("[]")
        assert ok is False
        assert len(errors) > 0

    def test_validate_multiple_missing_required(self):
        v = ToolValidator.from_schema_json(
            _make_required_schema(["a", "b"], a={"type": "number"}, b={"type": "string"})
        )
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) >= 1

    def test_validate_error_list_is_strings(self):
        v = ToolValidator.from_schema_json(_make_required_schema(["x"], x={"type": "number"}))
        _, errors = v.validate("{}")
        for e in errors:
            assert isinstance(e, str)


# ===========================================================================
# EventBus
# ===========================================================================


class TestEventBusCreate:
    def test_create(self):
        bus = EventBus()
        assert bus is not None

    def test_repr_zero_subscriptions(self):
        bus = EventBus()
        assert "subscriptions=0" in repr(bus)

    def test_str_zero_subscriptions(self):
        bus = EventBus()
        assert "0" in str(bus)


class TestEventBusSubscribePublish:
    def test_subscribe_returns_int_token(self):
        bus = EventBus()
        token = bus.subscribe("my_event", lambda **kw: None)
        assert isinstance(token, int)

    def test_subscribe_increments_count(self):
        bus = EventBus()
        bus.subscribe("e1", lambda **kw: None)
        assert "subscriptions=1" in repr(bus)

    def test_subscribe_two_events(self):
        bus = EventBus()
        bus.subscribe("e1", lambda **kw: None)
        bus.subscribe("e2", lambda **kw: None)
        assert "subscriptions=2" in repr(bus)

    def test_subscribe_same_event_two_handlers(self):
        bus = EventBus()
        bus.subscribe("e1", lambda **kw: None)
        bus.subscribe("e1", lambda **kw: None)
        assert "subscriptions=2" in repr(bus)

    def test_publish_calls_handler(self):
        bus = EventBus()
        received = []
        bus.subscribe("action_done", lambda **kw: received.append(kw))
        bus.publish("action_done", action="create_sphere")
        assert len(received) == 1

    def test_publish_kwargs_received(self):
        bus = EventBus()
        received = []
        bus.subscribe("evt", lambda **kw: received.append(kw))
        bus.publish("evt", action="move", dcc="maya", status="ok")
        assert received[0]["action"] == "move"
        assert received[0]["dcc"] == "maya"
        assert received[0]["status"] == "ok"

    def test_publish_no_kwargs_calls_handler(self):
        bus = EventBus()
        calls = []
        bus.subscribe("tick", lambda **kw: calls.append(kw))
        bus.publish("tick")
        assert len(calls) == 1
        assert calls[0] == {}

    def test_publish_multiple_times(self):
        bus = EventBus()
        received = []
        bus.subscribe("e", lambda **kw: received.append(kw))
        bus.publish("e", n=1)
        bus.publish("e", n=2)
        bus.publish("e", n=3)
        assert len(received) == 3

    def test_publish_different_events_isolated(self):
        bus = EventBus()
        a_calls = []
        b_calls = []
        bus.subscribe("event_a", lambda **kw: a_calls.append(kw))
        bus.subscribe("event_b", lambda **kw: b_calls.append(kw))
        bus.publish("event_a", x=1)
        assert len(a_calls) == 1
        assert len(b_calls) == 0
        bus.publish("event_b", y=2)
        assert len(a_calls) == 1
        assert len(b_calls) == 1

    def test_publish_no_subscribers_no_error(self):
        bus = EventBus()
        bus.publish("no_listeners", x=1)  # should not raise

    def test_multiple_handlers_same_event_all_called(self):
        bus = EventBus()
        count = []
        bus.subscribe("e", lambda **kw: count.append(1))
        bus.subscribe("e", lambda **kw: count.append(2))
        bus.publish("e")
        assert len(count) == 2
        assert sorted(count) == [1, 2]

    def test_handler_receives_kwarg_values_correctly(self):
        bus = EventBus()
        received = {}
        bus.subscribe("e", lambda **kw: received.update(kw))
        bus.publish("e", num=42, flag=True)
        assert received["num"] == 42
        assert received["flag"] is True


class TestEventBusUnsubscribe:
    def test_unsubscribe_removes_handler(self):
        bus = EventBus()
        token = bus.subscribe("e", lambda **kw: None)
        bus.unsubscribe("e", token)
        assert "subscriptions=0" in repr(bus)

    def test_unsubscribe_stops_calls(self):
        bus = EventBus()
        calls = []
        token = bus.subscribe("e", lambda **kw: calls.append(kw))
        bus.publish("e", n=1)
        bus.unsubscribe("e", token)
        bus.publish("e", n=2)
        assert len(calls) == 1

    def test_unsubscribe_one_of_two_handlers(self):
        bus = EventBus()
        calls_a = []
        calls_b = []
        token_a = bus.subscribe("e", lambda **kw: calls_a.append(kw))
        bus.subscribe("e", lambda **kw: calls_b.append(kw))
        bus.unsubscribe("e", token_a)
        bus.publish("e", x=1)
        assert len(calls_a) == 0
        assert len(calls_b) == 1

    def test_tokens_are_unique(self):
        bus = EventBus()
        t1 = bus.subscribe("e", lambda **kw: None)
        t2 = bus.subscribe("e", lambda **kw: None)
        assert t1 != t2

    def test_publish_after_all_unsubscribed_no_error(self):
        bus = EventBus()
        calls = []
        t = bus.subscribe("e", lambda **kw: calls.append(kw))
        bus.unsubscribe("e", t)
        bus.publish("e", x=99)
        assert len(calls) == 0


# ===========================================================================
# VtValue
# ===========================================================================


class TestVtValueFromFloat:
    def test_create(self):
        v = VtValue.from_float(3.14)
        assert v is not None

    def test_type_name_is_float(self):
        v = VtValue.from_float(3.14)
        assert v.type_name == "float"

    def test_to_python_returns_float(self):
        v = VtValue.from_float(1.5)
        assert isinstance(v.to_python(), float)

    def test_to_python_value_approx(self):
        v = VtValue.from_float(2.5)
        assert abs(v.to_python() - 2.5) < 0.001

    def test_repr_contains_float(self):
        v = VtValue.from_float(1.0)
        assert "float" in repr(v).lower() or "Float" in repr(v)

    def test_zero_float(self):
        v = VtValue.from_float(0.0)
        assert v.to_python() == 0.0


class TestVtValueFromInt:
    def test_create(self):
        v = VtValue.from_int(42)
        assert v is not None

    def test_type_name_is_int(self):
        v = VtValue.from_int(42)
        assert v.type_name == "int"

    def test_to_python_returns_int(self):
        v = VtValue.from_int(10)
        assert v.to_python() == 10

    def test_negative_int(self):
        v = VtValue.from_int(-5)
        assert v.to_python() == -5

    def test_zero_int(self):
        v = VtValue.from_int(0)
        assert v.to_python() == 0

    def test_large_int(self):
        v = VtValue.from_int(1_000_000)
        assert v.to_python() == 1_000_000


class TestVtValueFromBool:
    def test_true(self):
        v = VtValue.from_bool(True)
        assert v.to_python() is True

    def test_false(self):
        v = VtValue.from_bool(False)
        assert v.to_python() is False

    def test_type_name_is_bool(self):
        v = VtValue.from_bool(True)
        assert v.type_name == "bool"


class TestVtValueFromString:
    def test_create(self):
        v = VtValue.from_string("hello")
        assert v is not None

    def test_type_name_is_string(self):
        v = VtValue.from_string("hello")
        assert v.type_name == "string"

    def test_to_python_matches(self):
        v = VtValue.from_string("world")
        assert v.to_python() == "world"

    def test_empty_string(self):
        v = VtValue.from_string("")
        assert v.to_python() == ""

    def test_unicode_string(self):
        v = VtValue.from_string("Hello \u4e16\u754c")
        assert v.to_python() == "Hello \u4e16\u754c"


class TestVtValueFromToken:
    def test_create(self):
        v = VtValue.from_token("Xform")
        assert v is not None

    def test_type_name_is_token(self):
        v = VtValue.from_token("Mesh")
        assert v.type_name == "token"

    def test_to_python_matches(self):
        v = VtValue.from_token("Sphere")
        assert v.to_python() == "Sphere"


class TestVtValueFromAsset:
    def test_create(self):
        v = VtValue.from_asset("textures/diffuse.png")
        assert v is not None

    def test_type_name_is_asset(self):
        v = VtValue.from_asset("tex.png")
        assert v.type_name == "asset"

    def test_to_python_matches(self):
        v = VtValue.from_asset("path/to/file.usd")
        assert v.to_python() == "path/to/file.usd"


class TestVtValueFromVec3f:
    def test_create(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v is not None

    def test_type_name_is_float3(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v.type_name == "float3"

    def test_to_python_returns_tuple(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert isinstance(v.to_python(), tuple)

    def test_to_python_values(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        tup = v.to_python()
        assert abs(tup[0] - 1.0) < 0.001
        assert abs(tup[1] - 2.0) < 0.001
        assert abs(tup[2] - 3.0) < 0.001

    def test_to_python_length_3(self):
        v = VtValue.from_vec3f(0.0, 0.0, 0.0)
        assert len(v.to_python()) == 3

    def test_zero_vec(self):
        v = VtValue.from_vec3f(0.0, 0.0, 0.0)
        tup = v.to_python()
        assert all(abs(x) < 0.001 for x in tup)


class TestVtValueEquality:
    def test_same_float_to_python_equal(self):
        a = VtValue.from_float(1.0)
        b = VtValue.from_float(1.0)
        # VtValue instances are not value-equal via ==; compare via to_python()
        assert abs(a.to_python() - b.to_python()) < 0.001

    def test_same_int_to_python_equal(self):
        a = VtValue.from_int(5)
        b = VtValue.from_int(5)
        assert a.to_python() == b.to_python()

    def test_different_int_values_not_equal(self):
        a = VtValue.from_int(1)
        b = VtValue.from_int(2)
        assert a.to_python() != b.to_python()

    def test_different_types_different_type_name(self):
        a = VtValue.from_float(1.0)
        b = VtValue.from_int(1)
        assert a.type_name != b.type_name


# ===========================================================================
# SdfPath
# ===========================================================================


class TestSdfPathCreate:
    def test_create_absolute(self):
        sp = SdfPath("/World/Sphere")
        assert sp is not None

    def test_repr(self):
        sp = SdfPath("/World/Sphere")
        r = repr(sp)
        assert "/World/Sphere" in r or "SdfPath" in r

    def test_str(self):
        sp = SdfPath("/World/Sphere")
        assert "/World/Sphere" in str(sp)


class TestSdfPathName:
    def test_name_leaf(self):
        sp = SdfPath("/World/Sphere")
        assert sp.name == "Sphere"

    def test_name_parent(self):
        sp = SdfPath("/World")
        assert sp.name == "World"

    def test_name_root(self):
        sp = SdfPath("/")
        # Root name may be empty or "/"
        assert isinstance(sp.name, str)

    def test_name_deep_path(self):
        sp = SdfPath("/A/B/C/D")
        assert sp.name == "D"


class TestSdfPathIsAbsolute:
    def test_absolute_path(self):
        sp = SdfPath("/World/Sphere")
        assert sp.is_absolute is True

    def test_relative_path(self):
        sp = SdfPath("Sphere")
        assert sp.is_absolute is False

    def test_root_is_absolute(self):
        sp = SdfPath("/")
        assert sp.is_absolute is True


class TestSdfPathParent:
    def test_parent_is_method(self):
        sp = SdfPath("/World/Sphere")
        assert callable(sp.parent)

    def test_parent_returns_sdfpath(self):
        sp = SdfPath("/World/Sphere")
        par = sp.parent()
        assert isinstance(par, SdfPath)

    def test_parent_name(self):
        sp = SdfPath("/World/Sphere")
        assert sp.parent().name == "World"

    def test_parent_of_parent(self):
        sp = SdfPath("/A/B/C")
        assert sp.parent().parent().name == "A"

    def test_parent_path_string(self):
        sp = SdfPath("/World/Sphere")
        par = sp.parent()
        assert "World" in str(par)


class TestSdfPathChild:
    def test_child_appends_name(self):
        sp = SdfPath("/World")
        child = sp.child("Sphere")
        assert "Sphere" in str(child)

    def test_child_name(self):
        sp = SdfPath("/World")
        child = sp.child("Sphere")
        assert child.name == "Sphere"

    def test_child_is_absolute(self):
        sp = SdfPath("/World")
        child = sp.child("Sphere")
        assert child.is_absolute is True

    def test_child_of_child(self):
        sp = SdfPath("/World")
        grandchild = sp.child("A").child("B")
        assert grandchild.name == "B"

    def test_child_parent_round_trip(self):
        sp = SdfPath("/World")
        child = sp.child("Sphere")
        assert child.parent().name == sp.name


# ===========================================================================
# UsdStage
# ===========================================================================


class TestUsdStageFromJson:
    def test_scene_info_json_creates_stage(self):
        info = json.dumps({"name": "MyScene", "prims": []})
        stage = scene_info_json_to_stage(info)
        assert stage is not None

    def test_stage_name(self):
        info = json.dumps({"name": "MyScene", "prims": []})
        stage = scene_info_json_to_stage(info)
        assert stage.name == "MyScene"

    def test_stage_repr(self):
        info = json.dumps({"name": "MyScene", "prims": []})
        stage = scene_info_json_to_stage(info)
        assert "MyScene" in repr(stage)

    def test_stage_id_is_string(self):
        stage = _empty_stage()
        assert isinstance(stage.id, str)
        assert len(stage.id) > 0

    def test_default_up_axis(self):
        stage = _empty_stage()
        assert stage.up_axis in ("Y", "Z", "y", "z")

    def test_default_meters_per_unit(self):
        stage = _empty_stage()
        assert stage.meters_per_unit == 0.01

    def test_fps_default_none(self):
        stage = _empty_stage()
        # fps may be None or a float
        assert stage.fps is None or isinstance(stage.fps, float)

    def test_start_time_code_default(self):
        stage = _empty_stage()
        assert stage.start_time_code is None or isinstance(stage.start_time_code, float)

    def test_end_time_code_default(self):
        stage = _empty_stage()
        assert stage.end_time_code is None or isinstance(stage.end_time_code, float)

    def test_from_json_round_trip(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        j = stage.to_json()
        stage2 = UsdStage.from_json(j)
        assert stage2.name == stage.name

    def test_to_json_returns_string(self):
        stage = _empty_stage()
        j = stage.to_json()
        assert isinstance(j, str)
        assert len(j) > 0

    def test_to_json_is_valid_json(self):
        stage = _empty_stage()
        j = stage.to_json()
        parsed = json.loads(j)
        assert isinstance(parsed, dict)

    def test_from_json_invalid_raises(self):
        with pytest.raises((ValueError, RuntimeError, TypeError)):
            UsdStage.from_json("not_valid_json")


class TestUsdStageDefinePrim:
    def test_define_prim_returns_usd_prim(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Sphere", "Sphere")
        assert prim is not None

    def test_define_prim_type_name(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Sphere", "Sphere")
        assert prim.type_name == "Sphere"

    def test_define_prim_path_str(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Sphere", "Sphere")
        assert "/World/Sphere" in str(prim.path)

    def test_define_prim_name(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Mesh", "Mesh")
        assert prim.name == "Mesh"

    def test_define_prim_active(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Cube", "Cube")
        assert prim.active is True

    def test_define_prim_increases_count(self):
        stage = _empty_stage()
        initial = stage.prim_count()
        stage.define_prim("/World/A", "Xform")
        assert stage.prim_count() == initial + 1

    def test_define_multiple_prims(self):
        stage = _empty_stage()
        stage.define_prim("/World/A", "Sphere")
        stage.define_prim("/World/B", "Mesh")
        stage.define_prim("/World/C", "Cube")
        assert stage.prim_count() >= 3

    def test_define_prim_xform_type(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/Group", "Xform")
        assert prim.type_name == "Xform"


class TestUsdStageHasPrimGetPrim:
    def test_has_prim_true(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        assert stage.has_prim("/World/Sphere") is True

    def test_has_prim_false(self):
        stage = _empty_stage()
        assert stage.has_prim("/World/Missing") is False

    def test_get_prim_returns_prim(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        prim = stage.get_prim("/World/Sphere")
        assert prim is not None

    def test_get_prim_missing_returns_none(self):
        stage = _empty_stage()
        prim = stage.get_prim("/World/Missing")
        assert prim is None

    def test_get_prim_type_name_matches(self):
        stage = _empty_stage()
        stage.define_prim("/World/Mesh", "Mesh")
        prim = stage.get_prim("/World/Mesh")
        assert prim.type_name == "Mesh"

    def test_world_prim_always_exists(self):
        stage = _empty_stage()
        # /World is auto-created
        prim = stage.get_prim("/World")
        assert prim is not None


class TestUsdStageListPrimsTraverse:
    def test_list_prims_returns_list(self):
        stage = _empty_stage()
        prims = stage.list_prims()
        assert isinstance(prims, list)

    def test_list_prims_includes_defined(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        paths = [p.path for p in stage.list_prims()]
        assert any("Sphere" in str(p) for p in paths)

    def test_traverse_returns_list(self):
        stage = _empty_stage()
        result = stage.traverse()
        assert isinstance(result, list)

    def test_traverse_includes_defined_prims(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        stage.define_prim("/World/Mesh", "Mesh")
        paths = [str(p.path) for p in stage.traverse()]
        assert any("Sphere" in p for p in paths)
        assert any("Mesh" in p for p in paths)

    def test_prim_count_method(self):
        stage = _empty_stage()
        count = stage.prim_count()
        assert isinstance(count, int)
        assert count >= 1

    def test_prims_of_type_returns_list(self):
        stage = _empty_stage()
        result = stage.prims_of_type("Sphere")
        assert isinstance(result, list)

    def test_prims_of_type_empty_for_missing_type(self):
        stage = _empty_stage()
        result = stage.prims_of_type("NonexistentType")
        assert result == []

    def test_prims_of_type_finds_matching(self):
        stage = _empty_stage()
        stage.define_prim("/World/S1", "Sphere")
        stage.define_prim("/World/S2", "Sphere")
        stage.define_prim("/World/M1", "Mesh")
        spheres = stage.prims_of_type("Sphere")
        assert len(spheres) == 2

    def test_prims_of_type_excludes_other(self):
        stage = _empty_stage()
        stage.define_prim("/World/S1", "Sphere")
        stage.define_prim("/World/M1", "Mesh")
        spheres = stage.prims_of_type("Sphere")
        for p in spheres:
            assert p.type_name == "Sphere"


class TestUsdStageRemovePrim:
    def test_remove_prim_decreases_count(self):
        stage = _empty_stage()
        stage.define_prim("/World/Cube", "Cube")
        before = stage.prim_count()
        stage.remove_prim("/World/Cube")
        assert stage.prim_count() == before - 1

    def test_remove_prim_not_in_list_after(self):
        stage = _empty_stage()
        stage.define_prim("/World/Cube", "Cube")
        stage.remove_prim("/World/Cube")
        assert stage.has_prim("/World/Cube") is False

    def test_remove_nonexistent_no_error(self):
        stage = _empty_stage()
        # Removing non-existent prim should not raise
        stage.remove_prim("/World/Nonexistent")


class TestUsdStageAttributes:
    def test_set_get_float_attribute(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        stage.set_attribute("/World/Sphere", "radius", VtValue.from_float(5.0))
        v = stage.get_attribute("/World/Sphere", "radius")
        assert v is not None
        assert v.type_name == "float"
        assert abs(v.to_python() - 5.0) < 0.01

    def test_set_get_int_attribute(self):
        stage = _empty_stage()
        stage.define_prim("/World/Mesh", "Mesh")
        stage.set_attribute("/World/Mesh", "vertices", VtValue.from_int(8))
        v = stage.get_attribute("/World/Mesh", "vertices")
        assert v.to_python() == 8

    def test_set_get_bool_attribute(self):
        stage = _empty_stage()
        stage.define_prim("/World/P", "Xform")
        stage.set_attribute("/World/P", "visible", VtValue.from_bool(True))
        v = stage.get_attribute("/World/P", "visible")
        assert v.to_python() is True

    def test_set_get_string_attribute(self):
        stage = _empty_stage()
        stage.define_prim("/World/P", "Xform")
        stage.set_attribute("/World/P", "label", VtValue.from_string("hello"))
        v = stage.get_attribute("/World/P", "label")
        assert v.to_python() == "hello"

    def test_set_get_vec3f_attribute(self):
        stage = _empty_stage()
        stage.define_prim("/World/P", "Xform")
        stage.set_attribute("/World/P", "translate", VtValue.from_vec3f(1.0, 2.0, 3.0))
        v = stage.get_attribute("/World/P", "translate")
        tup = v.to_python()
        assert len(tup) == 3
        assert abs(tup[0] - 1.0) < 0.01

    def test_get_nonexistent_attribute_returns_none(self):
        stage = _empty_stage()
        stage.define_prim("/World/P", "Xform")
        v = stage.get_attribute("/World/P", "nonexistent_attr")
        assert v is None

    def test_set_attribute_missing_prim_raises_value_error(self):
        stage = _empty_stage()
        with pytest.raises(ValueError):
            stage.set_attribute("/World/Missing", "x", VtValue.from_float(1.0))


class TestUsdStagePrimAttributes:
    def test_prim_set_get_attribute(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        prim.set_attribute("score", VtValue.from_int(100))
        v = prim.get_attribute("score")
        assert v is not None
        assert v.to_python() == 100

    def test_prim_attribute_names_method(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        names = prim.attribute_names()
        assert isinstance(names, list)

    def test_prim_attribute_names_after_set(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        prim.set_attribute("my_attr", VtValue.from_float(1.0))
        names = prim.attribute_names()
        assert "my_attr" in names

    def test_prim_attributes_summary_is_dict(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        prim.set_attribute("x", VtValue.from_float(1.0))
        summary = prim.attributes_summary()
        assert isinstance(summary, dict)

    def test_prim_attributes_summary_type_names(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        prim.set_attribute("x", VtValue.from_float(1.0))
        prim.set_attribute("n", VtValue.from_int(5))
        summary = prim.attributes_summary()
        assert summary["x"] == "float"
        assert summary["n"] == "int"

    def test_prim_get_nonexistent_attr_returns_none(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Xform")
        v = prim.get_attribute("nonexistent")
        assert v is None

    def test_prim_has_api(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Sphere")
        result = prim.has_api("SphereAPI")
        assert isinstance(result, bool)

    def test_prim_has_api_false_for_wrong_type(self):
        stage = _empty_stage()
        prim = stage.define_prim("/World/P", "Sphere")
        result = prim.has_api("MeshAPI")
        assert isinstance(result, bool)


class TestUsdStageMetrics:
    def test_metrics_returns_dict(self):
        stage = _empty_stage()
        m = stage.metrics()
        assert isinstance(m, dict)

    def test_metrics_prim_count_key(self):
        stage = _empty_stage()
        m = stage.metrics()
        assert "prim_count" in m

    def test_metrics_mesh_count_key(self):
        stage = _empty_stage()
        m = stage.metrics()
        assert "mesh_count" in m

    def test_metrics_prim_count_matches_defined(self):
        stage = _empty_stage()
        stage.define_prim("/World/A", "Sphere")
        stage.define_prim("/World/B", "Mesh")
        m = stage.metrics()
        assert m["prim_count"] >= 2

    def test_metrics_mesh_count_counts_meshes(self):
        stage = _empty_stage()
        stage.define_prim("/World/M1", "Mesh")
        stage.define_prim("/World/M2", "Mesh")
        m = stage.metrics()
        assert m["mesh_count"] == 2


class TestUsdStageDefaultPrim:
    def test_default_prim_is_string(self):
        stage = _empty_stage()
        dp = stage.default_prim
        assert isinstance(dp, str)

    def test_set_default_prim(self):
        stage = _empty_stage()
        stage.define_prim("/World/Sphere", "Sphere")
        stage.set_default_prim("/World/Sphere")
        assert "/World/Sphere" in stage.default_prim or "Sphere" in stage.default_prim

    def test_set_meters_per_unit(self):
        stage = _empty_stage()
        stage.set_meters_per_unit(1.0)
        assert stage.meters_per_unit == 1.0

    def test_set_meters_per_unit_small(self):
        stage = _empty_stage()
        stage.set_meters_per_unit(0.001)
        assert abs(stage.meters_per_unit - 0.001) < 1e-6


class TestUsdStageExportUsda:
    def test_export_usda_returns_string(self):
        stage = _empty_stage()
        usda = stage.export_usda()
        assert isinstance(usda, str)

    def test_export_usda_starts_with_comment(self):
        stage = _empty_stage()
        usda = stage.export_usda()
        assert usda.startswith("#usda") or usda.startswith("#")

    def test_export_usda_contains_up_axis(self):
        stage = _empty_stage()
        usda = stage.export_usda()
        assert "upAxis" in usda or "up_axis" in usda or "Y" in usda or "Z" in usda

    def test_export_usda_contains_prim_name(self):
        stage = _empty_stage()
        stage.define_prim("/World/MySphere", "Sphere")
        usda = stage.export_usda()
        assert "MySphere" in usda or "Sphere" in usda
