"""Tests for progressive skill discovery — SkillPolicy, SkillScope, SkillDependencies.

Covers the ideal behavior of:
- SkillPolicy: allow_implicit_invocation and product filtering
- SkillDependencies: external dependency declarations via JSON setter
- SkillScope: trust-level fields on SkillSummary (scope, implicit_invocation)
- SkillCatalog: discover/list/find/load/unload pipeline
- Progressive filter: only show tools matching current DCC product

These tests verify the complete progressive discovery mechanism from
metadata API through catalog-level discovery and filtering.
"""

from __future__ import annotations

import json
from pathlib import Path
import tempfile
from typing import Any

import pytest

import dcc_mcp_core
from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import SkillMetadata
from dcc_mcp_core import SkillSummary
from dcc_mcp_core import ToolRegistry

# ── Helpers ───────────────────────────────────────────────────────────────────


def make_skill_dir(tmp_path: Path, name: str, dcc: str = "maya") -> Path:
    """Create a minimal valid skill directory."""
    skill_dir = tmp_path / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    (skill_dir / "scripts" / "run.py").write_text('import json,sys; print(json.dumps({"success":True,"message":"ok"}))')
    content = "\n".join(
        [
            "---",
            f"name: {name}",
            f"dcc: {dcc}",
            "---",
            f"# {name}",
            "A test skill.",
        ]
    )
    (skill_dir / "SKILL.md").write_text(content)
    return skill_dir


def make_catalog(extra_paths: list[str]) -> SkillCatalog:
    """Create a fresh SkillCatalog and run discover()."""
    cat = SkillCatalog(ToolRegistry())
    cat.discover(extra_paths=extra_paths)
    return cat


# ── SkillMetadata.policy: setter/getter/methods ───────────────────────────────


class TestSkillMetadataPolicy:
    """SkillPolicy is tested through the SkillMetadata Python setter API."""

    def test_default_metadata_allows_implicit_invocation(self) -> None:
        """SkillMetadata with no policy allows implicit invocation (default True)."""
        md = SkillMetadata("test-skill")
        assert md.is_implicit_invocation_allowed() is True

    def test_default_metadata_matches_any_product(self) -> None:
        """SkillMetadata with no policy matches any product (default True)."""
        md = SkillMetadata("test-skill")
        assert md.matches_product("maya") is True
        assert md.matches_product("blender") is True
        assert md.matches_product("houdini") is True
        assert md.matches_product("photoshop") is True

    def test_policy_none_by_default(self) -> None:
        """SkillMetadata.policy returns None when no policy is set."""
        md = SkillMetadata("test-skill")
        assert md.policy is None

    def test_set_policy_blocks_implicit_invocation(self) -> None:
        """Setting policy.allow_implicit_invocation=False blocks implicit invocation."""
        md = SkillMetadata("secure-skill")
        md.policy = json.dumps({"allow_implicit_invocation": False, "products": []})
        assert md.is_implicit_invocation_allowed() is False

    def test_set_policy_allows_explicit_true(self) -> None:
        """Setting policy.allow_implicit_invocation=True explicitly allows it."""
        md = SkillMetadata("open-skill")
        md.policy = json.dumps({"allow_implicit_invocation": True, "products": []})
        assert md.is_implicit_invocation_allowed() is True

    def test_set_policy_clear_with_none(self) -> None:
        """Setting policy to None clears it; defaults back to True."""
        md = SkillMetadata("resetable-skill")
        md.policy = json.dumps({"allow_implicit_invocation": False})
        assert md.is_implicit_invocation_allowed() is False
        md.policy = None
        assert md.is_implicit_invocation_allowed() is True

    def test_set_policy_product_filter_restricts_to_listed(self) -> None:
        """policy.products list restricts matches_product to listed DCCs."""
        md = SkillMetadata("maya-only")
        md.policy = json.dumps({"products": ["maya", "houdini"]})
        assert md.matches_product("maya") is True
        assert md.matches_product("houdini") is True
        assert md.matches_product("blender") is False
        assert md.matches_product("photoshop") is False

    def test_set_policy_empty_products_matches_all(self) -> None:
        """policy.products=[] means available for all DCC products."""
        md = SkillMetadata("all-dcc")
        md.policy = json.dumps({"products": []})
        assert md.matches_product("maya") is True
        assert md.matches_product("blender") is True

    def test_policy_getter_returns_json_string(self) -> None:
        """Policy getter returns a valid JSON string after set."""
        md = SkillMetadata("json-policy")
        md.policy = json.dumps({"allow_implicit_invocation": False, "products": ["maya"]})
        policy_val = md.policy
        assert policy_val is not None
        parsed = json.loads(policy_val)
        assert isinstance(parsed, dict)

    def test_policy_case_insensitive_product_matching(self) -> None:
        """Product matching is case-insensitive."""
        md = SkillMetadata("ci-skill")
        md.policy = json.dumps({"products": ["Maya"]})
        assert md.matches_product("maya") is True
        assert md.matches_product("MAYA") is True
        assert md.matches_product("Maya") is True


