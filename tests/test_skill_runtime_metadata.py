"""Regression tests for optional skill runtime metadata."""

from __future__ import annotations

from pathlib import Path

from dcc_mcp_core import SkillCatalog
from dcc_mcp_core import SkillRuntimeSummary
from dcc_mcp_core import ToolRegistry


def test_skill_runtime_metadata_surfaces_before_loading(tmp_path: Path) -> None:
    skill_dir = tmp_path / "openusd-runtime"
    skill_dir.mkdir()
    (skill_dir / "scripts").mkdir()
    (skill_dir / "SKILL.md").write_text(
        "\n".join(
            [
                "---",
                "name: openusd-runtime",
                "description: OpenUSD runtime probe skill",
                "metadata:",
                "  dcc-mcp:",
                "    dcc: python",
                "    tags: [usd]",
                "    runtimes:",
                "      - name: usd-core",
                "        type: python_package",
                "        package: usd-core",
                "        module: dcc_mcp_runtime_probe_missing_pxr_1210",
                "        optional: true",
                "        feature_level: full-usd",
                "        install_hint: pip install dcc-mcp-openusd[usd-core]",
                "---",
                "# OpenUSD Runtime",
            ]
        ),
        encoding="utf-8",
    )

    catalog = SkillCatalog(ToolRegistry())
    assert catalog.discover(extra_paths=[str(tmp_path)], dcc_name="python") == 1

    summaries = catalog.search_skills(query="openusd", dcc="python")
    assert len(summaries) == 1
    runtime = summaries[0].runtime
    assert isinstance(runtime, SkillRuntimeSummary)
    assert runtime.total == 1
    assert runtime.degraded == 1

    detail = catalog.get_skill_info("openusd-runtime")
    assert detail is not None
    assert detail["runtime"]["state"] == "degraded"
    assert detail["runtimes"][0]["name"] == "usd-core"
    assert detail["runtimes"][0]["state"] == "degraded"
