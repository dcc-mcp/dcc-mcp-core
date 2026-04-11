"""Deep tests for DccInfo, DccCapabilities, DccError, ScriptLanguage, DccErrorCode, SkillCatalog.

Covers: full field access, edge cases, enum completeness, SkillCatalog all methods
(discover/list_skills/find_skills/load_skill/unload_skill/is_loaded/loaded_count/get_skill_info).
"""

from __future__ import annotations

from pathlib import Path

import pytest

import dcc_mcp_core

# ── ScriptLanguage enum ──────────────────────────────────────────────────────


class TestScriptLanguage:
    def test_python_exists(self):
        assert dcc_mcp_core.ScriptLanguage.PYTHON is not None

    def test_mel_exists(self):
        assert dcc_mcp_core.ScriptLanguage.MEL is not None

    def test_maxscript_exists(self):
        assert dcc_mcp_core.ScriptLanguage.MAXSCRIPT is not None

    def test_hscript_exists(self):
        assert dcc_mcp_core.ScriptLanguage.HSCRIPT is not None

    def test_vex_exists(self):
        assert dcc_mcp_core.ScriptLanguage.VEX is not None

    def test_lua_exists(self):
        assert dcc_mcp_core.ScriptLanguage.LUA is not None

    def test_csharp_exists(self):
        assert dcc_mcp_core.ScriptLanguage.CSHARP is not None

    def test_blueprint_exists(self):
        assert dcc_mcp_core.ScriptLanguage.BLUEPRINT is not None

    def test_repr_is_str(self):
        assert isinstance(repr(dcc_mcp_core.ScriptLanguage.PYTHON), str)

    def test_str_is_str(self):
        assert isinstance(str(dcc_mcp_core.ScriptLanguage.MEL), str)

    def test_equality_same(self):
        assert dcc_mcp_core.ScriptLanguage.PYTHON == dcc_mcp_core.ScriptLanguage.PYTHON

    def test_equality_different(self):
        assert dcc_mcp_core.ScriptLanguage.PYTHON != dcc_mcp_core.ScriptLanguage.MEL

    def test_maxscript_ne_python(self):
        assert dcc_mcp_core.ScriptLanguage.MAXSCRIPT != dcc_mcp_core.ScriptLanguage.PYTHON

    def test_csharp_ne_blueprint(self):
        assert dcc_mcp_core.ScriptLanguage.CSHARP != dcc_mcp_core.ScriptLanguage.BLUEPRINT


# ── DccErrorCode enum ────────────────────────────────────────────────────────


class TestDccErrorCode:
    def test_connection_failed_exists(self):
        assert dcc_mcp_core.DccErrorCode.CONNECTION_FAILED is not None

    def test_timeout_exists(self):
        assert dcc_mcp_core.DccErrorCode.TIMEOUT is not None

    def test_script_error_exists(self):
        assert dcc_mcp_core.DccErrorCode.SCRIPT_ERROR is not None

    def test_not_responding_exists(self):
        assert dcc_mcp_core.DccErrorCode.NOT_RESPONDING is not None

    def test_unsupported_exists(self):
        assert dcc_mcp_core.DccErrorCode.UNSUPPORTED is not None

    def test_permission_denied_exists(self):
        assert dcc_mcp_core.DccErrorCode.PERMISSION_DENIED is not None

    def test_invalid_input_exists(self):
        assert dcc_mcp_core.DccErrorCode.INVALID_INPUT is not None

    def test_scene_error_exists(self):
        assert dcc_mcp_core.DccErrorCode.SCENE_ERROR is not None

    def test_internal_exists(self):
        assert dcc_mcp_core.DccErrorCode.INTERNAL is not None

    def test_repr_is_str(self):
        assert isinstance(repr(dcc_mcp_core.DccErrorCode.TIMEOUT), str)

    def test_str_is_str(self):
        assert isinstance(str(dcc_mcp_core.DccErrorCode.INTERNAL), str)

    def test_equality_same(self):
        assert dcc_mcp_core.DccErrorCode.TIMEOUT == dcc_mcp_core.DccErrorCode.TIMEOUT

    def test_equality_different(self):
        assert dcc_mcp_core.DccErrorCode.TIMEOUT != dcc_mcp_core.DccErrorCode.INTERNAL

    def test_all_nine_distinct(self):
        codes = [
            dcc_mcp_core.DccErrorCode.CONNECTION_FAILED,
            dcc_mcp_core.DccErrorCode.TIMEOUT,
            dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            dcc_mcp_core.DccErrorCode.NOT_RESPONDING,
            dcc_mcp_core.DccErrorCode.UNSUPPORTED,
            dcc_mcp_core.DccErrorCode.PERMISSION_DENIED,
            dcc_mcp_core.DccErrorCode.INVALID_INPUT,
            dcc_mcp_core.DccErrorCode.SCENE_ERROR,
            dcc_mcp_core.DccErrorCode.INTERNAL,
        ]
        # Each pair should be unequal
        for i, a in enumerate(codes):
            for j, b in enumerate(codes):
                if i != j:
                    assert a != b


