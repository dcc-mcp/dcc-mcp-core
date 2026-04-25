"""Tests for the recipes system (issue #428)."""

from __future__ import annotations

import json
from pathlib import Path
import textwrap
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core.recipes import get_recipe_content
from dcc_mcp_core.recipes import get_recipes_path
from dcc_mcp_core.recipes import parse_recipe_anchors
from dcc_mcp_core.recipes import register_recipes_tools

# ── Fixtures ──────────────────────────────────────────────────────────────


@pytest.fixture()
def recipes_md(tmp_path: Path) -> Path:
    """Write a sample RECIPES.md and return its path."""
    content = textwrap.dedent(
        """\
        # Maya Recipes

        ## create_polygon_cube

        Create a named polygon cube at the origin.

        ```python
        cube = cmds.polyCube(name="myCube", w=1, h=1, d=1)[0]
        ```

        ## set_world_translation

        Set absolute world-space translation.

        ```python
        cmds.xform("myCube", translation=(1, 2, 3), worldSpace=True)
        ```

        ## delete_node

        Delete a named node safely.

        ```python
        if cmds.objExists("myCube"):
            cmds.delete("myCube")
        ```
        """
    )
    p = tmp_path / "RECIPES.md"
    p.write_text(content, encoding="utf-8")
    return p


def _make_metadata(skill_path: str | None, recipes_rel: str | None, *, nested: bool = False) -> MagicMock:
    """Build a minimal SkillMetadata mock."""
    md = MagicMock()
    md.skill_path = skill_path
    if recipes_rel is None:
        md.metadata = {}
    elif nested:
        md.metadata = {"dcc-mcp": {"recipes": recipes_rel}}
    else:
        md.metadata = {"dcc-mcp.recipes": recipes_rel}
    return md


# ── get_recipes_path ──────────────────────────────────────────────────────


class TestGetRecipesPath:
    def test_flat_form_with_skill_path(self, tmp_path: Path) -> None:
        skill_dir = tmp_path / "my-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), "references/RECIPES.md", nested=False)
        result = get_recipes_path(md)
        assert result == str(skill_dir / "references/RECIPES.md")

    def test_nested_form_with_skill_path(self, tmp_path: Path) -> None:
        skill_dir = tmp_path / "my-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), "RECIPES.md", nested=True)
        result = get_recipes_path(md)
        assert result == str(skill_dir / "RECIPES.md")

    def test_no_recipes_key_returns_none(self) -> None:
        md = _make_metadata("/some/path", None)
        assert get_recipes_path(md) is None

    def test_absolute_path_not_joined(self, tmp_path: Path) -> None:
        abs_path = str(tmp_path / "RECIPES.md")
        md = _make_metadata("/some/skill", abs_path, nested=False)
        result = get_recipes_path(md)
        assert result == abs_path

    def test_no_skill_path_returns_relative(self) -> None:
        md = _make_metadata(None, "references/RECIPES.md", nested=False)
        result = get_recipes_path(md)
        assert result == "references/RECIPES.md"

    def test_empty_metadata_returns_none(self) -> None:
        md = MagicMock()
        md.metadata = None
        md.skill_path = None
        assert get_recipes_path(md) is None


# ── parse_recipe_anchors ──────────────────────────────────────────────────


class TestParseRecipeAnchors:
    def test_returns_three_anchors(self, recipes_md: Path) -> None:
        anchors = parse_recipe_anchors(str(recipes_md))
        assert anchors == ["create_polygon_cube", "set_world_translation", "delete_node"]

    def test_missing_file_returns_empty(self, tmp_path: Path) -> None:
        result = parse_recipe_anchors(str(tmp_path / "nonexistent.md"))
        assert result == []

    def test_file_with_no_h2_headings(self, tmp_path: Path) -> None:
        p = tmp_path / "RECIPES.md"
        p.write_text("# Title\n\nSome text with # hash but no ## heading.\n", encoding="utf-8")
        assert parse_recipe_anchors(str(p)) == []

    def test_ignores_h1_headings(self, recipes_md: Path) -> None:
        anchors = parse_recipe_anchors(str(recipes_md))
        assert "Maya Recipes" not in anchors

    def test_preserves_order(self, tmp_path: Path) -> None:
        content = "## beta\n\ncontent\n\n## alpha\n\ncontent\n"
        p = tmp_path / "RECIPES.md"
        p.write_text(content, encoding="utf-8")
        assert parse_recipe_anchors(str(p)) == ["beta", "alpha"]


