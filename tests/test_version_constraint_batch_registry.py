"""Deep tests for VersionConstraint.matches, SemVer comparisons/repr, and register_batch.

Covers:
- VersionConstraint.parse for all operator forms: *, >=, >, <=, <, ^, ~, =
- VersionConstraint.matches(SemVer) truth table for all operators
- SemVer repr(), comparison operators (<, >, ==, !=, <=, >=)
- ToolRegistry.register_batch: empty batch, minimal fields, same name multi-DCC,
  multi-name multi-DCC, count_actions semantics, get_all_dccs after batch
"""

from __future__ import annotations

import pytest

import dcc_mcp_core

# ---------------------------------------------------------------------------
# Helper factories
# ---------------------------------------------------------------------------


def vc(expr: str) -> dcc_mcp_core.VersionConstraint:
    return dcc_mcp_core.VersionConstraint.parse(expr)


def sv(s: str) -> dcc_mcp_core.SemVer:
    return dcc_mcp_core.SemVer.parse(s)


# ---------------------------------------------------------------------------
# VersionConstraint.parse
# ---------------------------------------------------------------------------


class TestVersionConstraintParse:
    def test_wildcard(self) -> None:
        c = vc("*")
        assert c is not None

    def test_gte(self) -> None:
        c = vc(">=1.0.0")
        assert c is not None

    def test_gt(self) -> None:
        c = vc(">1.0.0")
        assert c is not None

    def test_lte(self) -> None:
        c = vc("<=2.0.0")
        assert c is not None

    def test_lt(self) -> None:
        c = vc("<2.0.0")
        assert c is not None

    def test_caret(self) -> None:
        c = vc("^1.0.0")
        assert c is not None

    def test_tilde(self) -> None:
        c = vc("~1.2.0")
        assert c is not None

    def test_exact(self) -> None:
        c = vc("=1.2.3")
        assert c is not None

    def test_repr_contains_constraint(self) -> None:
        c = vc(">=1.5.0")
        r = repr(c)
        assert "1.5.0" in r

    def test_str_contains_constraint(self) -> None:
        c = vc("^2.0.0")
        s = str(c)
        assert "2.0.0" in s


# ---------------------------------------------------------------------------
# VersionConstraint.matches — wildcard
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesWildcard:
    def test_wildcard_matches_zero(self) -> None:
        assert vc("*").matches(sv("0.0.0")) is True

    def test_wildcard_matches_one(self) -> None:
        assert vc("*").matches(sv("1.0.0")) is True

    def test_wildcard_matches_large(self) -> None:
        assert vc("*").matches(sv("99.99.99")) is True

    def test_wildcard_matches_prerelease_level(self) -> None:
        assert vc("*").matches(sv("0.1.0")) is True


# ---------------------------------------------------------------------------
# VersionConstraint.matches — >=
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesGTE:
    def test_gte_equal_matches(self) -> None:
        assert vc(">=1.0.0").matches(sv("1.0.0")) is True

    def test_gte_greater_matches(self) -> None:
        assert vc(">=1.0.0").matches(sv("1.5.0")) is True

    def test_gte_much_greater_matches(self) -> None:
        assert vc(">=1.0.0").matches(sv("5.0.0")) is True

    def test_gte_less_no_match(self) -> None:
        assert vc(">=1.0.0").matches(sv("0.9.9")) is False

    def test_gte_patch_equal_matches(self) -> None:
        assert vc(">=2.3.4").matches(sv("2.3.4")) is True

    def test_gte_patch_greater_matches(self) -> None:
        assert vc(">=2.3.4").matches(sv("2.3.5")) is True

    def test_gte_patch_less_no_match(self) -> None:
        assert vc(">=2.3.4").matches(sv("2.3.3")) is False


# ---------------------------------------------------------------------------
# VersionConstraint.matches — >
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesGT:
    def test_gt_exact_no_match(self) -> None:
        assert vc(">1.0.0").matches(sv("1.0.0")) is False

    def test_gt_greater_matches(self) -> None:
        assert vc(">1.0.0").matches(sv("1.0.1")) is True

    def test_gt_much_greater_matches(self) -> None:
        assert vc(">1.0.0").matches(sv("2.0.0")) is True

    def test_gt_less_no_match(self) -> None:
        assert vc(">1.0.0").matches(sv("0.9.9")) is False