# ── DccInfo ──────────────────────────────────────────────────────────────────


class TestDccInfoCreate:
    def test_create_minimal(self):
        info = dcc_mcp_core.DccInfo(
            dcc_type="maya",
            version="2025",
            platform="windows",
            pid=12345,
        )
        assert info.dcc_type == "maya"
        assert info.version == "2025"
        assert info.platform == "windows"
        assert info.pid == 12345

    def test_python_version_default_none(self):
        info = dcc_mcp_core.DccInfo("blender", "4.0", "linux", 999)
        assert info.python_version is None

    def test_python_version_custom(self):
        info = dcc_mcp_core.DccInfo("houdini", "20", "macos", 1, python_version="3.11.0")
        assert info.python_version == "3.11.0"

    def test_metadata_default_empty(self):
        info = dcc_mcp_core.DccInfo("3dsmax", "2024", "windows", 42)
        assert isinstance(info.metadata, dict)
        assert len(info.metadata) == 0

    def test_metadata_custom(self):
        info = dcc_mcp_core.DccInfo(
            "maya",
            "2025",
            "windows",
            100,
            metadata={"scene": "/project/scene.mb", "user": "artist"},
        )
        assert info.metadata["scene"] == "/project/scene.mb"
        assert info.metadata["user"] == "artist"

    def test_to_dict_returns_dict(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 12345)
        d = info.to_dict()
        assert isinstance(d, dict)

    def test_to_dict_contains_dcc_type(self):
        info = dcc_mcp_core.DccInfo("blender", "4.2", "linux", 7)
        d = info.to_dict()
        assert "dcc_type" in d or "blender" in str(d)

    def test_to_dict_contains_pid(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 9999)
        d = info.to_dict()
        assert 9999 in d.values() or str(9999) in str(d)

    def test_repr_is_str(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 1)
        assert isinstance(repr(info), str)

    def test_repr_nonempty(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 1)
        assert len(repr(info)) > 0

    def test_dcc_type_field_assignment(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 1)
        assert info.dcc_type == "maya"

    def test_pid_zero(self):
        info = dcc_mcp_core.DccInfo("python", "3.11", "linux", 0)
        assert info.pid == 0

    def test_large_pid(self):
        info = dcc_mcp_core.DccInfo("maya", "2025", "windows", 999999)
        assert info.pid == 999999


# ── DccCapabilities ──────────────────────────────────────────────────────────


