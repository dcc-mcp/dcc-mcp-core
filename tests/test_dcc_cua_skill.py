"""Contract tests for the dedicated project DCC-CUA routing Skill."""

from __future__ import annotations

from pathlib import Path

from conftest import REPO_ROOT
import dcc_mcp_core

DCC_CUA_SKILL_DIR = REPO_ROOT / "skills" / "dcc-cua"


def _normalized(path: Path) -> str:
    return " ".join(path.read_text(encoding="utf-8").lower().split())


class TestDccCuaSkill:
    def test_skill_is_valid_and_scannable(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(str(DCC_CUA_SKILL_DIR))
        assert meta is not None
        assert meta.name == "dcc-cua"
        assert meta.version == "0.1.0"

        report = dcc_mcp_core.validate_skill(str(DCC_CUA_SKILL_DIR))
        assert report.is_clean, report.issues

        scanner = dcc_mcp_core.SkillScanner()
        names = {Path(path).name for path in scanner.scan(extra_paths=[str(DCC_CUA_SKILL_DIR.parent)])}
        assert "dcc-cua" in names

    def test_named_requests_have_a_fail_closed_provider_boundary(self) -> None:
        meta = dcc_mcp_core.parse_skill_md(str(DCC_CUA_SKILL_DIR))
        assert meta is not None
        description = " ".join((meta.description or "").lower().split())
        for phrase in (
            "dcc-cua",
            "dcc cua",
            "our dcc-cua",
            "我们的 dcc-cua",
            "hard route",
            "take precedence over generic codex/openai computer use",
            "never silently fall back",
        ):
            assert phrase in description

        body = _normalized(DCC_CUA_SKILL_DIR / "SKILL.md")
        for phrase in (
            "non-substitution contract",
            "hard routing boundary",
            "the `computer-use` skill",
            "`@oai/sky`",
            "browser/chrome automation plugins",
            "never treat a dcc-cua runtime, binding, readiness, or permission failure",
            "explicitly retracts the dcc-cua requirement",
            "do not mention a generic computer use fallback unless the user asks for one",
        ):
            assert phrase in body

    def test_exact_name_wins_catalog_search(self) -> None:
        catalog = dcc_mcp_core.SkillCatalog(dcc_mcp_core.ToolRegistry())
        catalog.discover(extra_paths=[str(DCC_CUA_SKILL_DIR.parent)])

        for query in ("dcc-cua", "our dcc-cua", "我们的 dcc-cua"):
            results = catalog.search_skills(query)
            assert results, query
            assert results[0].name == "dcc-cua", (query, [result.name for result in results])

    def test_browser_route_stays_inside_dcc_cua(self) -> None:
        body = _normalized(DCC_CUA_SKILL_DIR / "SKILL.md")
        for phrase in (
            "browser work remains inside dcc-cua",
            "typed browser surface",
            "`browser_dom`",
            "not an in-app browser or chrome plugin",
            "bind the exact browser pid and window handle",
            "keep connection-scoped sessions and capabilities on one host connection",
        ):
            assert phrase in body

    def test_openai_interface_repeats_non_substitution_contract(self) -> None:
        interface = dcc_mcp_core.yaml_loads((DCC_CUA_SKILL_DIR / "agents" / "openai.yaml").read_text(encoding="utf-8"))[
            "interface"
        ]
        assert interface["display_name"] == "DCC-CUA"
        prompt = interface["default_prompt"].lower()
        for phrase in (
            "project-owned dcc-cua",
            "exact target",
            "verify the final state",
            "never substitute generic codex computer use",
            "computer-use",
            "@oai/sky",
            "browser/chrome plugins",
        ):
            assert phrase in prompt

    def test_skill_stays_compact(self) -> None:
        assert len((DCC_CUA_SKILL_DIR / "SKILL.md").read_text(encoding="utf-8").splitlines()) <= 220
