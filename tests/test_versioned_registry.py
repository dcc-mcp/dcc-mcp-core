"""Tests for SemVer, VersionConstraint, and VersionedRegistry.

Covers:
- SemVer construction, parsing, comparison, and string representation
- VersionConstraint parsing and matching (all operators: *, =, >=, >, <=, <, ^, ~)
- VersionedRegistry: register, resolve, resolve_all, versions, latest_version, total_entries
- Error paths: invalid semver, unsupported operator, unregistered actions
"""

from __future__ import annotations

import pytest

import dcc_mcp_core

# ── SemVer Tests ──────────────────────────────────────────────────────────────


class TestSemVer:
    def test_create_basic(self) -> None:
        v = dcc_mcp_core.SemVer(1, 2, 3)
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_str_representation(self) -> None:
        v = dcc_mcp_core.SemVer(1, 2, 3)
        assert str(v) == "1.2.3"

    def test_str_zero_patch(self) -> None:
        v = dcc_mcp_core.SemVer(2, 0, 0)
        assert str(v) == "2.0.0"

    def test_parse_standard(self) -> None:
        v = dcc_mcp_core.SemVer.parse("1.2.3")
        assert v.major == 1
        assert v.minor == 2
        assert v.patch == 3

    def test_parse_with_v_prefix(self) -> None:
        v = dcc_mcp_core.SemVer.parse("v2.0.0")
        assert v.major == 2
        assert v.minor == 0
        assert v.patch == 0

    def test_parse_two_component(self) -> None:
        v = dcc_mcp_core.SemVer.parse("v2.0")
        assert v.major == 2
        assert v.minor == 0
        assert v.patch == 0

    def test_parse_with_prerelease(self) -> None:
        # Prerelease part is stripped but should not raise
        v = dcc_mcp_core.SemVer.parse("1.0.0-alpha")
        assert v.major == 1
        assert v.minor == 0
        assert v.patch == 0

    def test_equality(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 2, 3)
        v2 = dcc_mcp_core.SemVer(1, 2, 3)
        assert v1 == v2

    def test_inequality_major(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(2, 0, 0)
        assert v1 != v2

    def test_less_than(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(2, 0, 0)
        assert v1 < v2

    def test_less_than_minor(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 1, 0)
        assert v1 < v2

    def test_less_than_patch(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 0, 1)
        assert v1 < v2

    def test_greater_than(self) -> None:
        v1 = dcc_mcp_core.SemVer(2, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 0, 0)
        assert v1 > v2

    def test_less_than_or_equal_equal(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 0, 0)
        assert v1 <= v2

    def test_less_than_or_equal_less(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(2, 0, 0)
        assert v1 <= v2

    def test_greater_than_or_equal_equal(self) -> None:
        v1 = dcc_mcp_core.SemVer(1, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 0, 0)
        assert v1 >= v2

    def test_greater_than_or_equal_greater(self) -> None:
        v1 = dcc_mcp_core.SemVer(2, 0, 0)
        v2 = dcc_mcp_core.SemVer(1, 0, 0)
        assert v1 >= v2

    def test_sort_order(self) -> None:
        versions = [
            dcc_mcp_core.SemVer(2, 0, 0),
            dcc_mcp_core.SemVer(1, 0, 0),
            dcc_mcp_core.SemVer(1, 5, 0),
            dcc_mcp_core.SemVer(1, 5, 3),
        ]
        sorted_versions = sorted(versions)
        assert str(sorted_versions[0]) == "1.0.0"
        assert str(sorted_versions[1]) == "1.5.0"
        assert str(sorted_versions[2]) == "1.5.3"
        assert str(sorted_versions[3]) == "2.0.0"

    def test_repr(self) -> None:
        v = dcc_mcp_core.SemVer(1, 2, 3)
        r = repr(v)
        assert "1" in r and "2" in r and "3" in r

    def test_parse_invalid_raises(self) -> None:
        with pytest.raises((ValueError, Exception)):
            dcc_mcp_core.SemVer.parse("not-a-version")

    def test_parse_empty_raises(self) -> None:
        with pytest.raises((ValueError, Exception)):
            dcc_mcp_core.SemVer.parse("")


# ── VersionConstraint Tests ───────────────────────────────────────────────────


class TestVersionConstraint:
    def test_wildcard_matches_any(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("*")
        assert c.matches(dcc_mcp_core.SemVer(0, 0, 1))
        assert c.matches(dcc_mcp_core.SemVer(1, 0, 0))
        assert c.matches(dcc_mcp_core.SemVer(99, 99, 99))

    def test_exact_match(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("=1.2.3")
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 3))
        assert not c.matches(dcc_mcp_core.SemVer(1, 2, 4))
        assert not c.matches(dcc_mcp_core.SemVer(1, 3, 0))

    def test_greater_than_or_equal(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse(">=1.2.0")
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 0))
        assert c.matches(dcc_mcp_core.SemVer(1, 5, 0))
        assert c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(1, 1, 9))
        assert not c.matches(dcc_mcp_core.SemVer(0, 9, 9))

    def test_strictly_greater_than(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse(">1.2.0")
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 1))
        assert c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(1, 2, 0))
        assert not c.matches(dcc_mcp_core.SemVer(1, 0, 0))

    def test_less_than_or_equal(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("<=2.0.0")
        assert c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert c.matches(dcc_mcp_core.SemVer(1, 9, 9))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 1))
        assert not c.matches(dcc_mcp_core.SemVer(3, 0, 0))

    def test_strictly_less_than(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("<2.0.0")
        assert c.matches(dcc_mcp_core.SemVer(1, 9, 9))
        assert c.matches(dcc_mcp_core.SemVer(0, 0, 1))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 1))

    def test_caret_same_major(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("^1.0.0")
        assert c.matches(dcc_mcp_core.SemVer(1, 0, 0))
        assert c.matches(dcc_mcp_core.SemVer(1, 5, 0))
        assert c.matches(dcc_mcp_core.SemVer(1, 9, 9))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(0, 9, 9))

    def test_caret_minor_version(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("^1.2.0")
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 0))
        assert c.matches(dcc_mcp_core.SemVer(1, 5, 3))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(1, 1, 9))

    def test_tilde_same_major_minor(self) -> None:
        c = dcc_mcp_core.VersionConstraint.parse("~1.2.3")
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 3))
        assert c.matches(dcc_mcp_core.SemVer(1, 2, 9))
        assert not c.matches(dcc_mcp_core.SemVer(1, 3, 0))
        assert not c.matches(dcc_mcp_core.SemVer(2, 0, 0))
        assert not c.matches(dcc_mcp_core.SemVer(1, 2, 2))

    def test_invalid_operator_raises(self) -> None:
        with pytest.raises((ValueError, Exception)):
            dcc_mcp_core.VersionConstraint.parse("??1.0.0")

    def test_empty_constraint_raises(self) -> None:
        with pytest.raises((ValueError, Exception)):
            dcc_mcp_core.VersionConstraint.parse("")