# ---------------------------------------------------------------------------
# VersionConstraint.matches — <=
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesLTE:
    def test_lte_equal_matches(self) -> None:
        assert vc("<=2.0.0").matches(sv("2.0.0")) is True

    def test_lte_less_matches(self) -> None:
        assert vc("<=2.0.0").matches(sv("1.9.9")) is True

    def test_lte_zero_matches(self) -> None:
        assert vc("<=2.0.0").matches(sv("0.0.1")) is True

    def test_lte_greater_no_match(self) -> None:
        assert vc("<=2.0.0").matches(sv("2.0.1")) is False

    def test_lte_much_greater_no_match(self) -> None:
        assert vc("<=2.0.0").matches(sv("3.0.0")) is False


# ---------------------------------------------------------------------------
# VersionConstraint.matches — <
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesLT:
    def test_lt_exact_no_match(self) -> None:
        assert vc("<2.0.0").matches(sv("2.0.0")) is False

    def test_lt_less_matches(self) -> None:
        assert vc("<2.0.0").matches(sv("1.9.9")) is True

    def test_lt_much_less_matches(self) -> None:
        assert vc("<2.0.0").matches(sv("0.1.0")) is True

    def test_lt_greater_no_match(self) -> None:
        assert vc("<2.0.0").matches(sv("2.0.1")) is False


# ---------------------------------------------------------------------------
# VersionConstraint.matches — ^ (caret: compatible range)
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesCaret:
    def test_caret_exact_matches(self) -> None:
        assert vc("^1.0.0").matches(sv("1.0.0")) is True

    def test_caret_same_major_higher_matches(self) -> None:
        assert vc("^1.0.0").matches(sv("1.9.9")) is True

    def test_caret_next_major_no_match(self) -> None:
        assert vc("^1.0.0").matches(sv("2.0.0")) is False

    def test_caret_lower_major_no_match(self) -> None:
        assert vc("^1.0.0").matches(sv("0.9.9")) is False

    def test_caret_minor_bump_matches(self) -> None:
        assert vc("^2.0.0").matches(sv("2.5.0")) is True

    def test_caret_patch_bump_matches(self) -> None:
        assert vc("^1.2.0").matches(sv("1.2.5")) is True


# ---------------------------------------------------------------------------
# VersionConstraint.matches — ~ (tilde: compatible patch)
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesTilde:
    def test_tilde_exact_matches(self) -> None:
        assert vc("~1.2.0").matches(sv("1.2.0")) is True

    def test_tilde_patch_bump_matches(self) -> None:
        assert vc("~1.2.0").matches(sv("1.2.5")) is True

    def test_tilde_minor_bump_no_match(self) -> None:
        assert vc("~1.2.0").matches(sv("1.3.0")) is False

    def test_tilde_lower_patch_no_match(self) -> None:
        assert vc("~1.2.3").matches(sv("1.2.2")) is False


# ---------------------------------------------------------------------------
# VersionConstraint.matches — = (exact)
# ---------------------------------------------------------------------------


class TestVersionConstraintMatchesExact:
    def test_exact_match(self) -> None:
        assert vc("=1.2.3").matches(sv("1.2.3")) is True

    def test_exact_patch_different_no_match(self) -> None:
        assert vc("=1.2.3").matches(sv("1.2.4")) is False

    def test_exact_minor_different_no_match(self) -> None:
        assert vc("=1.2.3").matches(sv("1.3.3")) is False

    def test_exact_major_different_no_match(self) -> None:
        assert vc("=1.2.3").matches(sv("2.2.3")) is False


# ---------------------------------------------------------------------------
# SemVer comparisons and string representation
# ---------------------------------------------------------------------------


class TestSemVerComparisons:
    def test_equal(self) -> None:
        assert sv("1.2.3") == sv("1.2.3")

    def test_not_equal(self) -> None:
        assert sv("1.2.3") != sv("1.2.4")

    def test_less_than_major(self) -> None:
        assert sv("1.0.0") < sv("2.0.0")

    def test_less_than_minor(self) -> None:
        assert sv("1.2.0") < sv("1.3.0")

    def test_less_than_patch(self) -> None:
        assert sv("1.2.3") < sv("1.2.4")

    def test_greater_than(self) -> None:
        assert sv("2.0.0") > sv("1.9.9")

    def test_lte_equal(self) -> None:
        assert sv("1.0.0") <= sv("1.0.0")

    def test_lte_less(self) -> None:
        assert sv("0.9.9") <= sv("1.0.0")

    def test_gte_equal(self) -> None:
        assert sv("2.0.0") >= sv("2.0.0")

    def test_gte_greater(self) -> None:
        assert sv("3.0.0") >= sv("2.0.0")


