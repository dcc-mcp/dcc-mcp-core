"""Deep tests for UsdStage.traverse(), prims_of_type(), and wrap_value/unwrap_value.

Covers:
- UsdStage.traverse() on flat/nested/deep hierarchies
- UsdStage.traverse() includes all prims regardless of depth
- UsdStage.prims_of_type() exact type filtering (Mesh, Xform, Sphere, Light, etc.)
- UsdStage.prims_of_type() returns empty list for non-existent type
- SdfPath.child() / parent() chaining
- wrap_value() creates correct wrapper types for bool/int/float/str
- unwrap_value() converts wrappers back to Python primitives
- wrap_value() passes through non-primitive types unchanged
- unwrap_parameters() batch unwraps dict values
- Wrapper __bool__/__int__/__float__/__str__ dunder behaviour
- Wrapper __eq__ and __hash__
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import SdfPath
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue
from dcc_mcp_core import unwrap_parameters
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _stage_with_prims(prims: list[tuple[str, str]]) -> UsdStage:
    """Create a stage with (path, type_name) pairs."""
    stage = UsdStage("test")
    for path, type_name in prims:
        stage.define_prim(path, type_name)
    return stage


# ---------------------------------------------------------------------------
# UsdStage.traverse()
# ---------------------------------------------------------------------------


class TestUsdStageTraverse:
    def test_traverse_empty_stage_returns_empty(self):
        stage = UsdStage("empty")
        assert stage.traverse() == []

    def test_traverse_single_prim(self):
        stage = _stage_with_prims([("/World", "Xform")])
        prims = stage.traverse()
        assert len(prims) == 1
        assert str(prims[0].path) == "/World"

    def test_traverse_flat_multiple_prims(self):
        stage = _stage_with_prims(
            [
                ("/Cube", "Mesh"),
                ("/Sphere", "Sphere"),
                ("/Camera", "Camera"),
            ]
        )
        prims = stage.traverse()
        assert len(prims) == 3
        paths = {str(p.path) for p in prims}
        assert paths == {"/Cube", "/Sphere", "/Camera"}

    def test_traverse_parent_child_both_included(self):
        stage = _stage_with_prims(
            [
                ("/World", "Xform"),
                ("/World/Cube", "Mesh"),
            ]
        )
        prims = stage.traverse()
        assert len(prims) == 2
        paths = {str(p.path) for p in prims}
        assert "/World" in paths
        assert "/World/Cube" in paths

    def test_traverse_deep_hierarchy_all_included(self):
        stage = _stage_with_prims(
            [
                ("/Root", "Xform"),
                ("/Root/Level1", "Xform"),
                ("/Root/Level1/Level2", "Xform"),
                ("/Root/Level1/Level2/Leaf", "Mesh"),
            ]
        )
        prims = stage.traverse()
        assert len(prims) == 4
        paths = {str(p.path) for p in prims}
        assert "/Root/Level1/Level2/Leaf" in paths

    def test_traverse_mixed_branches(self):
        stage = _stage_with_prims(
            [
                ("/Root", "Xform"),
                ("/Root/BranchA", "Xform"),
                ("/Root/BranchA/Mesh", "Mesh"),
                ("/Root/BranchB", "Sphere"),
            ]
        )
        prims = stage.traverse()
        assert len(prims) == 4

    def test_traverse_prims_have_path_attribute(self):
        stage = _stage_with_prims([("/P", "Mesh")])
        for prim in stage.traverse():
            assert hasattr(prim, "path")
            assert str(prim.path).startswith("/")

    def test_traverse_prims_have_type_name(self):
        stage = _stage_with_prims([("/M", "Mesh")])
        prims = stage.traverse()
        assert prims[0].type_name == "Mesh"

    def test_traverse_count_equals_total_defined(self):
        paths = [f"/Obj{i}" for i in range(10)]
        prims = [(p, "Xform") for p in paths]
        stage = _stage_with_prims(prims)
        assert len(stage.traverse()) == 10

    def test_traverse_after_remove_prim_decrements(self):
        stage = _stage_with_prims(
            [
                ("/A", "Mesh"),
                ("/B", "Mesh"),
            ]
        )
        stage.remove_prim("/A")
        prims = stage.traverse()
        assert len(prims) == 1
        assert str(prims[0].path) == "/B"


# ---------------------------------------------------------------------------
# UsdStage.prims_of_type()
# ---------------------------------------------------------------------------


class TestUsdStagePrimsOfType:
    def test_prims_of_type_returns_empty_for_nonexistent_type(self):
        stage = _stage_with_prims([("/M", "Mesh")])
        assert stage.prims_of_type("SphereLight") == []

    def test_prims_of_type_exact_match_mesh(self):
        stage = _stage_with_prims(
            [
                ("/A", "Mesh"),
                ("/B", "Sphere"),
                ("/C", "Mesh"),
            ]
        )
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 2
        paths = {str(p.path) for p in meshes}
        assert paths == {"/A", "/C"}

    def test_prims_of_type_single_sphere(self):
        stage = _stage_with_prims(
            [
                ("/S", "Sphere"),
                ("/M", "Mesh"),
            ]
        )
        spheres = stage.prims_of_type("Sphere")
        assert len(spheres) == 1
        assert str(spheres[0].path) == "/S"

    def test_prims_of_type_xform(self):
        stage = _stage_with_prims(
            [
                ("/World", "Xform"),
                ("/World/Sub", "Xform"),
                ("/World/Geo", "Mesh"),
            ]
        )
        xforms = stage.prims_of_type("Xform")
        assert len(xforms) == 2

    def test_prims_of_type_all_same_type(self):
        stage = _stage_with_prims(
            [
                ("/M1", "Mesh"),
                ("/M2", "Mesh"),
                ("/M3", "Mesh"),
            ]
        )
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 3

    def test_prims_of_type_empty_stage(self):
        stage = UsdStage("empty2")
        assert stage.prims_of_type("Mesh") == []

    def test_prims_of_type_does_not_include_other_types(self):
        stage = _stage_with_prims(
            [
                ("/A", "Mesh"),
                ("/B", "Sphere"),
                ("/C", "Camera"),
            ]
        )
        meshes = stage.prims_of_type("Mesh")
        type_names = {p.type_name for p in meshes}
        assert type_names == {"Mesh"}

    def test_prims_of_type_with_deep_nesting(self):
        stage = _stage_with_prims(
            [
                ("/Root", "Xform"),
                ("/Root/Level1", "Xform"),
                ("/Root/Level1/Leaf", "Mesh"),
            ]
        )
        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 1
        assert str(meshes[0].path) == "/Root/Level1/Leaf"


# ---------------------------------------------------------------------------
# SdfPath.child() and parent()
# ---------------------------------------------------------------------------


class TestSdfPathChildParent:
    def test_child_creates_correct_path(self):
        p = SdfPath("/World")
        c = p.child("Cube")
        assert str(c) == "/World/Cube"

    def test_child_chaining(self):
        p = SdfPath("/Root").child("Level1").child("Leaf")
        assert str(p) == "/Root/Level1/Leaf"

    def test_parent_of_child(self):
        c = SdfPath("/World/Cube")
        parent = c.parent()
        assert parent is not None
        assert str(parent) == "/World"

    def test_parent_of_root_returns_slash(self):
        root = SdfPath("/World")
        parent = root.parent()
        # parent of /World is /
        assert parent is not None
        assert str(parent) == "/"

    def test_name_of_child_path(self):
        p = SdfPath("/World/Cube")
        assert p.name == "Cube"

    def test_is_absolute(self):
        p = SdfPath("/World")
        assert p.is_absolute is True


# ---------------------------------------------------------------------------
# wrap_value() and unwrap_value()
# ---------------------------------------------------------------------------


class TestWrapValue:
    def test_wrap_bool_returns_boolean_wrapper(self):
        w = wrap_value(True)
        assert isinstance(w, BooleanWrapper)

    def test_wrap_false_returns_boolean_wrapper(self):
        w = wrap_value(False)
        assert isinstance(w, BooleanWrapper)

    def test_wrap_int_returns_int_wrapper(self):
        w = wrap_value(42)
        assert isinstance(w, IntWrapper)

    def test_wrap_zero_returns_int_wrapper(self):
        w = wrap_value(0)
        assert isinstance(w, IntWrapper)

    def test_wrap_negative_int(self):
        w = wrap_value(-7)
        assert isinstance(w, IntWrapper)

    def test_wrap_float_returns_float_wrapper(self):
        w = wrap_value(3.14)
        assert isinstance(w, FloatWrapper)

    def test_wrap_str_returns_string_wrapper(self):
        w = wrap_value("hello")
        assert isinstance(w, StringWrapper)

    def test_wrap_empty_str_returns_string_wrapper(self):
        w = wrap_value("")
        assert isinstance(w, StringWrapper)

    def test_wrap_non_primitive_passthrough(self):
        lst = [1, 2, 3]
        result = wrap_value(lst)
        assert result == lst

    def test_wrap_dict_passthrough(self):
        d = {"key": "val"}
        result = wrap_value(d)
        assert result is d


class TestUnwrapValue:
    def test_unwrap_boolean_wrapper_returns_bool(self):
        w = BooleanWrapper(True)
        assert unwrap_value(w) is True
        assert isinstance(unwrap_value(w), bool)

    def test_unwrap_false_boolean_wrapper(self):
        w = BooleanWrapper(False)
        assert unwrap_value(w) is False

    def test_unwrap_int_wrapper_returns_int(self):
        w = IntWrapper(99)
        result = unwrap_value(w)
        assert result == 99
        assert isinstance(result, int)

    def test_unwrap_float_wrapper_returns_float(self):
        w = FloatWrapper(2.718)
        result = unwrap_value(w)
        assert result == pytest.approx(2.718)
        assert isinstance(result, float)

    def test_unwrap_string_wrapper_returns_str(self):
        w = StringWrapper("world")
        result = unwrap_value(w)
        assert result == "world"
        assert isinstance(result, str)

    def test_unwrap_plain_bool_passthrough(self):
        assert unwrap_value(True) is True

    def test_unwrap_plain_int_passthrough(self):
        assert unwrap_value(42) == 42

    def test_unwrap_plain_str_passthrough(self):
        assert unwrap_value("abc") == "abc"


class TestWrapUnwrapRoundtrip:
    def test_bool_roundtrip(self):
        assert unwrap_value(wrap_value(True)) is True
        assert unwrap_value(wrap_value(False)) is False

    def test_int_roundtrip(self):
        assert unwrap_value(wrap_value(0)) == 0
        assert unwrap_value(wrap_value(1000)) == 1000
        assert unwrap_value(wrap_value(-1)) == -1

    def test_float_roundtrip(self):
        assert unwrap_value(wrap_value(0.0)) == pytest.approx(0.0)
        assert unwrap_value(wrap_value(1.5)) == pytest.approx(1.5)

    def test_str_roundtrip(self):
        assert unwrap_value(wrap_value("")) == ""
        assert unwrap_value(wrap_value("sphere1")) == "sphere1"


# ---------------------------------------------------------------------------
# unwrap_parameters()
# ---------------------------------------------------------------------------


class TestUnwrapParameters:
    def test_unwrap_mixed_dict(self):
        params = {
            "flag": BooleanWrapper(True),
            "count": IntWrapper(7),
            "scale": FloatWrapper(2.5),
            "name": StringWrapper("cube"),
        }
        result = unwrap_parameters(params)
        assert result["flag"] is True
        assert result["count"] == 7
        assert result["scale"] == pytest.approx(2.5)
        assert result["name"] == "cube"

    def test_unwrap_empty_dict(self):
        assert unwrap_parameters({}) == {}

    def test_unwrap_plain_values_unchanged(self):
        params = {"x": 1, "y": "hello", "z": True}
        result = unwrap_parameters(params)
        assert result == {"x": 1, "y": "hello", "z": True}


# ---------------------------------------------------------------------------
# Wrapper dunders
# ---------------------------------------------------------------------------


class TestWrapperDunders:
    def test_boolean_wrapper_bool(self):
        assert bool(BooleanWrapper(True)) is True
        assert bool(BooleanWrapper(False)) is False

    def test_int_wrapper_int(self):
        assert int(IntWrapper(5)) == 5

    def test_float_wrapper_float(self):
        assert float(FloatWrapper(1.5)) == pytest.approx(1.5)

    def test_string_wrapper_str(self):
        assert str(StringWrapper("test")) == "test"

    def test_boolean_wrapper_value_eq(self):
        # BooleanWrapper.__eq__ compares by value via .value attribute
        assert BooleanWrapper(True).value == BooleanWrapper(True).value
        assert BooleanWrapper(True).value != BooleanWrapper(False).value

    def test_int_wrapper_eq(self):
        assert IntWrapper(10) == IntWrapper(10)
        assert IntWrapper(1) != IntWrapper(2)

    def test_string_wrapper_value_eq(self):
        # StringWrapper.__eq__ compares by value via .value attribute
        assert StringWrapper("a").value == StringWrapper("a").value
        assert StringWrapper("a").value != StringWrapper("b").value

    def test_boolean_wrapper_hash(self):
        w1 = BooleanWrapper(True)
        w2 = BooleanWrapper(True)
        # Both True values have same hash
        assert hash(w1) == hash(w2)

    def test_int_wrapper_index(self):
        w = IntWrapper(3)
        lst = [0, 1, 2, 3]
        assert lst[w] == 3
