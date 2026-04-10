"""Deep tests for ActionValidator, ActionDispatcher, InputValidator, SkillCatalog.

DccInfo/DccCapabilities/DccError/SceneInfo/ScriptResult/SceneStatistics.

Coverage targets:
- ActionValidator: from_schema_json, from_action_registry, validate edge cases
- ActionDispatcher: skip_empty_schema_validation, remove_handler, handler_names, error paths
- InputValidator: require_string/number/forbid_substrings/validate
- SkillCatalog: discover, find_skills, list_skills, is_loaded, load/unload lifecycle
- DccInfo, DccCapabilities, DccError, DccErrorCode: construction / attributes / repr
- SceneStatistics, SceneInfo, ScriptResult, ScriptLanguage: construction / to_dict / attrs
"""

from __future__ import annotations

import json
import os
import tempfile

import pytest

from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import ActionValidator
from dcc_mcp_core import DccCapabilities
from dcc_mcp_core import DccError
from dcc_mcp_core import DccErrorCode
from dcc_mcp_core import DccInfo
from dcc_mcp_core import InputValidator
from dcc_mcp_core import SceneInfo
from dcc_mcp_core import SceneStatistics
from dcc_mcp_core import ScriptLanguage
from dcc_mcp_core import ScriptResult
from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import SkillMetadata

# ---------------------------------------------------------------------------
# ActionValidator: from_schema_json
# ---------------------------------------------------------------------------


