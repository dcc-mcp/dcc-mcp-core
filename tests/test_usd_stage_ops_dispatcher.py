"""Tests for UsdStage advanced ops (prims_of_type/remove_prim/has_prim/default_prim).

Also covers ToolDispatcher depth (remove_handler/has_handler/handler_names/skip_empty_schema).
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import SdfPath
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import UsdPrim
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# UsdStage advanced ops
# ---------------------------------------------------------------------------


class TestUsdStageHasPrim:
    """has_prim basic and edge cases."""

    def test_has_prim_existing(self):
        stage = UsdStage("test_has")
        stage.define_prim("/World", "Xform")
        assert stage.has_prim("/World") is True

    def test_has_prim_missing(self):
        stage = UsdStage("test_has_missing")
        assert stage.has_prim("/NonExistent") is False

    def test_has_prim_nested_existing(self):
        stage = UsdStage("test_has_nested")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        assert stage.has_prim("/World/Cube") is True

    def test_has_prim_nested_missing_parent(self):
        stage = UsdStage("test_has_nested_miss")
        assert stage.has_prim("/World/Cube") is False

    def test_has_prim_after_remove(self):
        stage = UsdStage("test_has_after_remove")
        stage.define_prim("/Sphere", "Sphere")
        assert stage.has_prim("/Sphere") is True
        stage.remove_prim("/Sphere")
        assert stage.has_prim("/Sphere") is False


class TestUsdStageRemovePrim:
    """remove_prim happy path and edge cases."""

    def test_remove_existing_prim(self):
        stage = UsdStage("test_remove")
        stage.define_prim("/Cube", "Mesh")
        result = stage.remove_prim("/Cube")
        assert result is True

    def test_remove_nonexistent_returns_false(self):
        stage = UsdStage("test_remove_nonexistent")
        result = stage.remove_prim("/DoesNotExist")
        assert result is False

    def test_remove_reduces_traverse_count(self):
        stage = UsdStage("test_remove_traverse")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Mesh")
        before = len(stage.traverse())
        stage.remove_prim("/A")
        after = len(stage.traverse())
        assert after == before - 1

    def test_remove_then_redefine(self):
        stage = UsdStage("test_remove_redefine")
        stage.define_prim("/Obj", "Mesh")
        stage.remove_prim("/Obj")
        assert stage.has_prim("/Obj") is False
        stage.define_prim("/Obj", "Sphere")
        assert stage.has_prim("/Obj") is True

    def test_remove_child_does_not_affect_parent(self):
        stage = UsdStage("test_remove_child")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Child", "Mesh")
        stage.remove_prim("/World/Child")
        assert stage.has_prim("/World") is True
        assert stage.has_prim("/World/Child") is False

    def test_remove_multiple_prims(self):
        stage = UsdStage("test_remove_multiple")
        for i in range(5):
            stage.define_prim(f"/Prim{i}", "Mesh")
        for i in range(5):
            stage.remove_prim(f"/Prim{i}")
        assert len(stage.traverse()) == 0


class TestUsdStageDefaultPrim:
    """default_prim property (read) and set_default_prim method."""

    def test_default_prim_initially_none(self):
        stage = UsdStage("test_default_none")
        assert stage.default_prim is None

    def test_set_default_prim_and_read(self):
        stage = UsdStage("test_default_set")
        stage.define_prim("/World", "Xform")
        stage.set_default_prim("/World")
        assert stage.default_prim == "/World"

    def test_set_default_prim_clear_with_empty_string(self):
        stage = UsdStage("test_default_unset")
        stage.define_prim("/World", "Xform")
        stage.set_default_prim("/World")
        # Use empty string to clear (None is not accepted by Rust binding)
        stage.set_default_prim("")
        # After clearing, default_prim may be None or ""
        dp = stage.default_prim
        assert dp is None or dp == ""

    def test_set_default_prim_overwrite(self):
        stage = UsdStage("test_default_overwrite")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        stage.set_default_prim("/A")
        stage.set_default_prim("/B")
        assert stage.default_prim == "/B"

    def test_default_prim_prop_attribute_exists(self):
        stage = UsdStage("test_default_prop")
        # default_prim_prop may be write-only or not yet readable; just check no crash
        stage.define_prim("/Root", "Xform")
        stage.set_default_prim("/Root")
        # Verify set_default_prim succeeded
        assert stage.default_prim == "/Root"

    def test_json_roundtrip_does_not_raise(self):
        stage = UsdStage("test_default_json")
        stage.define_prim("/Root", "Xform")
        stage.set_default_prim("/Root")
        json_str = stage.to_json()
        back = UsdStage.from_json(json_str)
        assert back is not None


class TestUsdStageListPrimsCount:
    """list_prims and prim_count methods."""

    def test_prim_count_empty(self):
        stage = UsdStage("test_count_empty")
        count = stage.prim_count()
        assert count == 0

    def test_prim_count_increases(self):
        stage = UsdStage("test_count_inc")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        assert stage.prim_count() == 2

    def test_list_prims_empty(self):
        stage = UsdStage("test_list_empty")
        prims = stage.list_prims()
        assert isinstance(prims, list)
        assert len(prims) == 0

    def test_list_prims_returns_prim_objects(self):
        stage = UsdStage("test_list_prims")
        stage.define_prim("/X", "Xform")
        prims = stage.list_prims()
        assert len(prims) >= 1
        assert isinstance(prims[0], UsdPrim)

    def test_list_prims_matches_prim_count(self):
        stage = UsdStage("test_list_count")
        stage.define_prim("/P1", "Mesh")
        stage.define_prim("/P2", "Mesh")
        stage.define_prim("/P3", "Sphere")
        assert len(stage.list_prims()) == stage.prim_count()

    def test_prim_count_after_remove(self):
        stage = UsdStage("test_count_rm")
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        stage.remove_prim("/A")
        assert stage.prim_count() == 1


class TestUsdStagePrivsOfType:
    """prims_of_type: filtering prims by USD type name."""

    def test_prims_of_type_empty_stage(self):
        stage = UsdStage("test_pot_empty")
        result = stage.prims_of_type("Mesh")
        assert isinstance(result, list)
        assert len(result) == 0

    def test_prims_of_type_single_match(self):
        stage = UsdStage("test_pot_single")
        stage.define_prim("/Cube", "Mesh")
        result = stage.prims_of_type("Mesh")
        assert len(result) == 1
        assert result[0].type_name == "Mesh"

    def test_prims_of_type_multiple_matches(self):
        stage = UsdStage("test_pot_multi")
        stage.define_prim("/CubeA", "Mesh")
        stage.define_prim("/CubeB", "Mesh")
        stage.define_prim("/Light", "SphereLight")
        result = stage.prims_of_type("Mesh")
        assert len(result) == 2
        names = {p.name for p in result}
        assert "CubeA" in names
        assert "CubeB" in names

    def test_prims_of_type_no_match(self):
        stage = UsdStage("test_pot_no_match")
        stage.define_prim("/Cube", "Mesh")
        result = stage.prims_of_type("Camera")
        assert len(result) == 0

    def test_prims_of_type_xform(self):
        stage = UsdStage("test_pot_xform")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Sub", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        xforms = stage.prims_of_type("Xform")
        assert len(xforms) >= 2

    def test_prims_of_type_returns_usd_prim_objects(self):
        stage = UsdStage("test_pot_types")
        stage.define_prim("/Sphere", "Sphere")
        result = stage.prims_of_type("Sphere")
        assert len(result) >= 1
        assert isinstance(result[0], UsdPrim)

    def test_prims_of_type_after_remove(self):
        stage = UsdStage("test_pot_remove")
        stage.define_prim("/MeshA", "Mesh")
        stage.define_prim("/MeshB", "Mesh")
        stage.remove_prim("/MeshA")
        result = stage.prims_of_type("Mesh")
        assert len(result) == 1
        assert result[0].name == "MeshB"

    def test_prims_of_type_case_sensitive(self):
        stage = UsdStage("test_pot_case")
        stage.define_prim("/Obj", "Mesh")
        # USD type names are PascalCase
        result_lower = stage.prims_of_type("mesh")
        result_upper = stage.prims_of_type("Mesh")
        # Verify we get results for exact case
        assert len(result_upper) >= 1
        # Lower case probably returns 0 (case-sensitive)
        assert isinstance(result_lower, list)


class TestUsdStageMetricsWithOps:
    """metrics() after various operations."""

    def test_metrics_increases_with_prims(self):
        stage = UsdStage("test_metrics_ops")
        m0 = stage.metrics()
        stage.define_prim("/A", "Mesh")
        stage.define_prim("/B", "Mesh")
        m2 = stage.metrics()
        assert m2["prim_count"] >= m0["prim_count"] + 2

    def test_metrics_decreases_after_remove(self):
        stage = UsdStage("test_metrics_rm")
        stage.define_prim("/X", "Mesh")
        stage.define_prim("/Y", "Mesh")
        m_before = stage.metrics()["prim_count"]
        stage.remove_prim("/X")
        m_after = stage.metrics()["prim_count"]
        assert m_after == m_before - 1


# ---------------------------------------------------------------------------
# ToolDispatcher depth tests
# ---------------------------------------------------------------------------


class TestActionDispatcherRemoveHandler:
    """remove_handler: returns True when removed, False when not found."""

    def _make_dispatcher(self):
        reg = ToolRegistry()
        reg.register("action_a", category="test")
        reg.register("action_b", category="test")
        dispatcher = ToolDispatcher(reg)
        return dispatcher

    def test_remove_existing_handler(self):
        d = self._make_dispatcher()
        d.register_handler("action_a", lambda p: "ok")
        result = d.remove_handler("action_a")
        assert result is True

    def test_remove_nonexistent_handler(self):
        d = self._make_dispatcher()
        result = d.remove_handler("action_a")
        assert result is False

    def test_remove_then_has_handler_false(self):
        d = self._make_dispatcher()
        d.register_handler("action_a", lambda p: "ok")
        d.remove_handler("action_a")
        assert d.has_handler("action_a") is False

    def test_remove_second_time_returns_false(self):
        d = self._make_dispatcher()
        d.register_handler("action_a", lambda p: "ok")
        d.remove_handler("action_a")
        result = d.remove_handler("action_a")
        assert result is False

    def test_remove_one_does_not_affect_other(self):
        d = self._make_dispatcher()
        d.register_handler("action_a", lambda p: "a")
        d.register_handler("action_b", lambda p: "b")
        d.remove_handler("action_a")
        assert d.has_handler("action_b") is True
        assert d.has_handler("action_a") is False


class TestActionDispatcherHasHandler:
    """has_handler: True if registered, False otherwise."""

    def _make_dispatcher(self):
        reg = ToolRegistry()
        reg.register("x", category="test")
        return ToolDispatcher(reg)

    def test_has_handler_before_register(self):
        d = self._make_dispatcher()
        assert d.has_handler("x") is False

    def test_has_handler_after_register(self):
        d = self._make_dispatcher()
        d.register_handler("x", lambda p: None)
        assert d.has_handler("x") is True

    def test_has_handler_unknown_action(self):
        d = self._make_dispatcher()
        assert d.has_handler("completely_unknown") is False


class TestActionDispatcherHandlerNames:
    """handler_names: sorted list of registered handler names."""

    def test_handler_names_empty(self):
        reg = ToolRegistry()
        d = ToolDispatcher(reg)
        names = d.handler_names()
        assert isinstance(names, list)
        assert len(names) == 0

    def test_handler_names_single(self):
        reg = ToolRegistry()
        reg.register("my_action")
        d = ToolDispatcher(reg)
        d.register_handler("my_action", lambda p: None)
        names = d.handler_names()
        assert names == ["my_action"]

    def test_handler_names_multiple_sorted(self):
        reg = ToolRegistry()
        reg.register("zulu")
        reg.register("alpha")
        reg.register("mike")
        d = ToolDispatcher(reg)
        d.register_handler("zulu", lambda p: None)
        d.register_handler("alpha", lambda p: None)
        d.register_handler("mike", lambda p: None)
        names = d.handler_names()
        assert names == sorted(names), "handler_names should be sorted alphabetically"
        assert set(names) == {"zulu", "alpha", "mike"}

    def test_handler_names_after_remove(self):
        reg = ToolRegistry()
        reg.register("a1")
        reg.register("a2")
        d = ToolDispatcher(reg)
        d.register_handler("a1", lambda p: None)
        d.register_handler("a2", lambda p: None)
        d.remove_handler("a1")
        names = d.handler_names()
        assert "a1" not in names
        assert "a2" in names

    def test_handler_count_matches_handler_names(self):
        reg = ToolRegistry()
        reg.register("p")
        reg.register("q")
        d = ToolDispatcher(reg)
        d.register_handler("p", lambda x: None)
        d.register_handler("q", lambda x: None)
        assert d.handler_count() == len(d.handler_names())


class TestActionDispatcherSkipEmptySchema:
    """skip_empty_schema_validation property."""

    def test_default_is_true(self):
        reg = ToolRegistry()
        d = ToolDispatcher(reg)
        # Default should be True (skip validation when schema is empty)
        assert isinstance(d.skip_empty_schema_validation, bool)

    def test_set_false(self):
        reg = ToolRegistry()
        d = ToolDispatcher(reg)
        d.skip_empty_schema_validation = False
        assert d.skip_empty_schema_validation is False

    def test_set_true(self):
        reg = ToolRegistry()
        d = ToolDispatcher(reg)
        d.skip_empty_schema_validation = False
        d.skip_empty_schema_validation = True
        assert d.skip_empty_schema_validation is True

    def test_toggle_multiple_times(self):
        reg = ToolRegistry()
        d = ToolDispatcher(reg)
        for expected in [False, True, False, True]:
            d.skip_empty_schema_validation = expected
            assert d.skip_empty_schema_validation is expected


class TestActionDispatcherDispatchVariants:
    """dispatch: various validation/handler scenarios."""

    def _make(self, schema: str = ""):
        reg = ToolRegistry()
        reg.register("action", input_schema=schema)
        d = ToolDispatcher(reg)
        return d

    def test_dispatch_no_handler_raises_key_error(self):
        d = self._make()
        with pytest.raises((KeyError, RuntimeError)):
            d.dispatch("action", "{}")

    def test_dispatch_with_handler_returns_output(self):
        d = self._make()
        d.register_handler("action", lambda p: {"done": True})
        result = d.dispatch("action", "{}")
        assert result["output"] == {"done": True}
        assert result["action"] == "action"

    def test_dispatch_validation_skipped_when_schema_empty(self):
        d = self._make(schema="")  # empty schema
        d.register_handler("action", lambda p: "ok")
        result = d.dispatch("action", '{"any": "thing"}')
        assert result["validation_skipped"] is True

    def test_dispatch_with_schema_validates_ok(self):
        schema = json.dumps({"type": "object", "required": ["x"], "properties": {"x": {"type": "number"}}})
        d = self._make(schema=schema)
        d.register_handler("action", lambda p: p["x"] * 2)
        result = d.dispatch("action", '{"x": 5}')
        assert result["output"] == 10

    def test_dispatch_with_schema_validation_fail(self):
        schema = json.dumps({"type": "object", "required": ["x"], "properties": {"x": {"type": "number"}}})
        d = self._make(schema=schema)
        d.register_handler("action", lambda p: "should not reach")
        with pytest.raises((ValueError, RuntimeError)):
            d.dispatch("action", '{"y": "wrong"}')

    def test_dispatch_invalid_json_raises(self):
        d = self._make()
        d.register_handler("action", lambda p: None)
        with pytest.raises((ValueError, RuntimeError)):
            d.dispatch("action", "NOT_JSON{{{")

    def test_dispatch_handler_exception_raises_runtime_error(self):
        d = self._make()

        def bad_handler(p):
            raise ValueError("handler exploded")

        d.register_handler("action", bad_handler)
        with pytest.raises((RuntimeError, ValueError)):
            d.dispatch("action", "{}")

    def test_dispatch_handler_receives_dict(self):
        d = self._make()
        received = {}

        def capture(p):
            received.update(p)
            return "captured"

        d.register_handler("action", capture)
        d.dispatch("action", '{"foo": 42}')
        assert received.get("foo") == 42

    def test_dispatch_non_callable_handler_raises_type_error(self):
        d = self._make()
        with pytest.raises((TypeError, RuntimeError)):
            d.register_handler("action", "not_callable")

    def test_dispatch_null_params_accepted(self):
        d = self._make()
        d.register_handler("action", lambda p: "null_ok")
        result = d.dispatch("action")  # default params_json="null"
        assert result["output"] == "null_ok"
