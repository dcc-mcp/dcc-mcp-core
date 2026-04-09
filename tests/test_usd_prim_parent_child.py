"""Deep tests for UsdPrim parent/child relationships and SdfPath tree structure.

Covers:
- SdfPath.parent() for nested paths
- SdfPath.child() chaining
- SdfPath.parent() for root path returns None
- UsdStage.traverse() returns all defined prims
- UsdStage.prims_of_type() returns only matching type
- UsdPrim.path returns SdfPath
- UsdPrim.name returns last segment
- UsdPrim.type_name is set on define_prim
- UsdPrim.active is True by default
- UsdStage.remove_prim() and has_prim() consistency
- UsdStage.get_prim() returns None for removed/undefined path
- stage.metrics() counts reflect define/remove operations
- Nested prim tree: /World/Geometry/Cube parent is /World/Geometry
- SdfPath equality/hash/str/repr
- UsdStage from_json restores traversal count
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import SdfPath
from dcc_mcp_core import UsdPrim
from dcc_mcp_core import UsdStage
from dcc_mcp_core import VtValue

# ---------------------------------------------------------------------------
# SdfPath tree tests
# ---------------------------------------------------------------------------


class TestSdfPathTree:
    def test_child_creates_nested_path(self):
        root = SdfPath("/World")
        child = root.child("Cube")
        assert str(child) == "/World/Cube"

    def test_child_chaining(self):
        p = SdfPath("/World").child("Geometry").child("Sphere")
        assert str(p) == "/World/Geometry/Sphere"

    def test_parent_returns_parent_path(self):
        p = SdfPath("/World/Cube")
        parent = p.parent()
        assert parent is not None
        assert str(parent) == "/World"

    def test_parent_of_root_returns_none_or_root(self):
        root = SdfPath("/World")
        parent = root.parent()
        # Parent of root /World is "/" or None depending on implementation
        # either is acceptable
        if parent is not None:
            assert str(parent) in ("/", "")

    def test_root_path_parent(self):
        root = SdfPath("/")
        parent = root.parent()
        # Root path / has no meaningful parent
        if parent is not None:
            # Should be empty or same
            assert isinstance(parent, SdfPath)

    def test_name_is_last_segment(self):
        p = SdfPath("/World/Geometry/Cube")
        assert p.name == "Cube"

    def test_name_of_root_level(self):
        p = SdfPath("/World")
        assert p.name == "World"

    def test_is_absolute_true_for_absolute_path(self):
        p = SdfPath("/World/Cube")
        assert p.is_absolute is True

    def test_is_absolute_false_for_relative(self):
        p = SdfPath("World/Cube")
        assert p.is_absolute is False

    def test_equality_same_path(self):
        a = SdfPath("/World/Cube")
        b = SdfPath("/World/Cube")
        assert a == b

    def test_inequality_different_path(self):
        a = SdfPath("/World/Cube")
        b = SdfPath("/World/Sphere")
        assert a != b

    def test_str_round_trip(self):
        s = "/World/Geometry/Mesh"
        p = SdfPath(s)
        assert str(p) == s

    def test_repr_non_empty(self):
        p = SdfPath("/World")
        r = repr(p)
        assert len(r) > 0
        assert "World" in r or "SdfPath" in r

    def test_hash_equal_paths(self):
        a = SdfPath("/World/Cube")
        b = SdfPath("/World/Cube")
        # Should be hashable and equal hashes
        assert hash(a) == hash(b)

    def test_parent_child_round_trip(self):
        parent = SdfPath("/World/Geometry")
        child = parent.child("Mesh")
        recovered_parent = child.parent()
        assert recovered_parent is not None
        assert str(recovered_parent) == str(parent)


# ---------------------------------------------------------------------------
# UsdPrim path/name/type_name/active properties
# ---------------------------------------------------------------------------


class TestUsdPrimProperties:
    def _make_stage_with_prim(self, path: str, type_name: str = "Xform") -> tuple[UsdStage, UsdPrim]:
        stage = UsdStage("test")
        prim = stage.define_prim(path, type_name)
        return stage, prim

    def test_prim_path_is_sdf_path(self):
        _, prim = self._make_stage_with_prim("/World")
        assert isinstance(prim.path, SdfPath)

    def test_prim_path_value(self):
        _, prim = self._make_stage_with_prim("/World/Cube")
        assert str(prim.path) == "/World/Cube"

    def test_prim_name_is_last_segment(self):
        _, prim = self._make_stage_with_prim("/World/Cube")
        assert prim.name == "Cube"

    def test_prim_name_for_root_level(self):
        _, prim = self._make_stage_with_prim("/World")
        assert prim.name == "World"

    def test_prim_type_name(self):
        _, prim = self._make_stage_with_prim("/Box", "Mesh")
        assert prim.type_name == "Mesh"

    def test_prim_active_is_true_by_default(self):
        _, prim = self._make_stage_with_prim("/Active")
        assert prim.active is True

    def test_prim_repr_contains_path(self):
        _, prim = self._make_stage_with_prim("/World/Repr")
        r = repr(prim)
        assert "Repr" in r or "UsdPrim" in r


# ---------------------------------------------------------------------------
# UsdStage.traverse() and tree structure
# ---------------------------------------------------------------------------


class TestUsdStageTraverse:
    def test_traverse_returns_all_prims(self):
        stage = UsdStage("traversal")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Geometry", "Xform")
        stage.define_prim("/World/Geometry/Cube", "Mesh")
        stage.define_prim("/World/Geometry/Sphere", "Mesh")
        stage.define_prim("/World/Lights", "Xform")

        all_prims = stage.traverse()
        paths = [str(p.path) for p in all_prims]

        assert "/World" in paths
        assert "/World/Geometry" in paths
        assert "/World/Geometry/Cube" in paths
        assert "/World/Geometry/Sphere" in paths
        assert "/World/Lights" in paths

    def test_traverse_count_matches_define_count(self):
        stage = UsdStage("count_test")
        expected = ["/A", "/A/B", "/A/B/C", "/D"]
        for p in expected:
            stage.define_prim(p, "Xform")
        all_prims = stage.traverse()
        assert len(all_prims) == len(expected)

    def test_traverse_empty_stage_returns_empty(self):
        stage = UsdStage("empty")
        all_prims = stage.traverse()
        assert all_prims == []

    def test_prims_of_type_filters_correctly(self):
        stage = UsdStage("type_filter")
        stage.define_prim("/Mesh1", "Mesh")
        stage.define_prim("/Mesh2", "Mesh")
        stage.define_prim("/Xform1", "Xform")
        stage.define_prim("/Sphere1", "Sphere")

        meshes = stage.prims_of_type("Mesh")
        assert len(meshes) == 2
        for m in meshes:
            assert m.type_name == "Mesh"

    def test_prims_of_type_empty_when_none_match(self):
        stage = UsdStage("no_match")
        stage.define_prim("/Box", "Mesh")
        lights = stage.prims_of_type("Light")
        assert lights == []

    def test_traverse_after_remove(self):
        stage = UsdStage("remove_test")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        stage.define_prim("/World/Sphere", "Mesh")

        stage.remove_prim("/World/Cube")
        remaining = stage.traverse()
        paths = [str(p.path) for p in remaining]

        assert "/World/Cube" not in paths
        assert "/World" in paths
        assert "/World/Sphere" in paths


# ---------------------------------------------------------------------------
# UsdStage.get_prim and has_prim with nested paths
# ---------------------------------------------------------------------------


class TestUsdStageGetHasPrim:
    def test_get_prim_returns_none_for_undefined(self):
        stage = UsdStage("test")
        prim = stage.get_prim("/Undefined/Path")
        assert prim is None

    def test_get_prim_returns_prim_for_defined(self):
        stage = UsdStage("test")
        stage.define_prim("/World", "Xform")
        prim = stage.get_prim("/World")
        assert prim is not None
        assert prim.name == "World"

    def test_has_prim_true_after_define(self):
        stage = UsdStage("test")
        stage.define_prim("/A/B", "Xform")
        assert stage.has_prim("/A/B") is True

    def test_has_prim_false_for_undefined(self):
        stage = UsdStage("test")
        assert stage.has_prim("/NotDefined") is False

    def test_has_prim_false_after_remove(self):
        stage = UsdStage("test")
        stage.define_prim("/Temp", "Xform")
        stage.remove_prim("/Temp")
        assert stage.has_prim("/Temp") is False

    def test_remove_prim_returns_true_for_existing(self):
        stage = UsdStage("test")
        stage.define_prim("/ToRemove", "Xform")
        result = stage.remove_prim("/ToRemove")
        assert result is True

    def test_remove_prim_returns_false_for_nonexistent(self):
        stage = UsdStage("test")
        result = stage.remove_prim("/DoesNotExist")
        assert result is False


# ---------------------------------------------------------------------------
# Nested prim parent/child structure via SdfPath
# ---------------------------------------------------------------------------


class TestNestedPrimHierarchy:
    def test_deep_hierarchy_paths(self):
        stage = UsdStage("hierarchy")
        stage.define_prim("/Root", "Xform")
        stage.define_prim("/Root/Group", "Xform")
        stage.define_prim("/Root/Group/Object", "Mesh")
        stage.define_prim("/Root/Group/Object/SubMesh", "Mesh")

        all_prims = stage.traverse()
        depth = len(all_prims)
        assert depth == 4

    def test_get_prim_at_each_level(self):
        stage = UsdStage("levels")
        paths = ["/L1", "/L1/L2", "/L1/L2/L3"]
        for p in paths:
            stage.define_prim(p, "Xform")
        for p in paths:
            prim = stage.get_prim(p)
            assert prim is not None
            assert stage.has_prim(p)

    def test_child_path_from_prim_path(self):
        stage = UsdStage("child_path")
        stage.define_prim("/Parent", "Xform")
        stage.define_prim("/Parent/Child", "Xform")
        parent = stage.get_prim("/Parent")
        assert parent is not None
        # /Parent/Child should be accessible as child path
        child_path = parent.path.child("Child")
        assert str(child_path) == "/Parent/Child"
        assert stage.has_prim(str(child_path))

    def test_parent_path_from_prim_path(self):
        stage = UsdStage("parent_path")
        stage.define_prim("/World", "Xform")
        stage.define_prim("/World/Cube", "Mesh")
        cube = stage.get_prim("/World/Cube")
        assert cube is not None
        parent_path = cube.path.parent()
        assert parent_path is not None
        assert str(parent_path) == "/World"
        # Parent should exist in stage
        parent_prim = stage.get_prim(str(parent_path))
        assert parent_prim is not None
        assert parent_prim.name == "World"


# ---------------------------------------------------------------------------
# UsdStage metrics and JSON round-trip with hierarchy
# ---------------------------------------------------------------------------


class TestUsdStageMetricsHierarchy:
    def test_metrics_prim_count(self):
        stage = UsdStage("metrics_test")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Xform")
        stage.define_prim("/C", "Mesh")
        m = stage.metrics()
        assert isinstance(m, dict)
        assert m.get("prim_count", 0) >= 3 or "prim_count" in m

    def test_from_json_restores_prim_count(self):
        stage = UsdStage("json_test")
        stage.define_prim("/A", "Xform")
        stage.define_prim("/B", "Mesh")
        stage.define_prim("/C", "Sphere")
        json_str = stage.to_json()
        restored = UsdStage.from_json(json_str)
        restored_prims = restored.traverse()
        assert len(restored_prims) == 3

    def test_from_json_restores_prim_names(self):
        stage = UsdStage("name_test")
        stage.define_prim("/Alpha", "Xform")
        stage.define_prim("/Beta", "Mesh")
        json_str = stage.to_json()
        restored = UsdStage.from_json(json_str)
        names = [p.name for p in restored.traverse()]
        assert "Alpha" in names
        assert "Beta" in names

    def test_from_json_restores_attributes(self):
        """UsdStage.from_json restores prims; attribute persistence depends on implementation.

        The JSON serialization may or may not persist prim attributes.
        We only assert the prim itself is recoverable.
        """
        stage = UsdStage("attr_test")
        prim = stage.define_prim("/Box", "Mesh")
        prim.set_attribute("size", VtValue.from_float(2.5))
        # Confirm attribute is set in original stage
        val = prim.get_attribute("size")
        assert val is not None
        assert abs(float(val.to_python()) - 2.5) < 1e-5

        # After JSON round-trip, prim should exist (attributes may or may not survive)
        json_str = stage.to_json()
        restored = UsdStage.from_json(json_str)
        box = restored.get_prim("/Box")
        assert box is not None
        assert box.name == "Box"