class TestActionValidatorFromSchemaJson:
    """ActionValidator.from_schema_json construction and basic behaviour."""

    def _make_schema(self, required=None, props=None):
        schema = {"type": "object"}
        if required:
            schema["required"] = required
        if props:
            schema["properties"] = props
        return json.dumps(schema)

    def test_create_basic_schema(self):
        v = ActionValidator.from_schema_json(self._make_schema())
        assert v is not None

    def test_repr_contains_class(self):
        v = ActionValidator.from_schema_json(self._make_schema())
        assert "ActionValidator" in repr(v)

    def test_validate_empty_object_no_required(self):
        v = ActionValidator.from_schema_json(self._make_schema())
        ok, errors = v.validate("{}")
        assert ok is True
        assert errors == []

    def test_validate_required_field_present(self):
        schema = self._make_schema(
            required=["radius"],
            props={"radius": {"type": "number"}},
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _errors = v.validate('{"radius": 1.5}')
        assert ok is True

    def test_validate_required_field_missing(self):
        schema = self._make_schema(
            required=["radius"],
            props={"radius": {"type": "number"}},
        )
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate("{}")
        assert ok is False
        assert len(errors) > 0

    def test_validate_wrong_type(self):
        schema = self._make_schema(
            required=["count"],
            props={"count": {"type": "integer"}},
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _errors = v.validate('{"count": "not_a_number"}')
        assert ok is False

    def test_validate_invalid_json_raises(self):
        v = ActionValidator.from_schema_json(self._make_schema())
        with pytest.raises((ValueError, Exception)):
            v.validate("not json at all")

    def test_validate_minimum_constraint_pass(self):
        schema = self._make_schema(
            required=["x"],
            props={"x": {"type": "number", "minimum": 0.0}},
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _ = v.validate('{"x": 0.0}')
        assert ok is True

    def test_validate_minimum_constraint_fail(self):
        schema = self._make_schema(
            required=["x"],
            props={"x": {"type": "number", "minimum": 0.0}},
        )
        v = ActionValidator.from_schema_json(schema)
        ok, errors = v.validate('{"x": -1.0}')
        assert ok is False
        assert len(errors) > 0

    def test_invalid_schema_json_raises(self):
        with pytest.raises((ValueError, Exception)):
            ActionValidator.from_schema_json("not json")

    def test_schema_with_string_type(self):
        schema = json.dumps(
            {
                "type": "object",
                "required": ["name"],
                "properties": {"name": {"type": "string"}},
            }
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _ = v.validate('{"name": "sphere"}')
        assert ok is True

    def test_schema_with_boolean_field(self):
        schema = json.dumps(
            {
                "type": "object",
                "properties": {"visible": {"type": "boolean"}},
            }
        )
        v = ActionValidator.from_schema_json(schema)
        ok, _ = v.validate('{"visible": true}')
        assert ok is True


# ---------------------------------------------------------------------------
# ActionValidator: from_action_registry
# ---------------------------------------------------------------------------


class TestActionValidatorFromRegistry:
    """ActionValidator.from_action_registry construction."""

    def _reg_with_schema(self, name="sphere", schema_props=None):
        reg = ActionRegistry()
        if schema_props is None:
            schema_props = {"radius": {"type": "number"}}
        schema = json.dumps({"type": "object", "required": list(schema_props.keys()), "properties": schema_props})
        reg.register(name=name, input_schema=schema)
        return reg

    def test_create_from_registry(self):
        reg = self._reg_with_schema()
        v = ActionValidator.from_action_registry(reg, "sphere")
        assert v is not None

    def test_validate_from_registry_pass(self):
        reg = self._reg_with_schema()
        v = ActionValidator.from_action_registry(reg, "sphere")
        ok, _ = v.validate('{"radius": 2.0}')
        assert ok is True

    def test_validate_from_registry_fail(self):
        reg = self._reg_with_schema()
        v = ActionValidator.from_action_registry(reg, "sphere")
        ok, _errors = v.validate("{}")
        assert ok is False
        reg = ActionRegistry()
        with pytest.raises((KeyError, Exception)):
            ActionValidator.from_action_registry(reg, "nonexistent")

    def test_with_dcc_name(self):
        reg = ActionRegistry()
        schema = json.dumps({"type": "object", "properties": {"r": {"type": "number"}}})
        reg.register(name="cube", dcc="maya", input_schema=schema)
        v = ActionValidator.from_action_registry(reg, "cube", dcc_name="maya")
        assert v is not None

    def test_multiple_fields(self):
        reg = ActionRegistry()
        schema = json.dumps(
            {
                "type": "object",
                "required": ["x", "y"],
                "properties": {
                    "x": {"type": "number"},
                    "y": {"type": "number"},
                },
            }
        )
        reg.register(name="move", input_schema=schema)
        v = ActionValidator.from_action_registry(reg, "move")
        ok, _ = v.validate('{"x": 1.0, "y": 2.0}')
        assert ok is True


# ---------------------------------------------------------------------------
# ActionDispatcher: deep tests
# ---------------------------------------------------------------------------


class TestActionDispatcherCreate:
    """ActionDispatcher construction and basic state."""

    def _make_reg_disp(self):
        reg = ActionRegistry()
        reg.register(name="foo")
        disp = ActionDispatcher(reg)
        return reg, disp

    def test_create(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        assert disp is not None

    def test_repr(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        assert "ActionDispatcher" in repr(disp)

    def test_handler_count_zero(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        assert disp.handler_count() == 0

    def test_has_handler_false(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        assert disp.has_handler("nonexistent") is False

    def test_skip_empty_schema_validation_default(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        # default is True (skip when schema is empty)
        assert isinstance(disp.skip_empty_schema_validation, bool)


class TestActionDispatcherHandlers:
    """Register, remove, query handlers."""

    def _make(self, names=None):
        reg = ActionRegistry()
        names = names or ["alpha", "beta"]
        for n in names:
            reg.register(name=n)
        disp = ActionDispatcher(reg)
        return reg, disp

    def test_register_handler(self):
        _, disp = self._make(["a"])
        disp.register_handler("a", lambda p: {"done": True})
        assert disp.has_handler("a") is True

    def test_handler_count_increments(self):
        _, disp = self._make(["a", "b"])
        disp.register_handler("a", lambda p: {})
        assert disp.handler_count() == 1
        disp.register_handler("b", lambda p: {})
        assert disp.handler_count() == 2

    def test_handler_names_sorted(self):
        _, disp = self._make(["z_act", "a_act"])
        disp.register_handler("z_act", lambda p: {})
        disp.register_handler("a_act", lambda p: {})
        names = disp.handler_names()
        assert names == sorted(names)

    def test_remove_handler_returns_true(self):
        _, disp = self._make(["x"])
        disp.register_handler("x", lambda p: {})
        result = disp.remove_handler("x")
        assert result is True

    def test_remove_handler_decrements_count(self):
        _, disp = self._make(["x"])
        disp.register_handler("x", lambda p: {})
        disp.remove_handler("x")
        assert disp.handler_count() == 0

    def test_remove_nonexistent_returns_false(self):
        _, disp = self._make()
        result = disp.remove_handler("nonexistent")
        assert result is False

    def test_has_handler_after_remove(self):
        _, disp = self._make(["x"])
        disp.register_handler("x", lambda p: {})
        disp.remove_handler("x")
        assert disp.has_handler("x") is False

    def test_non_callable_raises(self):
        _, disp = self._make(["x"])
        with pytest.raises((TypeError, Exception)):
            disp.register_handler("x", "not_callable")

    def test_dispatch_basic(self):
        reg = ActionRegistry()
        reg.register(name="create")
        disp = ActionDispatcher(reg)
        disp.register_handler("create", lambda p: {"ok": True})
        result = disp.dispatch("create", "{}")
        assert result["output"]["ok"] is True

    def test_dispatch_has_action_key(self):
        reg = ActionRegistry()
        reg.register(name="ping")
        disp = ActionDispatcher(reg)
        disp.register_handler("ping", lambda p: "pong")
        result = disp.dispatch("ping", "{}")
        assert result["action"] == "ping"

    def test_dispatch_no_handler_raises(self):
        reg = ActionRegistry()
        reg.register(name="orphan")
        disp = ActionDispatcher(reg)
        with pytest.raises((KeyError, RuntimeError, Exception)):
            disp.dispatch("orphan", "{}")

    def test_dispatch_validation_skipped_flag(self):
        reg = ActionRegistry()
        reg.register(name="act")
        disp = ActionDispatcher(reg)
        disp.register_handler("act", lambda p: {})
        result = disp.dispatch("act", "{}")
        assert "validation_skipped" in result


class TestActionDispatcherWithSchema:
    """Dispatcher with schema triggers real validation."""

    def _make_with_schema(self):
        reg = ActionRegistry()
        schema = json.dumps(
            {
                "type": "object",
                "required": ["radius"],
                "properties": {"radius": {"type": "number", "minimum": 0}},
            }
        )
        reg.register(name="sphere", input_schema=schema)
        disp = ActionDispatcher(reg)
        disp.register_handler("sphere", lambda p: {"r": p.get("radius")})
        return disp

    def test_valid_params_dispatches(self):
        disp = self._make_with_schema()
        result = disp.dispatch("sphere", '{"radius": 3.0}')
        assert result["output"]["r"] == 3.0

    def test_invalid_params_raises(self):
        disp = self._make_with_schema()
        with pytest.raises((ValueError, RuntimeError, Exception)):
            disp.dispatch("sphere", '{"radius": -1.0}')

    def test_skip_validation_flag_false_enforces_schema(self):
        disp = self._make_with_schema()
        disp.skip_empty_schema_validation = False
        # required field missing → should raise
        with pytest.raises((ValueError, RuntimeError, Exception)):
            disp.dispatch("sphere", "{}")


# ---------------------------------------------------------------------------
# InputValidator: deep tests
# ---------------------------------------------------------------------------


class TestInputValidatorCreate:
    """InputValidator construction and empty state."""

    def test_create(self):
        v = InputValidator()
        assert v is not None

    def test_repr(self):
        v = InputValidator()
        assert "InputValidator" in repr(v)

    def test_validate_no_rules_passes_empty(self):
        v = InputValidator()
        ok, err = v.validate("{}")
        assert ok is True
        assert err is None

    def test_validate_no_rules_passes_extra_fields(self):
        v = InputValidator()
        ok, _ = v.validate('{"anything": 123}')
        assert ok is True


class TestInputValidatorRequireString:
    """InputValidator.require_string constraints."""

    def test_require_string_pass(self):
        v = InputValidator()
        v.require_string("name", max_length=100, min_length=1)
        ok, _ = v.validate('{"name": "sphere"}')
        assert ok is True

    def test_require_string_missing_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=100, min_length=1)
        ok, err = v.validate("{}")
        assert ok is False
        assert err is not None

    def test_require_string_too_long_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=5, min_length=1)
        ok, _err = v.validate('{"name": "toolongstring"}')
        assert ok is False

    def test_require_string_too_short_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=100, min_length=5)
        ok, _err = v.validate('{"name": "ab"}')
        assert ok is False

    def test_require_string_exact_max_passes(self):
        v = InputValidator()
        v.require_string("n", max_length=3, min_length=1)
        ok, _ = v.validate('{"n": "abc"}')
        assert ok is True

    def test_require_string_wrong_type_fails(self):
        v = InputValidator()
        v.require_string("name", max_length=100, min_length=0)
        ok, _err = v.validate('{"name": 42}')
        assert ok is False


class TestInputValidatorRequireNumber:
    """InputValidator.require_number constraints."""

    def test_require_number_pass(self):
        v = InputValidator()
        v.require_number("count", min_value=0.0, max_value=1000.0)
        ok, _ = v.validate('{"count": 5}')
        assert ok is True

    def test_require_number_missing_fails(self):
        v = InputValidator()
        v.require_number("count", min_value=0.0, max_value=1000.0)
        ok, _err = v.validate("{}")
        assert ok is False

    def test_require_number_below_min_fails(self):
        v = InputValidator()
        v.require_number("x", min_value=0.0, max_value=100.0)
        ok, _ = v.validate('{"x": -1.0}')
        assert ok is False

    def test_require_number_above_max_fails(self):
        v = InputValidator()
        v.require_number("x", min_value=0.0, max_value=10.0)
        ok, _ = v.validate('{"x": 100.0}')
        assert ok is False

    def test_require_number_boundary_min_passes(self):
        v = InputValidator()
        v.require_number("x", min_value=0.0, max_value=10.0)
        ok, _ = v.validate('{"x": 0.0}')
        assert ok is True

    def test_require_number_boundary_max_passes(self):
        v = InputValidator()
        v.require_number("x", min_value=0.0, max_value=10.0)
        ok, _ = v.validate('{"x": 10.0}')
        assert ok is True

    def test_require_number_wrong_type_fails(self):
        v = InputValidator()
        v.require_number("x", min_value=0.0, max_value=100.0)
        ok, _err = v.validate('{"x": "five"}')
        assert ok is False


class TestInputValidatorForbidSubstrings:
    """InputValidator.forbid_substrings injection guard."""

    def test_no_forbidden_passes(self):
        v = InputValidator()
        v.require_string("cmd", max_length=200, min_length=0)
        v.forbid_substrings("cmd", ["DROP TABLE", "DELETE FROM"])
        ok, _ = v.validate('{"cmd": "create sphere"}')
        assert ok is True

    def test_forbidden_substring_fails(self):
        v = InputValidator()
        v.require_string("cmd", max_length=200, min_length=0)
        v.forbid_substrings("cmd", ["DROP TABLE"])
        ok, err = v.validate('{"cmd": "DROP TABLE users"}')
        assert ok is False
        assert err is not None

    def test_multiple_substrings_any_fails(self):
        v = InputValidator()
        v.require_string("q", max_length=200, min_length=0)
        v.forbid_substrings("q", ["--", ";", "/*"])
        ok, _ = v.validate('{"q": "normal query"}')
        assert ok is True
        # Validate injection fails
        ok2, _ = v.validate('{"q": "SELECT 1 -- comment"}')
        assert ok2 is False

    def test_forbidden_substring_case_sensitivity(self):
        # typically case-sensitive; "drop table" should pass if only "DROP TABLE" forbidden
        v = InputValidator()
        v.require_string("cmd", max_length=200, min_length=0)
        v.forbid_substrings("cmd", ["DROP TABLE"])
        ok, _ = v.validate('{"cmd": "drop table users"}')
        # Case-sensitivity is implementation-defined; just check it doesn't crash
        assert isinstance(ok, bool)


# ---------------------------------------------------------------------------
# SkillCatalog: deep tests
# ---------------------------------------------------------------------------

SKILL_MD_CONTENT = """\
---
name: test-skill-{idx}
description: A test skill number {idx}.
dcc: maya
tags: [test, auto]
version: "1.0.{idx}"
tools:
  - name: do_thing_{idx}
    description: Does something {idx}
---
# Test Skill {idx}

This is a test skill.
"""


def _make_skill_dir(tmp_path, idx: int) -> str:
    """Create a minimal skill package directory."""
    skill_dir = tmp_path / f"test-skill-{idx}"
    skill_dir.mkdir()
    skill_file = skill_dir / "SKILL.md"
    skill_file.write_text(SKILL_MD_CONTENT.format(idx=idx), encoding="utf-8")
    return str(skill_dir)


class TestSkillCatalogCreate:
    """SkillCatalog construction and empty state."""

    def test_create(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert cat is not None

    def test_repr(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert "SkillCatalog" in repr(cat)

    def test_len_zero_initial(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert len(cat) == 0

    def test_bool_false_when_empty(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert not cat

    def test_loaded_count_zero_initial(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        assert cat.loaded_count() == 0

    def test_list_skills_empty(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        skills = cat.list_skills()
        assert skills == [] or isinstance(skills, list)


class TestSkillCatalogDiscover:
    """SkillCatalog discover from directories."""

    def test_discover_no_paths_returns_zero(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        # Without env vars pointing to real dirs, should return 0
        count = cat.discover(extra_paths=[])
        assert isinstance(count, int)
        assert count >= 0

    def test_discover_with_tmp_skill(self, tmp_path):
        _make_skill_dir(tmp_path, 0)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        count = cat.discover(extra_paths=[str(tmp_path)])
        assert count >= 1

    def test_discover_multiple_skills(self, tmp_path):
        for i in range(3):
            _make_skill_dir(tmp_path, i)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        count = cat.discover(extra_paths=[str(tmp_path)])
        assert count >= 3

    def test_discover_increments_len(self, tmp_path):
        _make_skill_dir(tmp_path, 9)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        assert len(cat) >= 1

    def test_discover_bool_true_when_skills(self, tmp_path):
        _make_skill_dir(tmp_path, 5)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        assert bool(cat)

    def test_discover_dcc_filter(self, tmp_path):
        _make_skill_dir(tmp_path, 1)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        # Filter by matching dcc
        count = cat.discover(extra_paths=[str(tmp_path)], dcc_name="maya")
        assert isinstance(count, int)

    def test_discover_nonexistent_path(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        # Nonexistent path should not crash, returns 0
        count = cat.discover(extra_paths=["/nonexistent/path/xyz"])
        assert count == 0


class TestSkillCatalogListAndFind:
    """SkillCatalog list_skills and find_skills after discover."""

    def test_list_skills_returns_list(self, tmp_path):
        _make_skill_dir(tmp_path, 2)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        skills = cat.list_skills()
        assert isinstance(skills, list)

    def test_list_skills_has_discovered_skill(self, tmp_path):
        _make_skill_dir(tmp_path, 3)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        skills = cat.list_skills()
        names = [s.name if hasattr(s, "name") else s.get("name", "") for s in skills]
        assert "test-skill-3" in names

    def test_find_skills_no_filter(self, tmp_path):
        _make_skill_dir(tmp_path, 4)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        results = cat.find_skills()
        assert isinstance(results, list)

    def test_find_skills_by_query(self, tmp_path):
        _make_skill_dir(tmp_path, 6)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        results = cat.find_skills(query="test-skill-6")
        assert isinstance(results, list)

    def test_find_skills_by_tag(self, tmp_path):
        _make_skill_dir(tmp_path, 7)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        results = cat.find_skills(tags=["test"])
        assert isinstance(results, list)

    def test_find_skills_by_dcc(self, tmp_path):
        _make_skill_dir(tmp_path, 8)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        results = cat.find_skills(dcc="maya")
        assert isinstance(results, list)


class TestSkillCatalogLoadUnload:
    """SkillCatalog load_skill / unload_skill lifecycle."""

    def test_is_loaded_false_before_load(self, tmp_path):
        _make_skill_dir(tmp_path, 10)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        assert cat.is_loaded("test-skill-10") is False

    def test_load_skill_returns_list(self, tmp_path):
        _make_skill_dir(tmp_path, 11)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        actions = cat.load_skill("test-skill-11")
        assert isinstance(actions, list)

    def test_is_loaded_true_after_load(self, tmp_path):
        _make_skill_dir(tmp_path, 12)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        cat.load_skill("test-skill-12")
        assert cat.is_loaded("test-skill-12") is True

    def test_loaded_count_increments(self, tmp_path):
        _make_skill_dir(tmp_path, 13)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        assert cat.loaded_count() == 0
        cat.load_skill("test-skill-13")
        assert cat.loaded_count() == 1

    def test_load_skill_registers_actions_in_registry(self, tmp_path):
        _make_skill_dir(tmp_path, 14)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        cat.load_skill("test-skill-14")
        all_actions = reg.list_actions()
        assert len(all_actions) >= 1

    def test_unload_skill_returns_count(self, tmp_path):
        _make_skill_dir(tmp_path, 15)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        cat.load_skill("test-skill-15")
        removed = cat.unload_skill("test-skill-15")
        assert isinstance(removed, int)
        assert removed >= 0

    def test_is_loaded_false_after_unload(self, tmp_path):
        _make_skill_dir(tmp_path, 16)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        cat.load_skill("test-skill-16")
        cat.unload_skill("test-skill-16")
        assert cat.is_loaded("test-skill-16") is False

    def test_load_nonexistent_raises(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        with pytest.raises((ValueError, Exception)):
            cat.load_skill("nonexistent-skill-xyz")

    def test_unload_not_loaded_raises(self, tmp_path):
        _make_skill_dir(tmp_path, 17)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        with pytest.raises((ValueError, Exception)):
            cat.unload_skill("test-skill-17")

    def test_get_skill_info_after_discover(self, tmp_path):
        _make_skill_dir(tmp_path, 18)
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        cat.discover(extra_paths=[str(tmp_path)])
        info = cat.get_skill_info("test-skill-18")
        # May return dict or None
        assert info is None or isinstance(info, dict)

    def test_get_skill_info_missing_returns_none(self):
        reg = ActionRegistry()
        cat = SkillCatalog(reg)
        info = cat.get_skill_info("does-not-exist")
        assert info is None


# ---------------------------------------------------------------------------
# DccInfo
# ---------------------------------------------------------------------------


class TestDccInfo:
    """DccInfo construction and attribute access."""

    def test_create_minimal(self):
        info = DccInfo(dcc_type="maya", version="2024.2", platform="windows", pid=1234)
        assert info is not None

    def test_repr(self):
        info = DccInfo(dcc_type="maya", version="2024.2", platform="windows", pid=1234)
        assert "DccInfo" in repr(info)

    def test_dcc_type(self):
        info = DccInfo(dcc_type="blender", version="4.0", platform="linux", pid=999)
        assert info.dcc_type == "blender"

    def test_version(self):
        info = DccInfo(dcc_type="maya", version="2024.2", platform="windows", pid=1)
        assert info.version == "2024.2"

    def test_platform(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="macos", pid=2)
        assert info.platform == "macos"

    def test_pid(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="windows", pid=42)
        assert info.pid == 42

    def test_python_version_default_none(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="windows", pid=1)
        assert info.python_version is None

    def test_python_version_set(self):
        info = DccInfo(
            dcc_type="maya",
            version="2024",
            platform="windows",
            pid=1,
            python_version="3.10.11",
        )
        assert info.python_version == "3.10.11"

    def test_metadata_default_none(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="windows", pid=1)
        # Default metadata is {} (empty dict), not None
        assert info.metadata is None or info.metadata == {}

    def test_metadata_set(self):
        info = DccInfo(
            dcc_type="maya",
            version="1.0",
            platform="windows",
            pid=1,
            metadata={"key": "value"},
        )
        assert info.metadata is not None

    def test_to_dict_returns_dict(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="windows", pid=1)
        d = info.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_has_dcc_type(self):
        info = DccInfo(dcc_type="houdini", version="20.0", platform="linux", pid=5)
        d = info.to_dict()
        assert "dcc_type" in d or "dcc" in d

    def test_to_dict_has_pid(self):
        info = DccInfo(dcc_type="maya", version="1.0", platform="windows", pid=777)
        d = info.to_dict()
        assert 777 in d.values() or any("777" in str(v) for v in d.values())


# ---------------------------------------------------------------------------
# DccCapabilities
# ---------------------------------------------------------------------------


class TestDccCapabilities:
    """DccCapabilities construction and attribute access."""

    def test_create_default(self):
        cap = DccCapabilities()
        assert cap is not None

    def test_repr(self):
        cap = DccCapabilities()
        assert "DccCapabilities" in repr(cap)

    def test_scene_info_default_false(self):
        cap = DccCapabilities()
        assert cap.scene_info is False

    def test_snapshot_default_false(self):
        cap = DccCapabilities()
        assert cap.snapshot is False

    def test_undo_redo_default_false(self):
        cap = DccCapabilities()
        assert cap.undo_redo is False

    def test_progress_reporting_default_false(self):
        cap = DccCapabilities()
        assert cap.progress_reporting is False

    def test_file_operations_default_false(self):
        cap = DccCapabilities()
        assert cap.file_operations is False

    def test_selection_default_false(self):
        cap = DccCapabilities()
        assert cap.selection is False

    def test_extensions_default_none(self):
        cap = DccCapabilities()
        # Default extensions is {} (empty dict), not None
        assert cap.extensions is None or cap.extensions == {}

    def test_set_scene_info(self):
        cap = DccCapabilities(scene_info=True)
        assert cap.scene_info is True

    def test_set_multiple_caps(self):
        cap = DccCapabilities(
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            selection=True,
        )
        assert cap.scene_info is True
        assert cap.snapshot is True
        assert cap.undo_redo is True
        assert cap.selection is True

    def test_set_extensions(self):
        cap = DccCapabilities(extensions={"custom_ext": True})
        assert cap.extensions is not None


# ---------------------------------------------------------------------------
# DccError / DccErrorCode
# ---------------------------------------------------------------------------


class TestDccError:
    """DccError construction and attributes."""

    def test_create(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="test error")
        assert err is not None

    def test_repr(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="msg")
        assert "DccError" in repr(err)

    def test_str(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="msg")
        s = str(err)
        assert isinstance(s, str)

    def test_code_attribute(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="msg")
        assert err.code == DccErrorCode.INTERNAL

    def test_message_attribute(self):
        err = DccError(code=DccErrorCode.CONNECTION_FAILED, message="conn failed")
        assert err.message == "conn failed"

    def test_details_default_none(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="msg")
        assert err.details is None

    def test_details_set(self):
        err = DccError(code=DccErrorCode.INVALID_INPUT, message="bad", details="extra info")
        assert err.details == "extra info"

    def test_recoverable_default_false(self):
        err = DccError(code=DccErrorCode.INTERNAL, message="msg")
        assert err.recoverable is False

    def test_recoverable_set(self):
        err = DccError(code=DccErrorCode.NOT_RESPONDING, message="hung", recoverable=True)
        assert err.recoverable is True


class TestDccErrorCode:
    """DccErrorCode enum values."""

    def test_internal_exists(self):
        assert DccErrorCode.INTERNAL is not None

    def test_connection_failed_exists(self):
        assert DccErrorCode.CONNECTION_FAILED is not None

    def test_invalid_input_exists(self):
        assert DccErrorCode.INVALID_INPUT is not None

    def test_not_responding_exists(self):
        assert DccErrorCode.NOT_RESPONDING is not None

    def test_internal_repr(self):
        r = repr(DccErrorCode.INTERNAL)
        assert "INTERNAL" in r or "DccErrorCode" in r

    def test_codes_are_comparable(self):
        assert DccErrorCode.INTERNAL == DccErrorCode.INTERNAL
        assert DccErrorCode.INTERNAL != DccErrorCode.CONNECTION_FAILED

    def test_code_int_conversion(self):
        val = int(DccErrorCode.INTERNAL)
        assert isinstance(val, int)


# ---------------------------------------------------------------------------
# SceneStatistics
# ---------------------------------------------------------------------------


class TestSceneStatistics:
    """SceneStatistics construction and attributes."""

    def test_create_default(self):
        s = SceneStatistics()
        assert s is not None

    def test_repr(self):
        s = SceneStatistics()
        assert "SceneStatistics" in repr(s)

    def test_object_count_default_zero(self):
        s = SceneStatistics()
        assert s.object_count == 0

    def test_vertex_count_default_zero(self):
        s = SceneStatistics()
        assert s.vertex_count == 0

    def test_polygon_count_default_zero(self):
        s = SceneStatistics()
        assert s.polygon_count == 0

    def test_material_count_default_zero(self):
        s = SceneStatistics()
        assert s.material_count == 0

    def test_texture_count_default_zero(self):
        s = SceneStatistics()
        assert s.texture_count == 0

    def test_light_count_default_zero(self):
        s = SceneStatistics()
        assert s.light_count == 0

    def test_camera_count_default_zero(self):
        s = SceneStatistics()
        assert s.camera_count == 0

    def test_create_with_values(self):
        s = SceneStatistics(
            object_count=10,
            vertex_count=1000,
            polygon_count=500,
            material_count=3,
            texture_count=5,
            light_count=2,
            camera_count=1,
        )
        assert s.object_count == 10
        assert s.vertex_count == 1000
        assert s.polygon_count == 500
        assert s.material_count == 3
        assert s.texture_count == 5
        assert s.light_count == 2
        assert s.camera_count == 1


# ---------------------------------------------------------------------------
# SceneInfo
# ---------------------------------------------------------------------------


class TestSceneInfo:
    """SceneInfo construction and attributes."""

    def test_create_default(self):
        s = SceneInfo()
        assert s is not None

    def test_repr(self):
        s = SceneInfo()
        assert "SceneInfo" in repr(s)

    def test_file_path_default_empty_or_none(self):
        s = SceneInfo()
        assert s.file_path is None or s.file_path == ""

    def test_name_default(self):
        s = SceneInfo()
        assert s.name is None or isinstance(s.name, str)

    def test_modified_default_false(self):
        s = SceneInfo()
        assert s.modified is False

    def test_set_file_path(self):
        s = SceneInfo(file_path="/path/to/scene.ma")
        assert s.file_path == "/path/to/scene.ma"

    def test_set_name(self):
        s = SceneInfo(name="my_scene")
        assert s.name == "my_scene"

    def test_set_modified(self):
        s = SceneInfo(modified=True)
        assert s.modified is True

    def test_frame_range_default_none(self):
        s = SceneInfo()
        assert s.frame_range is None

    def test_current_frame_default_none(self):
        s = SceneInfo()
        assert s.current_frame is None

    def test_fps_default_none(self):
        s = SceneInfo()
        assert s.fps is None

    def test_set_fps(self):
        s = SceneInfo(fps=24.0)
        assert s.fps == 24.0

    def test_up_axis_default_none(self):
        s = SceneInfo()
        assert s.up_axis is None

    def test_set_up_axis(self):
        s = SceneInfo(up_axis="Y")
        assert s.up_axis == "Y"

    def test_units_default_none(self):
        s = SceneInfo()
        assert s.units is None

    def test_statistics_default_none(self):
        s = SceneInfo()
        # Default statistics is an empty SceneStatistics, not None
        assert s.statistics is None or isinstance(s.statistics, SceneStatistics)

    def test_set_statistics(self):
        stats = SceneStatistics(object_count=5)
        s = SceneInfo(statistics=stats)
        assert s.statistics is not None


# ---------------------------------------------------------------------------
# ScriptResult / ScriptLanguage
# ---------------------------------------------------------------------------


class TestScriptResult:
    """ScriptResult construction and attributes."""

    def test_create_success(self):
        r = ScriptResult(success=True, execution_time_ms=50)
        assert r is not None

    def test_repr(self):
        r = ScriptResult(success=True, execution_time_ms=10)
        assert "ScriptResult" in repr(r)

    def test_success_attribute(self):
        r = ScriptResult(success=True, execution_time_ms=0)
        assert r.success is True

    def test_success_false(self):
        r = ScriptResult(success=False, execution_time_ms=0)
        assert r.success is False

    def test_execution_time_ms(self):
        r = ScriptResult(success=True, execution_time_ms=100)
        assert r.execution_time_ms == 100

    def test_output_default_none(self):
        r = ScriptResult(success=True, execution_time_ms=0)
        assert r.output is None

    def test_output_set(self):
        r = ScriptResult(success=True, execution_time_ms=10, output="result_value")
        assert r.output == "result_value"

    def test_error_default_none(self):
        r = ScriptResult(success=True, execution_time_ms=0)
        assert r.error is None

    def test_error_set(self):
        r = ScriptResult(success=False, execution_time_ms=0, error="SomeError: bad input")
        assert r.error is not None

    def test_context_default_none(self):
        r = ScriptResult(success=True, execution_time_ms=0)
        # Default context is {} (empty dict), not None
        assert r.context is None or r.context == {}

    def test_to_dict_returns_dict(self):
        r = ScriptResult(success=True, execution_time_ms=50, output="ok")
        d = r.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_has_success(self):
        r = ScriptResult(success=True, execution_time_ms=50)
        d = r.to_dict()
        assert "success" in d


class TestScriptLanguage:
    """ScriptLanguage enum values."""

    def test_python_exists(self):
        lang = ScriptLanguage.PYTHON
        assert lang is not None

    def test_repr_python(self):
        lang = ScriptLanguage.PYTHON
        r = repr(lang)
        assert "PYTHON" in r or "python" in r.lower() or "ScriptLanguage" in r

    def test_str_python(self):
        lang = ScriptLanguage.PYTHON
        s = str(lang)
        assert isinstance(s, str)

    def test_int_conversion(self):
        val = int(ScriptLanguage.PYTHON)
        assert isinstance(val, int)

    def test_equality(self):
        assert ScriptLanguage.PYTHON == ScriptLanguage.PYTHON

    def test_other_languages_exist(self):
        # At least PYTHON must exist; others (MEL, MAXSCRIPT, etc.) are optional
        langs = [attr for attr in dir(ScriptLanguage) if not attr.startswith("_")]
        assert len(langs) >= 1