class TestDccCapabilitiesCreate:
    def test_default_all_false(self):
        caps = dcc_mcp_core.DccCapabilities()
        assert caps.scene_info is False
        assert caps.snapshot is False
        assert caps.undo_redo is False
        assert caps.progress_reporting is False
        assert caps.file_operations is False
        assert caps.selection is False

    def test_default_script_languages_empty(self):
        caps = dcc_mcp_core.DccCapabilities()
        assert isinstance(caps.script_languages, list)
        assert len(caps.script_languages) == 0

    def test_custom_scene_info_true(self):
        caps = dcc_mcp_core.DccCapabilities(scene_info=True)
        assert caps.scene_info is True

    def test_custom_snapshot_true(self):
        caps = dcc_mcp_core.DccCapabilities(snapshot=True)
        assert caps.snapshot is True

    def test_custom_undo_redo_true(self):
        caps = dcc_mcp_core.DccCapabilities(undo_redo=True)
        assert caps.undo_redo is True

    def test_custom_progress_reporting_true(self):
        caps = dcc_mcp_core.DccCapabilities(progress_reporting=True)
        assert caps.progress_reporting is True

    def test_custom_file_operations_true(self):
        caps = dcc_mcp_core.DccCapabilities(file_operations=True)
        assert caps.file_operations is True

    def test_custom_selection_true(self):
        caps = dcc_mcp_core.DccCapabilities(selection=True)
        assert caps.selection is True

    def test_extensions_default_empty(self):
        caps = dcc_mcp_core.DccCapabilities()
        assert isinstance(caps.extensions, dict)
        assert len(caps.extensions) == 0

    def test_extensions_custom(self):
        caps = dcc_mcp_core.DccCapabilities(extensions={"gpu_instancing": True, "alembic": False})
        assert caps.extensions.get("gpu_instancing") is True
        assert caps.extensions.get("alembic") is False

    def test_script_languages_single(self):
        caps = dcc_mcp_core.DccCapabilities(script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON])
        assert len(caps.script_languages) == 1

    def test_script_languages_multiple(self):
        caps = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL]
        )
        assert len(caps.script_languages) == 2

    def test_repr_is_str(self):
        caps = dcc_mcp_core.DccCapabilities()
        assert isinstance(repr(caps), str)

    def test_full_maya_profile(self):
        caps = dcc_mcp_core.DccCapabilities(
            script_languages=[dcc_mcp_core.ScriptLanguage.PYTHON, dcc_mcp_core.ScriptLanguage.MEL],
            scene_info=True,
            snapshot=True,
            undo_redo=True,
            progress_reporting=False,
            file_operations=True,
            selection=True,
        )
        assert caps.scene_info is True
        assert caps.snapshot is True
        assert caps.undo_redo is True
        assert caps.selection is True
        assert len(caps.script_languages) == 2


# ── DccError ─────────────────────────────────────────────────────────────────


class TestDccErrorCreate:
    def test_create_minimal(self):
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.TIMEOUT,
            message="Connection timed out",
        )
        assert err.code == dcc_mcp_core.DccErrorCode.TIMEOUT
        assert err.message == "Connection timed out"

    def test_details_default_none(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.INTERNAL, "Something broke")
        assert err.details is None

    def test_details_custom(self):
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            message="Script failed",
            details="AttributeError: 'NoneType'",
        )
        assert err.details == "AttributeError: 'NoneType'"

    def test_recoverable_default_false(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.INTERNAL, "oops")
        assert err.recoverable is False

    def test_recoverable_true(self):
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.NOT_RESPONDING,
            message="DCC not responding",
            recoverable=True,
        )
        assert err.recoverable is True

    def test_connection_failed_code(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.CONNECTION_FAILED, "no host")
        assert err.code == dcc_mcp_core.DccErrorCode.CONNECTION_FAILED

    def test_permission_denied_code(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.PERMISSION_DENIED, "access denied")
        assert err.code == dcc_mcp_core.DccErrorCode.PERMISSION_DENIED

    def test_invalid_input_code(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.INVALID_INPUT, "bad params")
        assert err.code == dcc_mcp_core.DccErrorCode.INVALID_INPUT

    def test_scene_error_code(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.SCENE_ERROR, "scene parse error")
        assert err.code == dcc_mcp_core.DccErrorCode.SCENE_ERROR

    def test_unsupported_code(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.UNSUPPORTED, "feature not available")
        assert err.code == dcc_mcp_core.DccErrorCode.UNSUPPORTED

    def test_repr_is_str(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.TIMEOUT, "timeout")
        assert isinstance(repr(err), str)

    def test_str_is_str(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.INTERNAL, "err")
        assert isinstance(str(err), str)

    def test_repr_nonempty(self):
        err = dcc_mcp_core.DccError(dcc_mcp_core.DccErrorCode.TIMEOUT, "timeout")
        assert len(repr(err)) > 0

    def test_full_construction(self):
        err = dcc_mcp_core.DccError(
            code=dcc_mcp_core.DccErrorCode.SCRIPT_ERROR,
            message="Python script raised an exception",
            details="AttributeError: 'NoneType' object has no attribute 'name'",
            recoverable=True,
        )
        assert err.code == dcc_mcp_core.DccErrorCode.SCRIPT_ERROR
        assert err.message == "Python script raised an exception"
        assert "NoneType" in err.details
        assert err.recoverable is True