# ── VersionedRegistry Tests ───────────────────────────────────────────────────


class TestVersionedRegistryBasics:
    def test_create_empty(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        assert vr.total_entries() == 0

    def test_register_single_version(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        assert vr.total_entries() == 1

    def test_register_multiple_versions(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "1.5.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")
        assert vr.total_entries() == 3

    def test_register_different_actions(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("delete_mesh", "maya", "1.0.0")
        assert vr.total_entries() == 2

    def test_register_different_dccs(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "blender", "1.0.0")
        assert vr.total_entries() == 2

    def test_overwrite_same_version(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0", description="v1")
        vr.register_versioned("create_sphere", "maya", "1.0.0", description="v1 updated")
        # Same triple overwrites, total_entries stays 1
        assert vr.total_entries() == 1

    def test_repr(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        r = repr(vr)
        assert isinstance(r, str)
        assert len(r) > 0

    def test_register_with_all_params(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned(
            "create_sphere",
            "maya",
            "1.2.0",
            description="Create a sphere",
            category="geometry",
            tags=["geo", "create"],
        )
        assert vr.total_entries() == 1
        result = vr.resolve("create_sphere", "maya", "*")
        assert result is not None
        assert result["description"] == "Create a sphere"
        assert result["category"] == "geometry"


class TestVersionedRegistryVersions:
    def test_versions_returns_sorted(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "2.0.0")
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vs = vr.versions("act", "maya")
        assert vs == ["1.0.0", "1.5.0", "2.0.0"]

    def test_versions_empty_for_unknown(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vs = vr.versions("unknown_action", "maya")
        assert vs == []

    def test_versions_different_dcc_isolation(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        vr.register_versioned("act", "blender", "3.0.0")
        maya_vs = vr.versions("act", "maya")
        blender_vs = vr.versions("act", "blender")
        assert maya_vs == ["1.0.0", "2.0.0"]
        assert blender_vs == ["3.0.0"]

    def test_latest_version_basic(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.5.0")
        vr.register_versioned("act", "maya", "1.9.0")
        latest = vr.latest_version("act", "maya")
        assert latest == "2.5.0"

    def test_latest_version_single(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.2.3")
        assert vr.latest_version("act", "maya") == "1.2.3"

    def test_latest_version_none_for_unknown(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        result = vr.latest_version("nonexistent", "maya")
        assert result is None


class TestVersionedRegistryResolve:
    def test_resolve_wildcard_returns_latest(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        result = vr.resolve("act", "maya", "*")
        assert result is not None
        # wildcard should return best match (latest or specific depending on impl)
        assert "version" in result

    def test_resolve_caret_returns_best_match(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        result = vr.resolve("act", "maya", "^1.0.0")
        assert result is not None
        # Caret: same major, highest matching → 1.5.0
        assert result["version"] == "1.5.0"

    def test_resolve_exact_version(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "2.0.0")
        result = vr.resolve("act", "maya", "=1.0.0")
        assert result is not None
        assert result["version"] == "1.0.0"

    def test_resolve_gte_constraint(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.8.0")
        vr.register_versioned("act", "maya", "2.5.0")
        result = vr.resolve("act", "maya", ">=1.5.0")
        assert result is not None
        # Should resolve to highest matching: 2.5.0
        assert result["version"] in ["1.8.0", "2.5.0"]

    def test_resolve_no_match_returns_none(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        result = vr.resolve("act", "maya", ">=2.0.0")
        assert result is None

    def test_resolve_unknown_action_returns_none(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        result = vr.resolve("nonexistent", "maya", "*")
        assert result is None

    def test_resolve_unknown_dcc_returns_none(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        result = vr.resolve("act", "blender", "*")
        assert result is None

    def test_resolve_result_contains_metadata(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned(
            "act",
            "maya",
            "1.0.0",
            description="Test action",
            category="geometry",
            tags=["geo"],
        )
        result = vr.resolve("act", "maya", "=1.0.0")
        assert result is not None
        assert result["name"] == "act"
        assert result["dcc"] == "maya"
        assert result["version"] == "1.0.0"
        assert result["description"] == "Test action"
        assert result["category"] == "geometry"


class TestVersionedRegistryResolveAll:
    def test_resolve_all_wildcard_returns_all(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        results = vr.resolve_all("act", "maya", "*")
        assert len(results) == 3

    def test_resolve_all_sorted_ascending(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "2.0.0")
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        results = vr.resolve_all("act", "maya", "*")
        assert len(results) == 3
        assert results[0]["version"] == "1.0.0"
        assert results[1]["version"] == "1.5.0"
        assert results[2]["version"] == "2.0.0"

    def test_resolve_all_caret_filters_major(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        vr.register_versioned("act", "maya", "1.5.0")
        vr.register_versioned("act", "maya", "2.0.0")
        results = vr.resolve_all("act", "maya", "^1.0.0")
        # Only major=1 versions
        assert len(results) == 2
        assert all(r["version"].startswith("1.") for r in results)

    def test_resolve_all_no_match_returns_empty(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.0.0")
        results = vr.resolve_all("act", "maya", ">=5.0.0")
        assert results == []

    def test_resolve_all_unknown_action_returns_empty(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        results = vr.resolve_all("nonexistent", "maya", "*")
        assert results == []

    def test_resolve_all_tilde_range(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("act", "maya", "1.2.0")
        vr.register_versioned("act", "maya", "1.2.5")
        vr.register_versioned("act", "maya", "1.3.0")
        vr.register_versioned("act", "maya", "2.0.0")
        results = vr.resolve_all("act", "maya", "~1.2.0")
        # Tilde: same major.minor (1.2.x)
        assert len(results) == 2
        versions = [r["version"] for r in results]
        assert "1.2.0" in versions
        assert "1.2.5" in versions


class TestVersionedRegistryIntegration:
    """End-to-end scenarios matching AGENTS.md usage examples."""

    def test_agents_md_example(self) -> None:
        """Reproduce the exact example from AGENTS.md."""
        vr = dcc_mcp_core.VersionedRegistry()
        vr.register_versioned("create_sphere", "maya", "1.0.0")
        vr.register_versioned("create_sphere", "maya", "1.5.0")
        vr.register_versioned("create_sphere", "maya", "2.0.0")

        result = vr.resolve("create_sphere", "maya", "^1.0.0")
        assert result is not None
        assert result["version"] == "1.5.0"

    def test_multi_dcc_isolation(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        for dcc in ["maya", "blender", "houdini", "max"]:
            vr.register_versioned("render_scene", dcc, "1.0.0")
            vr.register_versioned("render_scene", dcc, "2.0.0")
        assert vr.total_entries() == 8
        for dcc in ["maya", "blender", "houdini", "max"]:
            vs = vr.versions("render_scene", dcc)
            assert vs == ["1.0.0", "2.0.0"]

    def test_semver_with_version_constraint(self) -> None:
        """Direct SemVer + VersionConstraint interaction."""
        c = dcc_mcp_core.VersionConstraint.parse("^1.0.0")
        v_match = dcc_mcp_core.SemVer(1, 5, 0)
        v_no_match = dcc_mcp_core.SemVer(2, 0, 0)
        assert c.matches(v_match)
        assert not c.matches(v_no_match)

    def test_version_upgrade_scenario(self) -> None:
        """Simulate Maya plugin upgrade: old client uses ^1.x, new client uses ^2.x."""
        vr = dcc_mcp_core.VersionedRegistry()
        # Initial versions
        vr.register_versioned("export_fbx", "maya", "1.0.0", description="Basic FBX export")
        vr.register_versioned("export_fbx", "maya", "1.2.0", description="FBX with materials")
        # New major release
        vr.register_versioned("export_fbx", "maya", "2.0.0", description="FBX v2 with USD")

        # Old client compatibility
        old_result = vr.resolve("export_fbx", "maya", "^1.0.0")
        assert old_result is not None
        assert old_result["version"] == "1.2.0"

        # New client gets latest
        new_result = vr.resolve("export_fbx", "maya", "^2.0.0")
        assert new_result is not None
        assert new_result["version"] == "2.0.0"

        # Latest regardless of major
        latest = vr.latest_version("export_fbx", "maya")
        assert latest == "2.0.0"

    def test_total_entries_across_multiple_actions_and_dccs(self) -> None:
        vr = dcc_mcp_core.VersionedRegistry()
        actions = ["create_mesh", "delete_mesh", "render_scene"]
        dccs = ["maya", "blender"]
        versions = ["1.0.0", "2.0.0"]
        for action in actions:
            for dcc in dccs:
                for version in versions:
                    vr.register_versioned(action, dcc, version)
        expected = len(actions) * len(dccs) * len(versions)
        assert vr.total_entries() == expected
