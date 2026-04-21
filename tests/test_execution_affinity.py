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


def _write_skill(base: Path, name: str, frontmatter_body: str) -> Path:
    """Write a SKILL.md with the given frontmatter body under base/name."""
    skill_dir = base / name
    skill_dir.mkdir(parents=True, exist_ok=True)
    content = f"---\nname: {name}\ndcc: python\n{frontmatter_body}---\n# {name}\n"
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    return skill_dir


class TestSkillMdExecution:
    def test_execution_async_parses(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "render-farm",
            "tools:\n  - name: render_frames\n    execution: async\n    timeout_hint_secs: 600\n",
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
            "tools:\n  - name: do_thing\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None
        assert meta.tools[0].execution == "sync"
        assert meta.tools[0].timeout_hint_secs is None

    def test_deferred_user_flag_rejected(self, tmp_path: Path) -> None:
        """`deferred: true` at the user level must be rejected (server-set only)."""
        skill_dir = _write_skill(
            tmp_path,
            "bad-skill",
            "tools:\n  - name: x\n    deferred: true\n",
        )
        # parse_skill_md swallows YAML errors and returns None — the point
        # is that the skill does NOT load with a silent success.
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is None, "deferred: true must cause a load failure"

    def test_unknown_execution_value_rejected(self, tmp_path: Path) -> None:
        skill_dir = _write_skill(
            tmp_path,
            "bad-skill",
            "tools:\n  - name: x\n    execution: background\n",
        )
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is None

    def test_backward_compat_pre_change_skill_md(self, tmp_path: Path) -> None:
        """Existing SKILL.md files (no `execution` field) still load."""
        skill_dir = _write_skill(
            tmp_path,
            "legacy-skill",
            "tools:\n  - name: legacy_tool\n    description: old style\n",
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