# ── get_recipe_content ────────────────────────────────────────────────────


class TestGetRecipeContent:
    def test_returns_first_section(self, recipes_md: Path) -> None:
        content = get_recipe_content(str(recipes_md), "create_polygon_cube")
        assert content is not None
        assert "## create_polygon_cube" in content
        assert "polyCube" in content
        assert "## set_world_translation" not in content

    def test_returns_middle_section(self, recipes_md: Path) -> None:
        content = get_recipe_content(str(recipes_md), "set_world_translation")
        assert content is not None
        assert "xform" in content
        assert "polyCube" not in content
        assert "cmds.delete" not in content

    def test_returns_last_section(self, recipes_md: Path) -> None:
        content = get_recipe_content(str(recipes_md), "delete_node")
        assert content is not None
        assert "cmds.delete" in content

    def test_unknown_anchor_returns_none(self, recipes_md: Path) -> None:
        assert get_recipe_content(str(recipes_md), "no_such_anchor") is None

    def test_missing_file_returns_none(self, tmp_path: Path) -> None:
        assert get_recipe_content(str(tmp_path / "missing.md"), "foo") is None

    def test_content_stripped_of_trailing_whitespace(self, tmp_path: Path) -> None:
        content = "## foo\n\nsome code\n\n\n"
        p = tmp_path / "RECIPES.md"
        p.write_text(content, encoding="utf-8")
        result = get_recipe_content(str(p), "foo")
        assert result is not None
        assert not result.endswith("\n")


# ── register_recipes_tools ────────────────────────────────────────────────


class TestRegisterRecipesTools:
    def _make_server(self, skill_metas: list[MagicMock]) -> tuple[MagicMock, dict]:
        """Return (server_mock, handler_registry)."""
        server = MagicMock()
        registry = MagicMock()
        server.registry = registry
        handlers: dict = {}
        server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
        return server, handlers

    def test_registers_two_tools(self, recipes_md: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-scripting"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipes_md), nested=False)
        md.name = "maya-scripting"
        server, _handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        calls = [c.kwargs["name"] for c in server.registry.register.call_args_list]
        assert "recipes__list" in calls
        assert "recipes__get" in calls

    def test_list_handler_returns_anchors(self, recipes_md: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-scripting"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipes_md), nested=False)
        md.name = "maya-scripting"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__list"](json.dumps({"skill": "maya-scripting"}))
        assert result["success"] is True
        assert "create_polygon_cube" in result["context"]["anchors"]

    def test_list_unknown_skill_returns_error(self, tmp_path: Path) -> None:
        md = _make_metadata(None, None)
        md.name = "maya-scripting"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__list"](json.dumps({"skill": "unknown-skill"}))
        assert result["success"] is False
        assert "not found" in result["message"]

    def test_get_handler_returns_content(self, recipes_md: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-scripting"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipes_md), nested=False)
        md.name = "maya-scripting"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__get"](json.dumps({"skill": "maya-scripting", "anchor": "create_polygon_cube"}))
        assert result["success"] is True
        assert "polyCube" in result["context"]["content"]

    def test_get_unknown_anchor_returns_error(self, recipes_md: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-scripting"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipes_md), nested=False)
        md.name = "maya-scripting"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__get"](json.dumps({"skill": "maya-scripting", "anchor": "nonexistent"}))
        assert result["success"] is False
        assert "available_anchors" in result.get("context", {})

    def test_skill_without_recipes_file(self, tmp_path: Path) -> None:
        md = _make_metadata(None, None)
        md.name = "no-recipes-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__list"](json.dumps({"skill": "no-recipes-skill"}))
        assert result["success"] is True
        assert result["context"]["anchors"] == []

    def test_no_registry_logs_warning(self) -> None:
        class _BadServer:
            @property
            def registry(self):
                raise AttributeError("no registry")

        import logging

        with patch.object(logging.getLogger("dcc_mcp_core.recipes"), "warning") as mock_warn:
            register_recipes_tools(_BadServer(), skills=[])
        mock_warn.assert_called_once()