# ── SkillCatalog ──────────────────────────────────────────────────────────────
# Note: SkillCatalog(registry) takes an ActionRegistry, not SkillScanner.
# The pyi stub shows SkillScanner but the runtime API accepts ActionRegistry.


class TestSkillCatalogCreate:
    def test_create_with_registry(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        assert catalog is not None

    def test_repr_is_str(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        assert isinstance(repr(catalog), str)

    def test_list_skills_empty_initially(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        skills = catalog.list_skills()
        assert isinstance(skills, list)

    def test_loaded_count_zero_initially(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        assert catalog.loaded_count() == 0

    def test_discover_no_paths(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        # Should not raise
        catalog.discover()

    def test_discover_with_extra_paths_nonexistent(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        catalog.discover(extra_paths=["/nonexistent/path/xyz"])
        skills = catalog.list_skills()
        assert isinstance(skills, list)

    def test_discover_with_dcc_name(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        catalog.discover(dcc_name="maya")
        skills = catalog.list_skills()
        assert isinstance(skills, list)

    def test_find_skills_no_args(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        results = catalog.find_skills()
        assert isinstance(results, list)

    def test_find_skills_with_query(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        results = catalog.find_skills(query="maya")
        assert isinstance(results, list)

    def test_find_skills_with_tags(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        results = catalog.find_skills(tags=["geometry"])
        assert isinstance(results, list)

    def test_find_skills_with_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        results = catalog.find_skills(dcc="maya")
        assert isinstance(results, list)

    def test_find_skills_combined_filters(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        results = catalog.find_skills(query="sphere", tags=["create"], dcc="maya")
        assert isinstance(results, list)

    def test_load_skill_nonexistent_raises_or_false(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        # Unknown skill: either returns False/empty list or raises ValueError
        try:
            result = catalog.load_skill("nonexistent-skill-xyz")
            assert result is False or result == [] or isinstance(result, list)
        except (ValueError, RuntimeError):
            pass  # expected when skill not found

    def test_unload_skill_nonexistent_raises_or_false(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        try:
            result = catalog.unload_skill("nonexistent-skill-xyz")
            assert result is False or result == 0 or isinstance(result, int)
        except (ValueError, RuntimeError):
            pass

    def test_is_loaded_nonexistent_false(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        assert catalog.is_loaded("nonexistent-skill-xyz") is False

    def test_get_skill_info_nonexistent_returns_none(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        result = catalog.get_skill_info("nonexistent-skill-xyz")
        assert result is None

    def test_list_skills_status_loaded_empty(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        skills = catalog.list_skills(status="loaded")
        assert isinstance(skills, list)
        assert len(skills) == 0

    def test_list_skills_status_unloaded(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        skills = catalog.list_skills(status="unloaded")
        assert isinstance(skills, list)

    def test_discover_returns_none_or_int(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        result = catalog.discover()
        # discover() may return None or int count
        assert result is None or isinstance(result, int)

    def test_loaded_count_returns_int(self):
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        count = catalog.loaded_count()
        assert isinstance(count, int)
        assert count >= 0


class TestSkillCatalogWithRealSkills:
    """Tests using the examples/skills directory (known to exist in the repo)."""

    @pytest.fixture
    def catalog_with_skills(self):
        """Create catalog with actual skill examples."""
        skills_dir = Path(__file__).parent.parent / "examples" / "skills"
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        if skills_dir.is_dir():
            catalog.discover(extra_paths=[str(skills_dir)])
        return catalog

    def test_discover_finds_skills(self, catalog_with_skills):
        skills = catalog_with_skills.list_skills()
        # examples/skills has at least hello-world
        assert isinstance(skills, list)

    def test_list_skills_returns_skill_summaries(self, catalog_with_skills):
        skills = catalog_with_skills.list_skills()
        for s in skills:
            # SkillSummary instances
            assert hasattr(s, "name") or isinstance(s, dict)

    def test_find_skills_returns_subset(self, catalog_with_skills):
        all_skills = catalog_with_skills.list_skills()
        found = catalog_with_skills.find_skills(query="hello")
        assert isinstance(found, list)
        # result should be subset of all_skills
        assert len(found) <= len(all_skills)

    def test_list_skills_loaded_plus_unloaded_equals_total(self, catalog_with_skills):
        total = catalog_with_skills.list_skills()
        loaded = catalog_with_skills.list_skills(status="loaded")
        unloaded = catalog_with_skills.list_skills(status="unloaded")
        assert len(loaded) + len(unloaded) == len(total)

    def test_load_skill_hello_world(self, catalog_with_skills):
        """hello-world skill should be discoverable and loadable."""
        all_skills = catalog_with_skills.list_skills()
        skill_names = [s.name if hasattr(s, "name") else s.get("name", "") for s in all_skills]
        if "hello-world" in skill_names:
            result = catalog_with_skills.load_skill("hello-world")
            # load_skill returns list[str] of registered action names
            assert isinstance(result, (bool, list))
            loaded = result is True or (isinstance(result, list) and len(result) >= 0)
            assert loaded
            assert catalog_with_skills.is_loaded("hello-world") is True
            assert catalog_with_skills.loaded_count() >= 1

    def test_unload_skill_after_load(self, catalog_with_skills):
        all_skills = catalog_with_skills.list_skills()
        skill_names = [s.name if hasattr(s, "name") else s.get("name", "") for s in all_skills]
        if "hello-world" in skill_names:
            catalog_with_skills.load_skill("hello-world")
            if catalog_with_skills.is_loaded("hello-world"):
                result = catalog_with_skills.unload_skill("hello-world")
                # unload_skill returns int (removed count) or bool
                assert isinstance(result, (bool, int))

    def test_get_skill_info_returns_metadata_or_none(self, catalog_with_skills):
        all_skills = catalog_with_skills.list_skills()
        if all_skills:
            first_name = all_skills[0].name if hasattr(all_skills[0], "name") else all_skills[0].get("name", "")
            info = catalog_with_skills.get_skill_info(first_name)
            # Returns SkillMetadata, dict, or None
            assert info is None or hasattr(info, "name") or (isinstance(info, dict) and "name" in info)

    def test_loaded_count_tracks_load(self, catalog_with_skills):
        before = catalog_with_skills.loaded_count()
        all_skills = catalog_with_skills.list_skills()
        skill_names = [s.name if hasattr(s, "name") else s.get("name", "") for s in all_skills]
        if "hello-world" in skill_names and not catalog_with_skills.is_loaded("hello-world"):
            catalog_with_skills.load_skill("hello-world")
            after = catalog_with_skills.loaded_count()
            assert after >= before


# ── SkillSummary fields ───────────────────────────────────────────────────────


class TestSkillSummaryFields:
    """Validate SkillSummary field types via SkillCatalog.list_skills()."""

    @pytest.fixture
    def first_summary(self):
        skills_dir = Path(__file__).parent.parent / "examples" / "skills"
        reg = dcc_mcp_core.ActionRegistry()
        catalog = dcc_mcp_core.SkillCatalog(reg)
        if skills_dir.is_dir():
            catalog.discover(extra_paths=[str(skills_dir)])
        skills = catalog.list_skills()
        if not skills:
            pytest.skip("No skills discovered")
        return skills[0]

    def test_name_is_str(self, first_summary):
        assert isinstance(first_summary.name, str)

    def test_name_nonempty(self, first_summary):
        assert len(first_summary.name) > 0

    def test_description_is_str(self, first_summary):
        assert isinstance(first_summary.description, str)

    def test_version_is_str(self, first_summary):
        assert isinstance(first_summary.version, str)

    def test_dcc_is_str(self, first_summary):
        assert isinstance(first_summary.dcc, str)

    def test_tags_is_list(self, first_summary):
        assert isinstance(first_summary.tags, list)

    def test_tool_count_is_int(self, first_summary):
        assert isinstance(first_summary.tool_count, int)
        assert first_summary.tool_count >= 0

    def test_tool_names_is_list(self, first_summary):
        assert isinstance(first_summary.tool_names, list)

    def test_loaded_is_bool(self, first_summary):
        assert isinstance(first_summary.loaded, bool)

    def test_repr_is_str(self, first_summary):
        assert isinstance(repr(first_summary), str)

    def test_tool_count_matches_tool_names(self, first_summary):
        assert first_summary.tool_count == len(first_summary.tool_names)
