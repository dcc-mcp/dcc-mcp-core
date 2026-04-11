"""Deep tests for VtValue, SdfPath, type wrappers, McpHttpConfig/McpHttpServer, and scan_and_load.

Covers: VtValue all factory methods, SdfPath path operations/equality/hashing,
BooleanWrapper/IntWrapper/FloatWrapper/StringWrapper value/cast/repr,
wrap_value/unwrap_value/unwrap_parameters dispatch matrix,
McpHttpConfig port/server_name/server_version properties,
McpHttpServer register_handler/has_handler/catalog API,
scan_and_load and scan_and_load_lenient with real examples/skills directory.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import SdfPath
from dcc_mcp_core import StringWrapper
from dcc_mcp_core import VtValue
from dcc_mcp_core import scan_and_load
from dcc_mcp_core import scan_and_load_lenient
from dcc_mcp_core import unwrap_parameters
from dcc_mcp_core import unwrap_value
from dcc_mcp_core import wrap_value

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

EXAMPLES_SKILLS_DIR = str(Path(__file__).parent.parent / "examples" / "skills")


# ===========================================================================
# VtValue — all factory methods
# ===========================================================================


class TestVtValueFromBool:
    def test_from_bool_true_type_name(self):
        assert VtValue.from_bool(True).type_name == "bool"

    def test_from_bool_false_type_name(self):
        assert VtValue.from_bool(False).type_name == "bool"

    def test_from_bool_true_to_python(self):
        assert VtValue.from_bool(True).to_python() is True

    def test_from_bool_false_to_python(self):
        assert VtValue.from_bool(False).to_python() is False

    def test_from_bool_repr_contains_bool(self):
        r = repr(VtValue.from_bool(True))
        assert "bool" in r.lower() or "Bool" in r


class TestVtValueFromInt:
    def test_type_name(self):
        assert VtValue.from_int(42).type_name == "int"

    def test_to_python_positive(self):
        assert VtValue.from_int(100).to_python() == 100

    def test_to_python_zero(self):
        assert VtValue.from_int(0).to_python() == 0

    def test_to_python_negative(self):
        assert VtValue.from_int(-5).to_python() == -5

    def test_repr_contains_int(self):
        r = repr(VtValue.from_int(7))
        assert "int" in r.lower() or "Int" in r


class TestVtValueFromFloat:
    def test_type_name(self):
        assert VtValue.from_float(3.14).type_name == "float"

    def test_to_python_approx(self):
        v = VtValue.from_float(1.5)
        assert abs(v.to_python() - 1.5) < 1e-5

    def test_to_python_zero(self):
        v = VtValue.from_float(0.0)
        assert v.to_python() == pytest.approx(0.0)

    def test_to_python_negative(self):
        v = VtValue.from_float(-2.5)
        assert v.to_python() == pytest.approx(-2.5, abs=1e-5)

    def test_repr_contains_float(self):
        r = repr(VtValue.from_float(1.0))
        assert "float" in r.lower() or "Float" in r


class TestVtValueFromString:
    def test_type_name(self):
        assert VtValue.from_string("hello").type_name == "string"

    def test_to_python_hello(self):
        assert VtValue.from_string("hello").to_python() == "hello"

    def test_to_python_empty(self):
        assert VtValue.from_string("").to_python() == ""

    def test_to_python_unicode(self):
        assert VtValue.from_string("日本語").to_python() == "日本語"


class TestVtValueFromToken:
    def test_type_name(self):
        assert VtValue.from_token("Mesh").type_name == "token"

    def test_to_python(self):
        assert VtValue.from_token("Mesh").to_python() == "Mesh"

    def test_to_python_empty_token(self):
        assert VtValue.from_token("").to_python() == ""

    def test_distinct_from_string(self):
        # token and string have different type_names
        assert VtValue.from_token("X").type_name != VtValue.from_string("X").type_name


class TestVtValueFromAsset:
    def test_type_name(self):
        assert VtValue.from_asset("/path/to/file.usd").type_name == "asset"

    def test_to_python(self):
        path = "/project/scene.usda"
        assert VtValue.from_asset(path).to_python() == path

    def test_empty_asset(self):
        v = VtValue.from_asset("")
        assert v.type_name == "asset"


class TestVtValueFromVec3f:
    def test_type_name(self):
        assert VtValue.from_vec3f(1.0, 2.0, 3.0).type_name == "float3"

    def test_to_python_is_tuple(self):
        result = VtValue.from_vec3f(1.0, 2.0, 3.0).to_python()
        assert isinstance(result, tuple)
        assert len(result) == 3

    def test_to_python_values(self):
        x, y, z = VtValue.from_vec3f(4.0, 5.0, 6.0).to_python()
        assert x == pytest.approx(4.0)
        assert y == pytest.approx(5.0)
        assert z == pytest.approx(6.0)

    def test_to_python_zeros(self):
        x, y, z = VtValue.from_vec3f(0.0, 0.0, 0.0).to_python()
        assert (x, y, z) == (pytest.approx(0.0), pytest.approx(0.0), pytest.approx(0.0))

    def test_to_python_negative(self):
        x, _y, _z = VtValue.from_vec3f(-1.0, -2.0, -3.0).to_python()
        assert x == pytest.approx(-1.0)


# ===========================================================================
# SdfPath — construction, path operations, equality, hashing
# ===========================================================================


class TestSdfPathBasic:
    def test_root_path_name_empty(self):
        p = SdfPath("/")
        # Root has no component name
        assert isinstance(p.name, str)

    def test_single_level_name(self):
        assert SdfPath("/World").name == "World"

    def test_is_absolute(self):
        assert SdfPath("/World").is_absolute is True

    def test_str_roundtrip(self):
        assert str(SdfPath("/World")) == "/World"

    def test_repr_format(self):
        r = repr(SdfPath("/World"))
        assert "/World" in r


class TestSdfPathChild:
    def test_child_path_str(self):
        child = SdfPath("/World").child("Cube")
        assert str(child) == "/World/Cube"

    def test_child_name(self):
        assert SdfPath("/World").child("Cube").name == "Cube"

    def test_child_is_absolute(self):
        assert SdfPath("/World").child("Cube").is_absolute is True

    def test_deep_child(self):
        p = SdfPath("/World").child("Group").child("Mesh")
        assert str(p) == "/World/Group/Mesh"
        assert p.name == "Mesh"


class TestSdfPathParent:
    def test_parent_of_child(self):
        parent = SdfPath("/World/Cube").parent()
        assert str(parent) == "/World"

    def test_parent_name(self):
        assert SdfPath("/World/Cube").parent().name == "World"

    def test_parent_of_root(self):
        # Parent of root-like paths — should not raise
        p = SdfPath("/World")
        result = p.parent()
        assert result is not None


class TestSdfPathEqualityAndHash:
    def test_eq_same_path(self):
        assert SdfPath("/World/Cube") == SdfPath("/World/Cube")

    def test_ne_different_path(self):
        assert SdfPath("/World/Cube") != SdfPath("/World/Sphere")

    def test_ne_parent(self):
        assert SdfPath("/World/Cube") != SdfPath("/World")

    def test_hash_same_path_equal(self):
        assert hash(SdfPath("/World/Cube")) == hash(SdfPath("/World/Cube"))

    def test_hash_different_paths_likely_different(self):
        # hashes are stable per path; just verify no exceptions
        h1 = hash(SdfPath("/World/Cube"))
        h2 = hash(SdfPath("/World/Sphere"))
        assert isinstance(h1, int) and isinstance(h2, int)

    def test_usable_as_dict_key(self):
        d = {SdfPath("/A"): 1, SdfPath("/B"): 2}
        assert d[SdfPath("/A")] == 1
        assert d[SdfPath("/B")] == 2

    def test_usable_in_set(self):
        s = {SdfPath("/A"), SdfPath("/A"), SdfPath("/B")}
        assert len(s) == 2


# ===========================================================================
# BooleanWrapper
# ===========================================================================


class TestBooleanWrapper:
    def test_value_true(self):
        assert BooleanWrapper(True).value is True

    def test_value_false(self):
        assert BooleanWrapper(False).value is False

    def test_repr_contains_true(self):
        assert "True" in repr(BooleanWrapper(True))

    def test_repr_contains_false(self):
        assert "False" in repr(BooleanWrapper(False))

    def test_hash_is_int(self):
        assert isinstance(hash(BooleanWrapper(True)), int)

    def test_hash_stable(self):
        assert hash(BooleanWrapper(True)) == hash(BooleanWrapper(True))


# ===========================================================================
# IntWrapper
# ===========================================================================


class TestIntWrapper:
    def test_value(self):
        assert IntWrapper(42).value == 42

    def test_zero(self):
        assert IntWrapper(0).value == 0

    def test_negative(self):
        assert IntWrapper(-99).value == -99

    def test_int_cast(self):
        assert int(IntWrapper(5)) == 5

    def test_index(self):
        lst = [10, 20, 30]
        assert lst[IntWrapper(1)] == 20

    def test_repr_contains_value(self):
        assert "42" in repr(IntWrapper(42))

    def test_eq(self):
        assert IntWrapper(7) == IntWrapper(7)

    def test_ne(self):
        assert IntWrapper(7) != IntWrapper(8)

    def test_hash(self):
        assert isinstance(hash(IntWrapper(10)), int)

    def test_hash_consistent(self):
        assert hash(IntWrapper(10)) == hash(IntWrapper(10))


# ===========================================================================
# FloatWrapper
# ===========================================================================


class TestFloatWrapper:
    def test_value(self):
        assert FloatWrapper(3.14).value == pytest.approx(3.14)

    def test_zero(self):
        assert FloatWrapper(0.0).value == pytest.approx(0.0)

    def test_negative(self):
        assert FloatWrapper(-1.5).value == pytest.approx(-1.5)

    def test_float_cast(self):
        assert float(FloatWrapper(2.5)) == pytest.approx(2.5)

    def test_repr_contains_value(self):
        assert "3.14" in repr(FloatWrapper(3.14))


# ===========================================================================
# StringWrapper
# ===========================================================================


class TestStringWrapper:
    def test_value(self):
        assert StringWrapper("hello").value == "hello"

    def test_empty(self):
        assert StringWrapper("").value == ""

    def test_str_cast(self):
        assert str(StringWrapper("world")) == "world"

    def test_repr_contains_value(self):
        assert "hello" in repr(StringWrapper("hello"))

    def test_hash(self):
        assert isinstance(hash(StringWrapper("abc")), int)

    def test_hash_consistent(self):
        assert hash(StringWrapper("x")) == hash(StringWrapper("x"))


# ===========================================================================
# wrap_value dispatch
# ===========================================================================


class TestWrapValueDispatch:
    def test_wrap_true_gives_boolean_wrapper(self):
        assert isinstance(wrap_value(True), BooleanWrapper)

    def test_wrap_false_gives_boolean_wrapper(self):
        assert isinstance(wrap_value(False), BooleanWrapper)

    def test_wrap_int_gives_int_wrapper(self):
        assert isinstance(wrap_value(42), IntWrapper)

    def test_wrap_zero_gives_int_wrapper(self):
        assert isinstance(wrap_value(0), IntWrapper)

    def test_wrap_float_gives_float_wrapper(self):
        assert isinstance(wrap_value(3.14), FloatWrapper)

    def test_wrap_string_gives_string_wrapper(self):
        assert isinstance(wrap_value("hello"), StringWrapper)

    def test_wrap_empty_string_gives_string_wrapper(self):
        assert isinstance(wrap_value(""), StringWrapper)

    def test_wrap_list_passthrough(self):
        val = [1, 2, 3]
        assert wrap_value(val) is val

    def test_wrap_dict_passthrough(self):
        d = {"a": 1}
        assert wrap_value(d) is d

    def test_wrap_none_passthrough(self):
        assert wrap_value(None) is None

    def test_wrap_preserves_bool_value(self):
        assert wrap_value(True).value is True

    def test_wrap_preserves_int_value(self):
        assert wrap_value(99).value == 99

    def test_wrap_preserves_float_value(self):
        assert wrap_value(1.23).value == pytest.approx(1.23)

    def test_wrap_preserves_string_value(self):
        assert wrap_value("xyz").value == "xyz"


# ===========================================================================
# unwrap_value
# ===========================================================================


class TestUnwrapValueExtended:
    def test_unwrap_boolean_wrapper_true(self):
        assert unwrap_value(BooleanWrapper(True)) is True

    def test_unwrap_boolean_wrapper_false(self):
        assert unwrap_value(BooleanWrapper(False)) is False

    def test_unwrap_int_wrapper(self):
        assert unwrap_value(IntWrapper(7)) == 7

    def test_unwrap_float_wrapper(self):
        assert unwrap_value(FloatWrapper(2.5)) == pytest.approx(2.5)

    def test_unwrap_string_wrapper(self):
        assert unwrap_value(StringWrapper("hi")) == "hi"

    def test_unwrap_plain_bool_passthrough(self):
        assert unwrap_value(True) is True

    def test_unwrap_plain_int_passthrough(self):
        assert unwrap_value(42) == 42

    def test_unwrap_plain_float_passthrough(self):
        assert unwrap_value(3.14) == pytest.approx(3.14)

    def test_unwrap_plain_str_passthrough(self):
        assert unwrap_value("x") == "x"

    def test_unwrap_list_passthrough(self):
        lst = [1, 2]
        assert unwrap_value(lst) is lst

    def test_unwrap_none_passthrough(self):
        assert unwrap_value(None) is None


# ===========================================================================
# unwrap_parameters
# ===========================================================================


class TestUnwrapParametersExtended:
    def test_unwraps_all_four_types(self):
        result = unwrap_parameters(
            {
                "flag": BooleanWrapper(True),
                "count": IntWrapper(5),
                "scale": FloatWrapper(2.0),
                "name": StringWrapper("cube"),
            }
        )
        assert result["flag"] is True
        assert result["count"] == 5
        assert result["scale"] == pytest.approx(2.0)
        assert result["name"] == "cube"

    def test_plain_values_pass_through(self):
        result = unwrap_parameters({"x": 10, "y": "hello"})
        assert result["x"] == 10
        assert result["y"] == "hello"

    def test_empty_dict(self):
        assert unwrap_parameters({}) == {}

    def test_keys_preserved(self):
        result = unwrap_parameters({"foo": IntWrapper(1), "bar": IntWrapper(2)})
        assert set(result.keys()) == {"foo", "bar"}

    def test_none_passthrough(self):
        result = unwrap_parameters({"val": None})
        assert result["val"] is None


# ===========================================================================
# McpHttpConfig
# ===========================================================================


class TestMcpHttpConfig:
    def test_port_stored(self):
        assert McpHttpConfig(port=8765).port == 8765

    def test_port_custom(self):
        assert McpHttpConfig(port=9000).port == 9000

    def test_server_name_default_not_none(self):
        cfg = McpHttpConfig(port=8765)
        assert cfg.server_name is not None
        assert isinstance(cfg.server_name, str)

    def test_server_version_default_not_none(self):
        cfg = McpHttpConfig(port=8765)
        assert cfg.server_version is not None
        assert isinstance(cfg.server_version, str)

    def test_server_name_custom(self):
        cfg = McpHttpConfig(port=8765, server_name="my-dcc-server")
        assert cfg.server_name == "my-dcc-server"

    def test_server_version_custom(self):
        cfg = McpHttpConfig(port=8765, server_version="3.0.0")
        assert cfg.server_version == "3.0.0"

    def test_server_name_and_version_both_custom(self):
        cfg = McpHttpConfig(port=7890, server_name="blender-mcp", server_version="0.5.0")
        assert cfg.server_name == "blender-mcp"
        assert cfg.server_version == "0.5.0"

    def test_different_ports_independent(self):
        a = McpHttpConfig(port=1111)
        b = McpHttpConfig(port=2222)
        assert a.port != b.port


# ===========================================================================
# McpHttpServer — register_handler / has_handler / catalog API
# ===========================================================================


class TestMcpHttpServerHandlers:
    def _make_server(self):
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=8765)
        return McpHttpServer(reg, cfg)

    def test_has_handler_false_before_register(self):
        server = self._make_server()
        assert server.has_handler("create_sphere") is False

    def test_has_handler_true_after_register(self):
        server = self._make_server()
        server.register_handler("create_sphere", lambda p: {})
        assert server.has_handler("create_sphere") is True

    def test_has_handler_false_for_other_name(self):
        server = self._make_server()
        server.register_handler("create_sphere", lambda p: {})
        assert server.has_handler("delete_mesh") is False

    def test_register_multiple_handlers(self):
        server = self._make_server()
        server.register_handler("action_a", lambda p: {})
        server.register_handler("action_b", lambda p: {})
        assert server.has_handler("action_a") is True
        assert server.has_handler("action_b") is True

    def test_nonexistent_handler_false(self):
        server = self._make_server()
        assert server.has_handler("nonexistent_xyz") is False

    def test_has_handler_empty_name_false(self):
        server = self._make_server()
        assert server.has_handler("") is False


class TestMcpHttpServerCatalogApi:
    def _make_server(self):
        reg = ActionRegistry()
        cfg = McpHttpConfig(port=8765)
        return McpHttpServer(reg, cfg)

    def test_list_skills_empty_initially(self):
        server = self._make_server()
        assert server.list_skills() == []

    def test_loaded_count_zero_initially(self):
        server = self._make_server()
        assert server.loaded_count() == 0

    def test_find_skills_empty_query_returns_list(self):
        server = self._make_server()
        result = server.find_skills(query="geometry")
        assert isinstance(result, list)

    def test_is_loaded_false_for_nonexistent(self):
        server = self._make_server()
        assert server.is_loaded("nonexistent_skill") is False

    def test_server_name_accessible_via_config(self):
        cfg = McpHttpConfig(port=8765, server_name="test-server")
        assert cfg.server_name == "test-server"


# ===========================================================================
# scan_and_load — happy path with examples/skills
# ===========================================================================


class TestScanAndLoad:
    def test_returns_tuple_of_two(self):
        result = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_skills_is_list(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert isinstance(skills, list)

    def test_skipped_is_list(self):
        _, skipped = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert isinstance(skipped, list)

    def test_finds_expected_skill_count(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        # examples/skills has 9 skills
        assert len(skills) >= 9

    def test_each_skill_has_name(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        for s in skills:
            assert isinstance(s.name, str)
            assert len(s.name) > 0

    def test_each_skill_has_version(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        for s in skills:
            assert isinstance(s.version, str)

    def test_each_skill_has_skill_path(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        for s in skills:
            assert isinstance(s.skill_path, str)
            assert len(s.skill_path) > 0

    def test_maya_geometry_present(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        names = [s.name for s in skills]
        assert "maya-geometry" in names

    def test_hello_world_present(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        names = [s.name for s in skills]
        assert "hello-world" in names

    def test_no_skipped_for_valid_dir(self):
        _, skipped = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert skipped == []

    def test_empty_paths_returns_empty(self):
        env_backup = os.environ.pop("DCC_MCP_SKILL_PATHS", None)
        try:
            skills, skipped = scan_and_load(extra_paths=[])
            assert skills == []
            assert skipped == []
        finally:
            if env_backup is not None:
                os.environ["DCC_MCP_SKILL_PATHS"] = env_backup

    def test_nonexistent_path_returns_empty(self):
        skills, _ = scan_and_load(extra_paths=["/totally/nonexistent/path/xyz"])
        assert skills == []


class TestScanAndLoadLenient:
    def test_returns_tuple(self):
        result = scan_and_load_lenient(extra_paths=[EXAMPLES_SKILLS_DIR])
        assert isinstance(result, tuple) and len(result) == 2

    def test_skills_count_matches_strict(self):
        skills_strict, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        skills_lenient, _ = scan_and_load_lenient(extra_paths=[EXAMPLES_SKILLS_DIR])
        # Both should agree on valid skills directory
        assert len(skills_lenient) == len(skills_strict)

    def test_lenient_empty_path(self):
        env_backup = os.environ.pop("DCC_MCP_SKILL_PATHS", None)
        try:
            skills, _skipped = scan_and_load_lenient(extra_paths=[])
            assert skills == []
        finally:
            if env_backup is not None:
                os.environ["DCC_MCP_SKILL_PATHS"] = env_backup

    def test_lenient_nonexistent_path(self):
        skills, _ = scan_and_load_lenient(extra_paths=["/nonexistent/path/abc"])
        assert skills == []


class TestScanAndLoadSkillProperties:
    """Verify properties on individual SkillMetadata objects."""

    def _skills(self):
        skills, _ = scan_and_load(extra_paths=[EXAMPLES_SKILLS_DIR])
        return {s.name: s for s in skills}

    def test_maya_geometry_has_scripts(self):
        skills = self._skills()
        assert len(skills["maya-geometry"].scripts) >= 1

    def test_maya_geometry_description_nonempty(self):
        skills = self._skills()
        assert len(skills["maya-geometry"].description) > 0

    def test_hello_world_scripts_list(self):
        skills = self._skills()
        assert isinstance(skills["hello-world"].scripts, list)

    def test_skill_path_is_absolute(self):
        skills = self._skills()
        for s in skills.values():
            # skill_path must be an absolute path
            assert Path(s.skill_path).is_absolute(), f"{s.name} skill_path not absolute: {s.skill_path}"

    def test_scripts_have_extension(self):
        skills = self._skills()
        valid_exts = {".py", ".mel", ".ms", ".bat", ".cmd", ".sh", ".bash", ".ps1", ".jsx", ".js"}
        for s in skills.values():
            for script in s.scripts:
                ext = Path(script).suffix.lower()
                assert ext in valid_exts, f"Unexpected extension {ext} for {script}"

    def test_tools_is_list(self):
        skills = self._skills()
        for s in skills.values():
            assert isinstance(s.tools, list)