# ── SkillMetadata.external_deps ───────────────────────────────────────────────


class TestSkillMetadataExternalDeps:
    """external_deps declares MCP, env_var, and binary requirements."""

    def test_external_deps_none_by_default(self) -> None:
        """SkillMetadata.external_deps returns None when no deps are set."""
        md = SkillMetadata("nodeps-skill")
        assert md.external_deps is None

    def test_set_external_deps_env_var(self) -> None:
        """Setting external_deps with env_var deps stores them as JSON."""
        md = SkillMetadata("env-dep-skill")
        deps = {"tools": [{"type": "env_var", "value": "MAYA_LICENSE_KEY"}]}
        md.external_deps = json.dumps(deps)
        val = md.external_deps
        assert val is not None
        parsed = json.loads(val)
        assert isinstance(parsed, dict)

    def test_set_external_deps_bin(self) -> None:
        """Setting external_deps with binary deps stores them."""
        md = SkillMetadata("bin-dep-skill")
        deps = {"tools": [{"type": "bin", "value": "ffmpeg"}]}
        md.external_deps = json.dumps(deps)
        assert md.external_deps is not None

    def test_set_external_deps_mcp(self) -> None:
        """Setting external_deps with MCP server deps stores them."""
        md = SkillMetadata("mcp-dep-skill")
        deps = {
            "tools": [
                {"type": "mcp", "value": "render-server", "description": "Needs render server"},
            ]
        }
        md.external_deps = json.dumps(deps)
        assert md.external_deps is not None

    def test_external_deps_roundtrip(self) -> None:
        """external_deps JSON roundtrip preserves structure."""
        md = SkillMetadata("roundtrip-skill")
        original = {
            "tools": [
                {"type": "env_var", "value": "HOUDINI_PATH"},
                {"type": "bin", "value": "hython"},
            ]
        }
        md.external_deps = json.dumps(original)
        result = json.loads(md.external_deps)
        assert "tools" in result
        assert len(result["tools"]) == 2

    def test_clear_external_deps_with_none(self) -> None:
        """Setting external_deps to None clears the dependencies."""
        md = SkillMetadata("clearable-skill")
        md.external_deps = json.dumps({"tools": [{"type": "bin", "value": "ffmpeg"}]})
        assert md.external_deps is not None
        md.external_deps = None
        assert md.external_deps is None


# ── SkillSummary: scope and implicit_invocation ───────────────────────────────


