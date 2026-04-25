"""Integration tests for the BM25-lite ranker powering ``search_skills``.

Issue #343 — `SkillCatalog.search_skills` (the function backing the
``search_skills`` MCP tool) now tokenises the query, drops stopwords,
weights matches across skill-level fields AND sibling ``tools.yaml``
entries, and sorts results deterministically.

These tests exercise a synthetic fixture skill set and assert the
resulting ORDER of results.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry

# ---------------------------------------------------------------------------
# Fixture builder
# ---------------------------------------------------------------------------


def _write_skill(
    root: Path,
    name: str,
    *,
    description: str = "",
    tags: list[str] | None = None,
    dcc: str = "maya",
    search_hint: str = "",
    tools_yaml: str | None = None,
) -> None:
    """Create a SKILL.md directory (with optional sibling tools.yaml)."""
    skill_dir = root / name
    skill_dir.mkdir(parents=True, exist_ok=True)

    tags_block = "tags: []\n" if not tags else "tags:\n" + "".join(f"  - {t}\n" for t in tags)
    hint_line = f'search-hint: "{search_hint}"\n' if search_hint else ""
    tools_meta = (
        "metadata:\n  dcc-mcp.dcc: " + dcc + "\n  dcc-mcp.tools: tools.yaml\n" if tools_yaml is not None else ""
    )
    body = (
        f"---\n"
        f"name: {name}\n"
        f"version: 1.0.0\n"
        f'description: "{description}"\n'
        f"dcc: {dcc}\n"
        f"{tags_block}"
        f"{hint_line}"
        f"{tools_meta}"
        f"---\n\n# {name}\n"
    )
    (skill_dir / "SKILL.md").write_text(body, encoding="utf-8")

    if tools_yaml is not None:
        (skill_dir / "tools.yaml").write_text(tools_yaml, encoding="utf-8")


@pytest.fixture
def catalog(tmp_path: Path) -> SkillCatalog:
    """Build a catalog populated with a deliberately ambiguous skill set.

    Skills are designed so that several naive matchers would produce
    different orderings; the BM25-lite scorer must still produce the
    deterministic expected order for the assertions below.
    """
    # 1. skill whose NAME is exactly "polygon-bevel" — should win on
    #    exact-name fast path when queried by its name.
    _write_skill(
        tmp_path,
        "polygon-bevel",
        description="Bevels polygon edges cleanly.",
        tags=["modeling", "polygon"],
        dcc="maya",
        search_hint="polygon bevel edge chamfer",
    )

    # 2. description-only mention of polygon, with no tool, no hint.
    _write_skill(
        tmp_path,
        "misc-utils",
        description="Miscellaneous utilities for polygon cleanup.",
        tags=["utility"],
        dcc="maya",
    )

    # 3. skill whose SKILL.md does NOT mention "turntable" at all, but
    #    whose sibling tools.yaml declares a `turntable` tool — sibling
    #    expansion must make it scorable.
    _write_skill(
        tmp_path,
        "camera-helpers",
        description="Helpers for cinematic shots.",
        tags=["camera"],
        dcc="maya",
        tools_yaml=(
            "tools:\n"
            "  - name: turntable\n"
            "    description: Create a turntable camera rig.\n"
            "  - name: dolly_in\n"
            "    description: Animate a simple dolly-in move.\n"
        ),
    )

    # 4. blender skill — used to exercise the dcc filter + scope.
    _write_skill(
        tmp_path,
        "render-utils",
        description="Rendering helpers for Blender.",
        tags=["render"],
        dcc="blender",
    )

    reg = ToolRegistry()
    cat = SkillCatalog(reg)
    discovered = cat.discover(extra_paths=[str(tmp_path)])
    assert discovered >= 4, f"expected >=4 skills, got {discovered}"
    return cat


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_exact_name_ranks_first(catalog: SkillCatalog) -> None:
    """Querying the exact skill name places it first, regardless of
    other skills with more matching fields.
    """
    results = catalog.search_skills(query="polygon-bevel")
    assert results, "expected at least one result"
    assert results[0].name == "polygon-bevel"


def test_sibling_tools_yaml_contributes_to_score(catalog: SkillCatalog) -> None:
    """A skill whose only mention of the query is in its sibling
    tools.yaml must still appear in search results (#343 + #356).
    """
    results = catalog.search_skills(query="turntable")
    names = [r.name for r in results]
    assert "camera-helpers" in names, f"sibling tools.yaml entry must make skill scorable, got {names}"
    assert results[0].name == "camera-helpers"


def test_multi_token_query_prefers_skill_matching_all_tokens(
    catalog: SkillCatalog,
) -> None:
    """A multi-token query favours the skill that hits on more tokens."""
    results = catalog.search_skills(query="polygon bevel")
    assert results
    # `polygon-bevel` has both tokens in name + tags + hint; `misc-utils`
    # only has `polygon` in description.
    assert results[0].name == "polygon-bevel"


def test_stopword_only_query_returns_nothing(catalog: SkillCatalog) -> None:
    """`the of and` contains only stopwords — no skill should rank."""
    results = catalog.search_skills(query="the of and")
    assert results == [], "stopword-only query must return no results"


def test_dcc_filter_applied_before_scoring(catalog: SkillCatalog) -> None:
    """The `dcc` pre-filter excludes skills for other DCCs before the
    scorer runs — a maya-only query must not surface blender skills.
    """
    results = catalog.search_skills(query="render", dcc="maya")
    names = [r.name for r in results]
    assert "render-utils" not in names, f"blender skill must be filtered out, got {names}"
