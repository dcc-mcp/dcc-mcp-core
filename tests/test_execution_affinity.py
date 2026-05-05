"""Issue #317 — execution / timeout_hint_secs on ToolDeclaration & Action.

Covers:
- SKILL.md with `execution: async` parses into ToolDeclaration.execution == "async"
- Absence of `execution` defaults to "sync"
- SKILL.md with `deferred: true` at the user level is rejected
- Unknown `execution` values are rejected
- ActionRegistry.register accepts `execution` / `timeout_hint_secs`
- Backward compat: pre-change SKILL.md files still load
"""

from __future__ import annotations

from pathlib import Path

import pytest

import dcc_mcp_core


def _write_skill(base: Path, name: str, tools_yaml_body: str) -> Path:
    """Write a SKILL.md + sibling tools.yaml under base/name and return the skill dir.

    ``tools_yaml_body`` is the YAML content (excluding the top-level ``tools:`` key)
    that will be placed inside ``tools.yaml`` and referenced via the
    ``metadata.dcc-mcp.tools`` sibling-file pointer (issue #356).
    """
    skill_dir = base / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    skill_md = f"---\nname: {name}\nmetadata:\n  dcc-mcp:\n    dcc: python\n    tools: tools.yaml\n---\n# {name}\n"
    (skill_dir / "SKILL.md").write_text(skill_md, encoding="utf-8")
    (skill_dir / "tools.yaml").write_text("tools:\n" + tools_yaml_body, encoding="utf-8")
    return skill_dir


class TestSkillMdExecution:
    def test_execution_async_parses(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "render-farm",
            "  - name: render_frames\n    execution: async\n    timeout_hint_secs: 600\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert len(meta.tools) == 1
        assert meta.tools[0].execution == "async"
        assert meta.tools[0].timeout_hint_secs == 600

    def test_execution_defaults_to_sync(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "quick-skill",
            "  - name: do_thing\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert meta.tools[0].execution == "sync"
        assert meta.tools[0].timeout_hint_secs is None

    def test_deferred_user_flag_rejected(self, tmp_path: Path) -> None:
        """`deferred: true` at the user level must be rejected (server-set only).

        With sibling tools.yaml the skill itself still parses, but the
        tools list is empty because every tool entry fails validation.
        """
        skill_dir = _write_skill(
            tmp_path,
            "bad-skill",
            "  - name: x\n    deferred: true\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert meta.tools == [], "deferred: true must not yield a tool declaration"

    def test_unknown_execution_value_rejected(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "bad-skill",
            "  - name: x\n    execution: background\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert meta.tools == [], "unknown execution value must not yield a tool declaration"

    def test_backward_compat_pre_change_skill_md(self, tmp_path: Path) -> None:
        """Existing SKILL.md files (no `execution` field) still load."""
        skill_dir = _write_skill(
            tmp_path,
            "legacy-skill",
            "  - name: legacy_tool\n    description: old style\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert meta.tools[0].execution == "sync"
        assert meta.tools[0].timeout_hint_secs is None


class TestToolDeclarationPython:
    def test_python_constructor_accepts_execution(self) -> None:
        td = dcc_mcp_core.ToolDeclaration(
            name="render",
            execution="async",
            timeout_hint_secs=120,
        )
        assert td.execution == "async"
        assert td.timeout_hint_secs == 120

    def test_python_constructor_rejects_bad_execution(self) -> None:
        with pytest.raises(ValueError, match=r"sync.*async"):
            dcc_mcp_core.ToolDeclaration(name="bad", execution="maybe")

    def test_setter_validates(self) -> None:
        td = dcc_mcp_core.ToolDeclaration(name="t")
        td.execution = "async"
        assert td.execution == "async"
        with pytest.raises(ValueError):
            td.execution = "background"


class TestToolRegistryExecution:
    def test_register_accepts_execution(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register(
            name="render_frames",
            description="Async render",
            execution="async",
            timeout_hint_secs=600,
        )
        action = reg.get_action("render_frames")
        assert action is not None
        assert action["execution"] == "async"
        assert action["timeout_hint_secs"] == 600

    def test_register_defaults(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register(name="quick")
        action = reg.get_action("quick")
        assert action is not None
        assert action["execution"] == "sync"
        # timeout_hint_secs omitted when None (serde skip_serializing_if)
        assert action.get("timeout_hint_secs") in (None, 0, False)

    def test_register_rejects_bad_execution(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        with pytest.raises(ValueError):
            reg.register(name="bad", execution="background")


class TestAsyncRenderExampleSkill:
    """The bundled example skill under examples/skills/async-render-example/
    must declare execution: async + timeout_hint_secs and parse cleanly.
    """

    def test_example_skill_loads(self) -> None:
        root = Path(__file__).resolve().parents[1]
        skill_dir = root / "examples" / "skills" / "async-render-example"
        assert skill_dir.is_dir(), f"example skill missing: {skill_dir}"
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        by_name = {t.name: t for t in meta.tools}
        assert "render_frames" in by_name
        assert by_name["render_frames"].execution == "async"
        assert by_name["render_frames"].timeout_hint_secs == 600
        assert by_name["quick_status"].execution == "sync"