class TestSkillSummaryScopeFields:
    """SkillSummary exposes scope and implicit_invocation after catalog discovery."""

    def test_skill_summary_has_scope_field(self, tmp_path: Path) -> None:
        """SkillSummary.scope is a non-empty string after discovery."""
        make_skill_dir(tmp_path, "scope-test")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        summaries = cat.list_skills()
        assert len(summaries) >= 1
        for s in summaries:
            assert hasattr(s, "scope")
            assert isinstance(s.scope, str)

    def test_skill_summary_scope_is_valid_level(self, tmp_path: Path) -> None:
        """SkillSummary.scope is one of the recognized trust levels."""
        make_skill_dir(tmp_path, "trust-test")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        for s in cat.list_skills():
            assert s.scope in ("repo", "user", "system", "admin", "")

    def test_skill_summary_has_implicit_invocation_field(self, tmp_path: Path) -> None:
        """SkillSummary.implicit_invocation is a bool after discovery."""
        make_skill_dir(tmp_path, "implicit-test")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        for s in cat.list_skills():
            assert hasattr(s, "implicit_invocation")
            assert isinstance(s.implicit_invocation, bool)

    def test_default_skill_has_implicit_invocation_true(self, tmp_path: Path) -> None:
        """Skills without policy have implicit_invocation=True in summary."""
        make_skill_dir(tmp_path, "default-implicit")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        names = {s.name: s for s in cat.list_skills()}
        assert "default-implicit" in names
        assert names["default-implicit"].implicit_invocation is True

    def test_skill_summary_has_name_field(self, tmp_path: Path) -> None:
        """SkillSummary.name matches the skill directory name."""
        make_skill_dir(tmp_path, "named-skill")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        names = [s.name for s in cat.list_skills()]
        assert "named-skill" in names


# ── SkillCatalog progressive pipeline ────────────────────────────────────────


class TestSkillCatalogProgressivePipeline:
    """End-to-end progressive discovery: discover → list → load → unload."""

    def test_discover_returns_count_int(self, tmp_path: Path) -> None:
        """discover() returns an int count of skills found."""
        make_skill_dir(tmp_path, "count-test")
        cat = SkillCatalog(ToolRegistry())
        count = cat.discover(extra_paths=[str(tmp_path)])
        assert isinstance(count, int)
        assert count >= 1

    def test_discover_multiple_skills(self, tmp_path: Path) -> None:
        """discover() finds all skills in extra_paths."""
        for name in ("skill-alpha", "skill-beta", "skill-gamma"):
            make_skill_dir(tmp_path, name)
        cat = SkillCatalog(ToolRegistry())
        count = cat.discover(extra_paths=[str(tmp_path)])
        assert count >= 3

    def test_list_skills_returns_skill_summaries(self, tmp_path: Path) -> None:
        """list_skills() returns SkillSummary objects."""
        make_skill_dir(tmp_path, "summary-skill")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        summaries = cat.list_skills()
        assert isinstance(summaries, list)
        assert all(isinstance(s, SkillSummary) for s in summaries)

    def test_load_skill_makes_it_available(self, tmp_path: Path) -> None:
        """After load_skill(), is_loaded() returns True."""
        make_skill_dir(tmp_path, "loadable")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        cat.load_skill("loadable")
        assert cat.is_loaded("loadable") is True

    def test_unload_skill_removes_it(self, tmp_path: Path) -> None:
        """After unload_skill(), is_loaded() returns False."""
        make_skill_dir(tmp_path, "unloadable")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        cat.load_skill("unloadable")
        assert cat.is_loaded("unloadable") is True
        cat.unload_skill("unloadable")
        assert cat.is_loaded("unloadable") is False

    def test_is_loaded_false_before_load(self, tmp_path: Path) -> None:
        """is_loaded() returns False before the skill is loaded."""
        make_skill_dir(tmp_path, "pre-load")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        assert cat.is_loaded("pre-load") is False

    def test_loaded_count_increases_with_each_load(self, tmp_path: Path) -> None:
        """loaded_count() tracks the number of loaded skills."""
        for name in ("count-a", "count-b"):
            make_skill_dir(tmp_path, name)
        cat = make_catalog(extra_paths=[str(tmp_path)])
        before = cat.loaded_count()
        cat.load_skill("count-a")
        assert cat.loaded_count() == before + 1
        cat.load_skill("count-b")
        assert cat.loaded_count() == before + 2

    def test_find_skills_returns_summaries(self, tmp_path: Path) -> None:
        """find_skills() returns SkillSummary list."""
        make_skill_dir(tmp_path, "findable")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        results = cat.find_skills(query="findable")
        assert isinstance(results, list)

    def test_get_skill_info_after_load(self, tmp_path: Path) -> None:
        """get_skill_info() returns a dict with skill data after load."""
        make_skill_dir(tmp_path, "info-skill")
        cat = make_catalog(extra_paths=[str(tmp_path)])
        cat.load_skill("info-skill")
        info = cat.get_skill_info("info-skill")
        assert info is not None

    def test_load_unknown_skill_raises(self, tmp_path: Path) -> None:
        """load_skill() raises for an unknown skill name."""
        cat = SkillCatalog(ToolRegistry())
        with pytest.raises((ValueError, RuntimeError, KeyError)):
            cat.load_skill("nonexistent-skill-xyz")


