"""Tests for SdfPath hierarchy and UsdStage attribute set/get with VtValue.

Covers: SdfPath (parent/name/is_absolute/child) hierarchy traversal,
and UsdStage.set_attribute/get_attribute roundtrip for all VtValue types.
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import SdfPath
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ===========================================================================
# SdfPath tests
# ===========================================================================


class TestSdfPathBasicConstruction:
    """Tests for SdfPath construction and basic properties."""

    def test_absolute_path_name(self):
        p = SdfPath("/World/Sphere")
        assert p.name == "Sphere"

    def test_absolute_path_is_absolute(self):
        p = SdfPath("/World/Sphere")
        assert p.is_absolute is True

    def test_relative_path_is_not_absolute(self):
        p = SdfPath("Sphere")
        assert p.is_absolute is False

    def test_root_path_name_empty(self):
        p = SdfPath("/")
        assert p.name == ""

    def test_single_level_path_name(self):
        p = SdfPath("/World")
        assert p.name == "World"

    def test_three_level_path_name(self):
        p = SdfPath("/World/Sphere/Material")
        assert p.name == "Material"

    def test_path_string_representation(self):
        p = SdfPath("/World/Sphere")
        s = str(p)
        assert "Sphere" in s

    def test_root_path_is_absolute(self):
        p = SdfPath("/")
        assert p.is_absolute is True


class TestSdfPathParent:
    """Tests for SdfPath.parent() method."""

    def test_parent_returns_sdf_path(self):
        p = SdfPath("/World/Sphere")
        parent = p.parent()
        assert isinstance(parent, SdfPath)

    def test_parent_of_two_level_path(self):
        p = SdfPath("/World/Sphere")
        parent = p.parent()
        assert parent.name == "World"

    def test_parent_of_three_level_path(self):
        p = SdfPath("/World/Sphere/Material")
        parent = p.parent()
        assert parent.name == "Sphere"

    def test_parent_chain_to_root(self):
        p = SdfPath("/World/Sphere")
        grand = p.parent().parent()
        # /World's parent is /
        assert grand.name == ""

    def test_parent_of_single_level_is_root(self):
        p = SdfPath("/World")
        parent = p.parent()
        assert parent.name == ""

    def test_grandparent_matches_expected(self):
        p = SdfPath("/A/B/C")
        assert p.parent().name == "B"
        assert p.parent().parent().name == "A"

    def test_parent_preserves_is_absolute(self):
        p = SdfPath("/World/Sphere")
        parent = p.parent()
        assert parent.is_absolute is True


class TestSdfPathChild:
    """Tests for SdfPath.child() method."""

    def test_child_returns_sdf_path(self):
        p = SdfPath("/World")
        child = p.child("Sphere")
        assert isinstance(child, SdfPath)

    def test_child_name_is_correct(self):
        p = SdfPath("/World")
        child = p.child("Sphere")
        assert child.name == "Sphere"

    def test_child_parent_is_original(self):
        p = SdfPath("/World")
        child = p.child("Sphere")
        assert child.parent().name == "World"

    def test_child_of_root(self):
        root = SdfPath("/")
        child = root.child("World")
        assert child.name == "World"

    def test_chained_child_calls(self):
        p = SdfPath("/")
        result = p.child("World").child("Sphere").child("Material")
        assert result.name == "Material"
        assert result.parent().name == "Sphere"

    def test_child_is_absolute(self):
        p = SdfPath("/World")
        child = p.child("Sphere")
        assert child.is_absolute is True

    def test_multiple_children_different_names(self):
        base = SdfPath("/World")
        c1 = base.child("Sphere")
        c2 = base.child("Cube")
        c3 = base.child("Camera")
        assert c1.name == "Sphere"
        assert c2.name == "Cube"
        assert c3.name == "Camera"


class TestSdfPathHierarchy:
    """Tests for SdfPath deep hierarchy traversal."""

    def test_five_level_hierarchy(self):
        p = SdfPath("/A/B/C/D/E")
        assert p.name == "E"
        assert p.parent().name == "D"
        assert p.parent().parent().name == "C"
        assert p.parent().parent().parent().name == "B"
        assert p.parent().parent().parent().parent().name == "A"

    def test_child_builds_correct_hierarchy(self):
        root = SdfPath("/")
        p = root.child("A").child("B").child("C")
        assert p.name == "C"
        assert p.parent().name == "B"
        assert p.parent().parent().name == "A"

    def test_path_and_child_are_consistent(self):
        p = SdfPath("/World/Sphere")
        child = p.child("Mat")
        # parent of Mat should have same name as p
        assert child.parent().name == p.name


# ===========================================================================
# UsdStage set_attribute / get_attribute tests
# ===========================================================================


class TestUsdStageSetGetAttributeFloat:
    """Tests for UsdStage float attribute operations."""

    def test_set_get_float(self):
        stage = UsdStage("FloatAttr")
        stage.define_prim("/World", "Xform")
        stage.set_attribute("/World", "radius", VtValue.from_float(5.0))
        v = stage.get_attribute("/World", "radius")
        assert v.to_python() == pytest.approx(5.0)

    def test_set_get_zero_float(self):
        stage = UsdStage("ZeroFloat")
        stage.define_prim("/World", "Xform")
        stage.set_attribute("/World", "scale", VtValue.from_float(0.0))
        v = stage.get_attribute("/World", "scale")
        assert v.to_python() == pytest.approx(0.0)

    def test_set_get_negative_float(self):
        stage = UsdStage("NegFloat")
        stage.define_prim("/World", "Xform")
        stage.set_attribute("/World", "offset", VtValue.from_float(-3.14))
        v = stage.get_attribute("/World", "offset")
        assert v.to_python() == pytest.approx(-3.14)

    def test_float_type_name(self):
        v = VtValue.from_float(1.0)
        assert "float" in v.type_name.lower()

    def test_overwrite_float_attribute(self):
        stage = UsdStage("OverwriteFloat")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "size", VtValue.from_float(1.0))
        stage.set_attribute("/Root", "size", VtValue.from_float(99.0))
        v = stage.get_attribute("/Root", "size")
        assert v.to_python() == pytest.approx(99.0)


class TestUsdStageSetGetAttributeInt:
    """Tests for UsdStage int attribute operations."""

    def test_set_get_int(self):
        stage = UsdStage("IntAttr")
        stage.define_prim("/Node", "Xform")
        stage.set_attribute("/Node", "count", VtValue.from_int(42))
        v = stage.get_attribute("/Node", "count")
        assert v.to_python() == 42

    def test_set_get_zero_int(self):
        stage = UsdStage("ZeroInt")
        stage.define_prim("/Node", "Xform")
        stage.set_attribute("/Node", "index", VtValue.from_int(0))
        v = stage.get_attribute("/Node", "index")
        assert v.to_python() == 0

    def test_set_get_negative_int(self):
        stage = UsdStage("NegInt")
        stage.define_prim("/Node", "Xform")
        stage.set_attribute("/Node", "delta", VtValue.from_int(-7))
        v = stage.get_attribute("/Node", "delta")
        assert v.to_python() == -7

    def test_int_type_name(self):
        v = VtValue.from_int(1)
        assert "int" in v.type_name.lower()

    def test_overwrite_int_attribute(self):
        stage = UsdStage("OverwriteInt")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "steps", VtValue.from_int(1))
        stage.set_attribute("/Root", "steps", VtValue.from_int(100))
        v = stage.get_attribute("/Root", "steps")
        assert v.to_python() == 100


class TestUsdStageSetGetAttributeString:
    """Tests for UsdStage string attribute operations."""

    def test_set_get_string(self):
        stage = UsdStage("StrAttr")
        stage.define_prim("/Label", "Xform")
        stage.set_attribute("/Label", "text", VtValue.from_string("hello"))
        v = stage.get_attribute("/Label", "text")
        assert v.to_python() == "hello"

    def test_set_get_empty_string(self):
        stage = UsdStage("EmptyStr")
        stage.define_prim("/Label", "Xform")
        stage.set_attribute("/Label", "text", VtValue.from_string(""))
        v = stage.get_attribute("/Label", "text")
        assert v.to_python() == ""

    def test_string_type_name(self):
        v = VtValue.from_string("test")
        assert "string" in v.type_name.lower()

    def test_overwrite_string_attribute(self):
        stage = UsdStage("OverwriteStr")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "name", VtValue.from_string("old"))
        stage.set_attribute("/Root", "name", VtValue.from_string("new"))
        v = stage.get_attribute("/Root", "name")
        assert v.to_python() == "new"


class TestUsdStageSetGetAttributeBool:
    """Tests for UsdStage bool attribute operations."""

    def test_set_get_bool_true(self):
        stage = UsdStage("BoolAttr")
        stage.define_prim("/Flag", "Xform")
        stage.set_attribute("/Flag", "visible", VtValue.from_bool(True))
        v = stage.get_attribute("/Flag", "visible")
        assert v.to_python() is True

    def test_set_get_bool_false(self):
        stage = UsdStage("BoolFalse")
        stage.define_prim("/Flag", "Xform")
        stage.set_attribute("/Flag", "hidden", VtValue.from_bool(False))
        v = stage.get_attribute("/Flag", "hidden")
        assert v.to_python() is False

    def test_bool_type_name(self):
        v = VtValue.from_bool(True)
        assert "bool" in v.type_name.lower()

    def test_overwrite_bool_attribute(self):
        stage = UsdStage("OverwriteBool")
        stage.define_prim("/Root", "Xform")
        stage.set_attribute("/Root", "active", VtValue.from_bool(True))
        stage.set_attribute("/Root", "active", VtValue.from_bool(False))
        v = stage.get_attribute("/Root", "active")
        assert v.to_python() is False


class TestUsdStageSetGetAttributeMultiple:
    """Tests for multiple attributes on the same prim."""

    def test_multiple_attributes_different_types(self):
        stage = UsdStage("MultiAttr")
        stage.define_prim("/Prim", "Xform")
        stage.set_attribute("/Prim", "radius", VtValue.from_float(5.0))
        stage.set_attribute("/Prim", "count", VtValue.from_int(3))
        stage.set_attribute("/Prim", "label", VtValue.from_string("sphere"))
        stage.set_attribute("/Prim", "visible", VtValue.from_bool(True))

        assert stage.get_attribute("/Prim", "radius").to_python() == pytest.approx(5.0)
        assert stage.get_attribute("/Prim", "count").to_python() == 3
        assert stage.get_attribute("/Prim", "label").to_python() == "sphere"
        assert stage.get_attribute("/Prim", "visible").to_python() is True

    def test_attributes_on_nested_prims(self):
        stage = UsdStage("NestedAttr")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Sphere", "Sphere")
        stage.set_attribute("/World", "scale", VtValue.from_float(1.0))
        stage.set_attribute("/World/Sphere", "radius", VtValue.from_float(2.5))

        v1 = stage.get_attribute("/World", "scale")
        v2 = stage.get_attribute("/World/Sphere", "radius")
        assert v1.to_python() == pytest.approx(1.0)
        assert v2.to_python() == pytest.approx(2.5)

    def test_attribute_not_shared_between_prims(self):
        stage = UsdStage("IsolatedAttr")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        stage.set_attribute("/A", "size", VtValue.from_float(10.0))
        stage.set_attribute("/B", "size", VtValue.from_float(20.0))

        v_a = stage.get_attribute("/A", "size")
        v_b = stage.get_attribute("/B", "size")
        assert v_a.to_python() == pytest.approx(10.0)
        assert v_b.to_python() == pytest.approx(20.0)


class TestUsdStageGetAttributeMissing:
    """Tests for get_attribute on non-existent attributes."""

    def test_get_nonexistent_attribute_returns_none_or_raises(self):
        stage = UsdStage("MissingAttr")
        stage.define_prim("/Prim", "Xform")
        try:
            v = stage.get_attribute("/Prim", "nonexistent_attr")
            # If it returns, should be None or a VtValue
            assert v is None or isinstance(v, VtValue)
        except (RuntimeError, AttributeError):
            pass  # also acceptable

    def test_get_attribute_from_nonexistent_prim_raises(self):
        """get_attribute raises ValueError when prim does not exist."""
        stage = UsdStage("MissingPrim")
        with pytest.raises((ValueError, RuntimeError)):
            stage.get_attribute("/NonExistentPrim", "attr")


# ===========================================================================
# VtValue standalone tests
# ===========================================================================


class TestVtValueTypeName:
    """Tests for VtValue.type_name."""

    def test_float_type_name_contains_float(self):
        v = VtValue.from_float(1.0)
        assert "float" in v.type_name.lower()

    def test_int_type_name_contains_int(self):
        v = VtValue.from_int(1)
        assert "int" in v.type_name.lower()

    def test_bool_type_name_contains_bool(self):
        v = VtValue.from_bool(True)
        assert "bool" in v.type_name.lower()

    def test_string_type_name_contains_string(self):
        v = VtValue.from_string("x")
        assert "string" in v.type_name.lower()

    def test_token_type_name_contains_token(self):
        v = VtValue.from_token("myToken")
        assert "token" in v.type_name.lower()

    def test_vec3f_type_name(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        assert v.type_name is not None


class TestVtValueToPython:
    """Tests for VtValue.to_python conversion."""

    def test_float_to_python(self):
        v = VtValue.from_float(3.14)
        assert abs(v.to_python() - 3.14) < 1e-6

    def test_int_to_python(self):
        v = VtValue.from_int(99)
        assert v.to_python() == 99

    def test_bool_true_to_python(self):
        v = VtValue.from_bool(True)
        assert v.to_python() is True

    def test_bool_false_to_python(self):
        v = VtValue.from_bool(False)
        assert v.to_python() is False

    def test_string_to_python(self):
        v = VtValue.from_string("hello world")
        assert v.to_python() == "hello world"

    def test_token_to_python_is_string(self):
        v = VtValue.from_token("myToken")
        result = v.to_python()
        assert isinstance(result, str)
        assert "myToken" in result

    def test_vec3f_to_python_returns_something(self):
        v = VtValue.from_vec3f(1.0, 2.0, 3.0)
        result = v.to_python()
        assert result is not None
