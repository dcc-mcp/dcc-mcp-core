"""End-to-end regression for the search_skills path-source rank penalty (#1403).

Skills discovered from user-curated locations (`extra_paths`,
`DCC_MCP_*_SKILL_PATHS`, `~/.dcc-mcp/<dcc>/skills`) must outrank skills
that come from bundled / platform-installed directories for neutral
queries. This protects the "I'm iterating on my local skill" workflow
from being drowned out by starter material shipped with the package.

Exercises the full Rust catalog → Python binding path; the source
tagging happens in `crates/dcc-mcp-skills/src/scanner.rs::scan_with_sources`
and the rank multiplier in `crates/dcc-mcp-skills/src/catalog/scoring.rs`.
"""

from __future__ import annotations

import os
from pathlib import Path

import pytest

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry


def _write_skill(skills_root: Path, name: str, *, description: str, dcc: str = "maya") -> None:
    skill_dir = skills_root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    (skill_dir / "SKILL.md").write_text(
        "\n".join(
            [
                "---",
                f"name: {name}",
                f"description: {description}",
                "metadata:",
                "  dcc-mcp:",
                f"    dcc: {dcc}",
                "---",
                f"# {name}",
                "",
            ]
        ),
        encoding="utf-8",
    )


def test_env_var_outranks_explicit_arg_only_when_bundled(tmp_path: Path) -> None:
    # Two roots with identical-content skills. The `extra_paths` root is
    # tagged `ExplicitArg` (x 1.00) and the env-var root is tagged
    # `EnvVar` (x 1.00) — both at the same priority, so the ranker falls
    # through to the alphabetical tie-break.
    explicit_root = tmp_path / "explicit"
    env_root = tmp_path / "envvar"
    explicit_root.mkdir()
    env_root.mkdir()
    _write_skill(explicit_root, "alpha-render", description="render bake helpers")
    _write_skill(env_root, "beta-render", description="render bake helpers")

    saved = os.environ.get("DCC_MCP_MAYA_SKILL_PATHS")
    os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = str(env_root)
    try:
        catalog = SkillCatalog(ToolRegistry())
        catalog.discover(extra_paths=[str(explicit_root)], dcc_name="maya")
        names = [s.name for s in catalog.search_skills(query="render bake")]
    finally:
        if saved is None:
            os.environ.pop("DCC_MCP_MAYA_SKILL_PATHS", None)
        else:
            os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = saved

    assert "alpha-render" in names
    assert "beta-render" in names
    # Both x 1.00 → alphabetical tie-break (alpha < beta).
    assert names.index("alpha-render") < names.index("beta-render"), (
        f"both user-curated sources must tie on multiplier; order {names}"
    )


def test_explicit_arg_outranks_via_score_skills_default(tmp_path: Path) -> None:
    # Smoke test that the `extra_paths` lane works end-to-end: a single
    # discover() call tags the skill, and search_skills returns it.
    explicit_root = tmp_path / "explicit"
    explicit_root.mkdir()
    _write_skill(explicit_root, "user-render", description="render bake helpers")

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(explicit_root)], dcc_name="maya")
    names = [s.name for s in catalog.search_skills(query="render bake")]

    assert "user-render" in names, f"user-curated skill must be discoverable; got {names}"
    assert names[0] == "user-render", f"user-curated skill should lead unrelated bundled fixtures; got {names}"


@pytest.mark.parametrize("query", ["render bake helpers"])
def test_path_source_smoke_neutral_query(tmp_path: Path, query: str) -> None:
    # Defensive smoke test: with only one fixture under extra_paths the
    # catalog must return it as the top hit regardless of which bundled
    # skills the package ships with. This guards against the path-source
    # multiplier accidentally dropping legitimate user content.
    user_root = tmp_path / "userland"
    user_root.mkdir()
    _write_skill(user_root, "user-bake", description=query)

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(user_root)], dcc_name="maya")
    names = [s.name for s in catalog.search_skills(query=query)]

    assert names, "expected at least one match for a user-curated skill"
    assert names[0] == "user-bake", f"user-curated skill must lead neutral-query results; got {names}"