# ── Multi-product filtering scenario ─────────────────────────────────────────


class TestMultiProductFiltering:
    """Simulate two skills: one for Maya, one for Blender. Policy controls visibility."""

    def test_maya_only_skill_does_not_match_blender(self) -> None:
        """A skill with products=['maya'] does not match blender."""
        md = SkillMetadata("maya-tool")
        md.policy = json.dumps({"products": ["maya"]})
        assert md.matches_product("maya") is True
        assert md.matches_product("blender") is False

    def test_blender_only_skill_does_not_match_maya(self) -> None:
        """A skill with products=['blender'] does not match maya."""
        md = SkillMetadata("blender-tool")
        md.policy = json.dumps({"products": ["blender"]})
        assert md.matches_product("blender") is True
        assert md.matches_product("maya") is False

    def test_universal_skill_matches_all_products(self) -> None:
        """A skill with empty products matches every DCC."""
        md = SkillMetadata("universal-tool")
        md.policy = json.dumps({"products": []})
        for dcc in ("maya", "blender", "houdini", "photoshop", "substance"):
            assert md.matches_product(dcc) is True, f"Should match {dcc}"

    def test_filter_skills_by_current_dcc(self) -> None:
        """Simulate gateway filtering: only return skills for active DCC."""
        skills = [
            SkillMetadata("maya-rig"),
            SkillMetadata("blender-sculpt"),
            SkillMetadata("houdini-vex"),
            SkillMetadata("universal-util"),
        ]
        skills[0].policy = json.dumps({"products": ["maya"]})
        skills[1].policy = json.dumps({"products": ["blender"]})
        skills[2].policy = json.dumps({"products": ["houdini"]})
        # universal-util has no policy → matches all

        def visible_for(dcc: str) -> list[str]:
            return [s.name for s in skills if s.matches_product(dcc)]

        maya_tools = visible_for("maya")
        assert "maya-rig" in maya_tools
        assert "universal-util" in maya_tools
        assert "blender-sculpt" not in maya_tools
        assert "houdini-vex" not in maya_tools

        blender_tools = visible_for("blender")
        assert "blender-sculpt" in blender_tools
        assert "universal-util" in blender_tools
        assert "maya-rig" not in blender_tools

    def test_implicit_invocation_gates_tool_exposure(self) -> None:
        """Skills with allow_implicit_invocation=False are hidden until load_skill."""
        md_public = SkillMetadata("public-skill")
        md_gated = SkillMetadata("gated-skill")
        md_gated.policy = json.dumps({"allow_implicit_invocation": False})

        # Public skill shows immediately in tools/list
        assert md_public.is_implicit_invocation_allowed() is True

        # Gated skill requires explicit load_skill call first
        assert md_gated.is_implicit_invocation_allowed() is False