class TestSemVerFields:
    def test_major(self) -> None:
        v = sv("3.5.7")
        assert v.major == 3

    def test_minor(self) -> None:
        v = sv("3.5.7")
        assert v.minor == 5

    def test_patch(self) -> None:
        v = sv("3.5.7")
        assert v.patch == 7

    def test_zero_version(self) -> None:
        v = sv("0.0.0")
        assert v.major == 0
        assert v.minor == 0
        assert v.patch == 0

    def test_to_string(self) -> None:
        v = sv("1.2.3")
        assert str(v) == "1.2.3"

    def test_to_string_zero(self) -> None:
        v = sv("0.0.0")
        assert str(v) == "0.0.0"

    def test_repr_format(self) -> None:
        v = sv("4.5.6")
        r = repr(v)
        assert "4" in r and "5" in r and "6" in r

    def test_sorting(self) -> None:
        versions = [sv("1.9.0"), sv("0.1.0"), sv("2.0.0"), sv("1.0.0")]
        sorted_v = sorted(versions)
        assert sorted_v[0].major == 0
        assert sorted_v[-1].major == 2


# ---------------------------------------------------------------------------
# ToolRegistry.register_batch edge cases
# ---------------------------------------------------------------------------


class TestRegisterBatchEdgeCases:
    def test_empty_batch_no_op(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch([])
        assert reg.count_actions() == 0

    def test_minimal_fields(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch([{"name": "op", "category": "c"}])
        assert reg.count_actions() == 1

    def test_single_item_batch(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch([{"name": "sphere", "category": "geo", "dcc": "maya"}])
        assert reg.count_actions() == 1

    def test_batch_with_tags(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "op1", "category": "c", "dcc": "maya", "tags": ["a", "b"]},
            ]
        )
        tags = reg.get_tags(dcc_name="maya")
        assert "a" in tags
        assert "b" in tags

    def test_same_name_multiple_dccs(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "sphere", "category": "geo", "dcc": "maya"},
                {"name": "sphere", "category": "geo", "dcc": "blender"},
                {"name": "sphere", "category": "geo", "dcc": "houdini"},
            ]
        )
        # count_actions counts unique names, not (name, dcc) pairs
        assert reg.count_actions() == 1
        # But DCC-scoped counts work
        assert reg.count_actions(dcc_name="maya") == 1
        assert reg.count_actions(dcc_name="blender") == 1
        assert reg.count_actions(dcc_name="houdini") == 1

    def test_same_name_multiple_dccs_get_all_dccs(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "op", "category": "c", "dcc": "maya"},
                {"name": "op", "category": "c", "dcc": "blender"},
            ]
        )
        dccs = reg.get_all_dccs()
        assert "maya" in dccs
        assert "blender" in dccs

    def test_different_names_same_dcc(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "create", "category": "geo", "dcc": "maya"},
                {"name": "delete", "category": "geo", "dcc": "maya"},
                {"name": "modify", "category": "geo", "dcc": "maya"},
            ]
        )
        assert reg.count_actions(dcc_name="maya") == 3

    def test_batch_then_register_extends(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "op1", "category": "c", "dcc": "maya"},
                {"name": "op2", "category": "c", "dcc": "maya"},
            ]
        )
        reg.register("op3", description="d", category="c", dcc="maya")
        assert reg.count_actions(dcc_name="maya") == 3

    def test_batch_large(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        batch = [{"name": f"op_{i}", "category": "misc", "dcc": "maya"} for i in range(50)]
        reg.register_batch(batch)
        assert reg.count_actions(dcc_name="maya") == 50

    def test_batch_multiple_categories(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "create_sphere", "category": "geometry", "dcc": "maya"},
                {"name": "render_scene", "category": "render", "dcc": "maya"},
                {"name": "animate_joint", "category": "animation", "dcc": "maya"},
            ]
        )
        cats = reg.get_categories()
        assert "geometry" in cats
        assert "render" in cats
        assert "animation" in cats

    def test_batch_after_reset(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("initial", description="d", category="c", dcc="maya")
        reg.reset()
        assert reg.count_actions() == 0
        reg.register_batch([{"name": "new_op", "category": "c", "dcc": "maya"}])
        assert reg.count_actions() == 1

    def test_batch_search_by_tag(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "op_tagged", "category": "c", "dcc": "maya", "tags": ["special"]},
                {"name": "op_plain", "category": "c", "dcc": "maya"},
            ]
        )
        results = reg.search_actions(tags=["special"])
        names = [r["name"] for r in results]
        assert "op_tagged" in names
        assert "op_plain" not in names

    def test_batch_cross_dcc_categories(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "geo_op", "category": "geometry", "dcc": "maya"},
                {"name": "geo_op", "category": "geometry", "dcc": "blender"},
                {"name": "anim_op", "category": "animation", "dcc": "maya"},
            ]
        )
        assert reg.get_categories(dcc_name="maya") == sorted(["animation", "geometry"])
        assert reg.get_categories(dcc_name="blender") == ["geometry"]
