"""End-to-end regression for the search_skills layer-based rank penalty (#1398).

`infrastructure` and `example` layered skills must rank below `domain` /
unset layered skills for a neutral query, unless the caller explicitly
filters by a layer tag (`tags=["infrastructure"]`), in which case the raw
BM25 order is honoured inside the filtered slice.

Exercises the full Rust catalog → Python binding path; layer detection
lives in `crates/dcc-mcp-skills/src/catalog/catalog.rs` and the penalty in
`crates/dcc-mcp-skills/src/catalog/scoring.rs`.
"""

from __future__ import annotations

from pathlib import Path
from typing import Sequence

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import ToolRegistry


def _write_skill(
    skills_root: Path,
    name: str,
    *,
    description: str,
    layer: str | None,
    dcc: str = "maya",
    tags: Sequence[str] = (),
) -> None:
    skill_dir = skills_root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "scripts").mkdir()
    lines = [
        "---",
        f"name: {name}",
        f"description: {description}",
        "metadata:",
        "  dcc-mcp:",
        f"    dcc: {dcc}",
    ]
    if tags:
        rendered = ", ".join(tags)
        lines.append(f"    tags: [{rendered}]")
    if layer:
        lines.append(f"    layer: {layer}")
    lines += ["---", f"# {name}", ""]
    (skill_dir / "SKILL.md").write_text("\n".join(lines), encoding="utf-8")


def _build_catalog(tmp_path: Path) -> SkillCatalog:
    # Mirrors the screenshot scenario: a domain skill, two infrastructure
    # skills, and one example. All share the query tokens "render bake" so
    # raw BM25 would tie them — only the layer multiplier should break the
    # tie.
    _write_skill(
        tmp_path,
        "maya-render",
        description="render bake helpers for maya",
        layer="domain",
    )
    _write_skill(
        tmp_path,
        "dcc-diagnostics",
        description="render bake helpers diagnostics",
        layer="infrastructure",
    )
    _write_skill(
        tmp_path,
        "dcc-adapter",
        description="render bake helpers adapter",
        layer="infrastructure",
    )
    _write_skill(
        tmp_path,
        "demo-render",
        description="render bake helpers tutorial",
        layer="example",
        # Tag the demo so the explicit-filter test below can prefilter by
        # `tags=["example"]` and verify the exclusion bypass.
        tags=("example",),
    )
    catalog = SkillCatalog(ToolRegistry())
    # `discover()` also picks up bundled / env-configured skill paths, so
    # the total count can exceed the four fixtures written above. The
    # important assertion is that our four are all present.
    catalog.discover(extra_paths=[str(tmp_path)], dcc_name="maya")
    discovered_names = {s.name for s in catalog.search_skills()}
    for expected in ("maya-render", "dcc-diagnostics", "dcc-adapter", "demo-render"):
        assert expected in discovered_names, (
            f"fixture skill {expected!r} not discovered; got {sorted(discovered_names)}"
        )
    return catalog


def test_domain_ranks_above_infrastructure_for_neutral_query(tmp_path: Path) -> None:
    catalog = _build_catalog(tmp_path)
    summaries = catalog.search_skills(query="render bake")
    names = [s.name for s in summaries]
    assert names, "expected at least one match"
    assert names[0] == "maya-render", (
        f"domain skill must outrank infrastructure/example for a neutral query; got order {names}"
    )
    domain_idx = names.index("maya-render")
    for infra_name in ("dcc-diagnostics", "dcc-adapter"):
        assert names.index(infra_name) > domain_idx, (
            f"infrastructure skill {infra_name!r} ranked above domain; order was {names}"
        )


def test_example_excluded_from_unfiltered_results(tmp_path: Path) -> None:
    # `example` skills are dropped from search results entirely (#1398) —
    # they exist as authoring references, not for production agent flows.
    catalog = _build_catalog(tmp_path)
    names = [s.name for s in catalog.search_skills(query="render bake")]
    assert "demo-render" not in names, f"example layer must be excluded from unfiltered results; order {names}"


def test_example_visible_under_explicit_filter(tmp_path: Path) -> None:
    # `tags=["example"]` is the operator opting in to browse example skills;
    # the exclusion is lifted inside the filtered slice.
    catalog = _build_catalog(tmp_path)
    summaries = catalog.search_skills(query="render bake", tags=["example"])
    names = [s.name for s in summaries]
    assert "demo-render" in names, f"explicit example filter must restore demo skills; got {names}"


def test_explicit_infrastructure_filter_disables_penalty(tmp_path: Path) -> None:
    # When the agent explicitly asks for infrastructure skills, the
    # filtered slice is returned in raw BM25 order (no penalty).
    # The penalty bypass is keyed on the *tag* parameter matching a known
    # layer name (case-insensitive), so both skills must also carry the
    # "infrastructure" tag for the prefilter to keep them.
    _write_skill(
        tmp_path,
        "infra-strong",
        description="render bake render bake helpers diagnostics",
        layer="infrastructure",
        tags=("infrastructure",),
    )
    _write_skill(
        tmp_path,
        "infra-weak",
        description="render helpers",
        layer="infrastructure",
        tags=("infrastructure",),
    )

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(tmp_path)], dcc_name="maya")
    summaries = catalog.search_skills(query="render bake", tags=["infrastructure"])
    names = [s.name for s in summaries]
    assert names, "expected matches inside the explicit-infrastructure filter"
    assert names[0] == "infra-strong", f"explicit infrastructure filter must rank by raw BM25; got {names}"
