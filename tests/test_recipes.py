"""Tests for the recipes system (issue #428)."""

from __future__ import annotations

import json
from pathlib import Path
import textwrap
import time
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core.recipes import _RecipeSchemaValidator
from dcc_mcp_core.recipes import get_recipe_content
from dcc_mcp_core.recipes import get_recipes_path
from dcc_mcp_core.recipes import get_recipes_paths
from dcc_mcp_core.recipes import list_recipe_entries
from dcc_mcp_core.recipes import load_recipe_pack
from dcc_mcp_core.recipes import parse_recipe_anchors
from dcc_mcp_core.recipes import register_recipes_tools
from dcc_mcp_core.recipes import validate_recipe_inputs

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


@pytest.fixture()
def recipe_pack_yaml(tmp_path: Path) -> Path:
    content = textwrap.dedent(
        """\
        recipes:
          - name: build_pbr_material
            dcc: maya
            description: Build a PBR material network.
            inputs_schema:
              type: object
              required: [material_name, roughness]
              properties:
                material_name:
                  type: string
                roughness:
                  type: number
            steps:
              - tool: maya_materials__create
                arguments:
                  name: ${material_name}
              - tool: maya_materials__set_roughness
                arguments:
                  value: ${roughness}
            output_contract: material_graph
            toolset_profiles: [lookdev, surfacing]
        """
    )
    p = tmp_path / "recipes.yaml"
    p.write_text(content, encoding="utf-8")
    return p


@pytest.fixture()
def bounded_recipe_yaml(tmp_path: Path) -> Path:
    """Write a published-style bounded schema for registered-handler tests."""
    payload = {
        "recipes": [
            {
                "name": "bounded_recipe",
                "inputs_schema": {
                    "type": "object",
                    "required": ["axis", "count", "pivot", "name_prefix", "mode"],
                    "additionalProperties": False,
                    "properties": {
                        "axis": {"type": "string", "enum": ["x", "y", "z"]},
                        "count": {"type": "integer", "minimum": 1, "maximum": 128},
                        "pivot": {"type": "array", "minItems": 3, "maxItems": 3, "items": {"type": "number"}},
                        "name_prefix": {"type": "string", "minLength": 1, "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"},
                        "mode": {"oneOf": [{"const": "fast"}, {"const": "safe"}]},
                    },
                },
                "steps": [{"tool": "test__bounded"}],
                "output_contract": "test",
            }
        ]
    }
    p = tmp_path / "bounded-recipes.yaml"
    p.write_text(json.dumps(payload), encoding="utf-8")
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

    def test_get_recipes_paths_expands_glob(self, tmp_path: Path) -> None:
        skill_dir = tmp_path / "my-skill"
        recipe_dir = skill_dir / "recipes"
        recipe_dir.mkdir(parents=True)
        (recipe_dir / "a.yaml").write_text("recipes: []\n", encoding="utf-8")
        (recipe_dir / "b.yaml").write_text("recipes: []\n", encoding="utf-8")
        md = _make_metadata(str(skill_dir), "recipes/*.yaml", nested=True)

        assert get_recipes_paths(md) == [
            str(recipe_dir / "a.yaml"),
            str(recipe_dir / "b.yaml"),
        ]


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


# ── structured recipe packs ────────────────────────────────────────────────


class TestRecipePacks:
    def test_load_recipe_pack_returns_structured_recipe(self, recipe_pack_yaml: Path) -> None:
        recipes = load_recipe_pack(str(recipe_pack_yaml), skill_name="maya-domain")

        assert len(recipes) == 1
        payload = recipes[0].to_dict()
        assert payload["name"] == "build_pbr_material"
        assert payload["dcc"] == "maya"
        assert payload["inputs_schema"]["required"] == ["material_name", "roughness"]
        assert payload["steps"][0]["tool"] == "maya_materials__create"
        assert payload["provenance"]["skill"] == "maya-domain"

    def test_list_recipe_entries_includes_yaml_pack(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"

        entries = list_recipe_entries(md)

        assert [entry["name"] for entry in entries] == ["build_pbr_material"]
        assert entries[0]["provenance"]["format"] == "recipe-pack"

    def test_validate_recipe_inputs_reports_missing_and_type_errors(self, recipe_pack_yaml: Path) -> None:
        recipe = load_recipe_pack(str(recipe_pack_yaml))[0].to_dict()

        errors = validate_recipe_inputs(recipe, {"material_name": "mat", "roughness": "high"})

        assert errors == ["Input 'roughness' expected number, got str"]
        assert validate_recipe_inputs(recipe, {"material_name": "mat", "roughness": 0.5}) == []


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
        assert "recipes__search" in calls
        assert "recipes__validate" in calls
        assert "recipes__apply" in calls

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

    def test_list_handler_returns_structured_recipes(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__list"](json.dumps({"skill": "maya-domain"}))

        assert result["success"] is True
        assert result["context"]["anchors"] == []
        assert result["context"]["recipes"][0]["name"] == "build_pbr_material"

    def test_get_handler_returns_structured_recipe(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__get"](json.dumps({"skill": "maya-domain", "anchor": "build_pbr_material"}))

        assert result["success"] is True
        assert result["context"]["recipe"]["output_contract"] == "material_graph"

    def test_search_handler_finds_structured_recipe(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__search"](json.dumps({"query": "pbr", "dcc": "maya"}))

        assert result["success"] is True
        assert result["context"]["recipes"][0]["name"] == "build_pbr_material"

    def test_validate_handler_checks_recipe_inputs(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__validate"](
            json.dumps({"skill": "maya-domain", "recipe": "build_pbr_material", "inputs": {"material_name": "mat"}}),
        )

        assert result["success"] is True
        assert result["context"]["valid"] is False
        assert "Missing required input: roughness" in result["context"]["errors"]

    def test_apply_handler_returns_application_plan(self, recipe_pack_yaml: Path, tmp_path: Path) -> None:
        skill_dir = tmp_path / "maya-domain"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_pack_yaml), nested=True)
        md.name = "maya-domain"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        result = handlers["recipes__apply"](
            json.dumps(
                {
                    "skill": "maya-domain",
                    "recipe": "build_pbr_material",
                    "inputs": {"material_name": "mat", "roughness": 0.5},
                    "target": "scene",
                },
            ),
        )

        assert result["success"] is True
        assert result["context"]["steps"][0]["tool"] == "maya_materials__create"
        assert result["context"]["output_contract"] == "material_graph"

    @pytest.mark.parametrize(
        "mutate, expected_fragment",
        [
            (lambda value: {**value, "axis": "secret-axis"}, "$.axis"),
            (lambda value: {**value, "objects": ["forbidden"]}, "$.objects"),
            (lambda value: {**value, "count": 129}, "$.count"),
            (lambda value: {**value, "pivot": [0, 0]}, "$.pivot"),
            (lambda value: {**value, "pivot": [0, "bad", 0]}, "$.pivot[1]"),
            (lambda value: {**value, "name_prefix": ""}, "$.name_prefix"),
            (lambda value: {**value, "mode": "other"}, "$.mode"),
        ],
    )
    def test_validate_and_apply_reject_complete_schema_counterexamples(
        self, bounded_recipe_yaml: Path, tmp_path: Path, mutate, expected_fragment: str
    ) -> None:
        skill_dir = tmp_path / "bounded"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(bounded_recipe_yaml), nested=True)
        md.name = "bounded"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        valid_inputs = {"axis": "x", "count": 3, "pivot": [0, 0, 0], "name_prefix": "item", "mode": "fast"}
        invalid_inputs = mutate(valid_inputs)

        validated = handlers["recipes__validate"](
            json.dumps({"skill": "bounded", "recipe": "bounded_recipe", "inputs": invalid_inputs})
        )
        applied = handlers["recipes__apply"](
            json.dumps({"skill": "bounded", "recipe": "bounded_recipe", "inputs": invalid_inputs})
        )

        assert validated["success"] is True
        assert validated["context"]["valid"] is False
        assert any(expected_fragment in error for error in validated["context"]["errors"])
        assert applied["success"] is False
        assert any(expected_fragment in error for error in applied["context"]["errors"])

    def test_malformed_published_schema_fails_closed(self, tmp_path: Path) -> None:
        recipe = {"inputs_schema": {"type": "not-a-json-type"}}
        errors = validate_recipe_inputs(recipe, {})
        assert errors == ["$: Recipe input schema is invalid"]

        deep: dict[str, object] = {"type": "object"}
        root = deep
        for _ in range(130):
            child: dict[str, object] = {"type": "object"}
            root["properties"] = {"child": child}
            root = child
        assert validate_recipe_inputs({"inputs_schema": deep}, {}) == ["$: Recipe input schema is invalid"]

        duplicate_required = {"required": ["x", "x"]}
        duplicate_dependent = {"dependentRequired": {"x": ["y", "y"]}}
        assert validate_recipe_inputs({"inputs_schema": duplicate_required}, {}) == [
            "$: Recipe input schema is invalid"
        ]
        assert validate_recipe_inputs({"inputs_schema": duplicate_dependent}, {}) == [
            "$: Recipe input schema is invalid"
        ]

    def test_recursive_ref_and_ref_siblings_are_validated(self) -> None:
        schema = {
            "$defs": {
                "node": {
                    "type": "object",
                    "properties": {"value": {"type": "integer"}, "child": {"$ref": "#/$defs/node"}},
                    "additionalProperties": False,
                },
                "positive": {"type": "integer", "minimum": 2},
            },
            "type": "object",
            "properties": {
                "tree": {"$ref": "#/$defs/node"},
                "bounded": {"$ref": "#/$defs/positive", "maximum": 5},
            },
        }
        assert (
            validate_recipe_inputs(
                {"inputs_schema": schema},
                {"tree": {"value": 1, "child": {"value": 2}}, "bounded": 3},
            )
            == []
        )
        errors = validate_recipe_inputs({"inputs_schema": schema}, {"tree": {"value": 1}, "bounded": 6})
        assert any("$.bounded" in error and "maximum" in error for error in errors)
        assert validate_recipe_inputs({"inputs_schema": {"$ref": "#/missing"}}, {}) == [
            "$: Recipe input schema is invalid"
        ]
        root_ref_schema = {
            "type": "object",
            "required": ["value"],
            "properties": {"value": {"type": "integer"}, "child": {"$ref": "#"}},
        }
        assert validate_recipe_inputs({"inputs_schema": root_ref_schema}, {"value": 1, "child": {}}) == [
            "$.child: Missing required property 'value'"
        ]

    def test_unevaluated_keywords_and_json_number_semantics(self) -> None:
        schema = {
            "type": "object",
            "properties": {"value": {"enum": [1]}, "items": {"type": "array", "prefixItems": [{"type": "number"}]}},
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": schema}, {"value": 1.0, "items": [1]}) == []
        errors = validate_recipe_inputs({"inputs_schema": schema}, {"value": 1, "items": [1], "extra": "secret"})
        assert any("$.extra" in error and "Unevaluated" in error for error in errors)
        composed = {
            "allOf": [{"type": "object", "properties": {"known": {"type": "string"}}}],
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": composed}, {"known": "ok"}) == []
        assert any(
            "$.extra" in error
            for error in validate_recipe_inputs({"inputs_schema": composed}, {"known": "ok", "extra": 1})
        )
        ref_composed = {
            "$defs": {"base": {"properties": {"known": {"type": "string"}}}},
            "$ref": "#/$defs/base",
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": ref_composed}, {"known": "ok"}) == []
        either = {
            "anyOf": [
                {"properties": {"left": {"type": "integer"}}},
                {"properties": {"right": {"type": "integer"}}},
            ],
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": either}, {"left": 1}) == []
        assert any("$.extra" in error for error in validate_recipe_inputs({"inputs_schema": either}, {"extra": 1}))
        additional = {
            "type": "object",
            "additionalProperties": True,
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": additional}, {"extra": "allowed"}) == []
        assert (
            validate_recipe_inputs(
                {"inputs_schema": {"type": "array", "prefixItems": [{"type": "number"}], "unevaluatedItems": False}},
                [1],
            )
            == []
        )
        errors = validate_recipe_inputs(
            {"inputs_schema": {"type": "array", "prefixItems": [{"type": "number"}], "unevaluatedItems": False}},
            [1, 2],
        )
        assert any("[1]" in error and "Unevaluated" in error for error in errors)
        contains = {"type": "array", "contains": {"const": 1}, "unevaluatedItems": False}
        assert validate_recipe_inputs({"inputs_schema": contains}, [1]) == []
        assert any("[1]" in error for error in validate_recipe_inputs({"inputs_schema": contains}, [1, 2]))

        failed_all_of = {
            "allOf": [{"properties": {"a": {"type": "string"}}}],
            "unevaluatedProperties": False,
        }
        errors = validate_recipe_inputs({"inputs_schema": failed_all_of}, {"a": 1})
        assert any("$.a" in error and "Unevaluated" in error for error in errors)

    def test_instance_depth_budget_is_independent_of_rendered_property_path(
        self, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        monkeypatch.setattr(_RecipeSchemaValidator, "_MAX_DEPTH", 1)
        schema = {"type": "object", "properties": {"a.b": {"type": "string"}}}
        assert validate_recipe_inputs({"inputs_schema": schema}, {"a.b": "ok"}) == []

    def test_if_annotations_feed_unevaluated_properties(self) -> None:
        conditional = {
            "if": {"properties": {"kind": {"const": "a"}}},
            "then": {"properties": {"value": {"type": "integer"}}},
            "else": {"properties": {"fallback": {"type": "string"}}},
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": conditional}, {"kind": "a", "value": 1}) == []
        assert any(
            "$.kind" in error
            for error in validate_recipe_inputs({"inputs_schema": conditional}, {"kind": "b", "fallback": "ok"})
        )
        assert any(
            "$.extra" in error
            for error in validate_recipe_inputs(
                {"inputs_schema": conditional}, {"kind": "a", "value": 1, "extra": True}
            )
        )
        nested = {
            "if": {"properties": {"outer": {"properties": {"kind": {"const": "a"}}}}},
            "then": {"properties": {"outer": {"properties": {"value": {"type": "integer"}}}}},
            "unevaluatedProperties": False,
        }
        assert validate_recipe_inputs({"inputs_schema": nested}, {"outer": {"kind": "a", "value": 1}}) == []
        no_branch = {"if": {"properties": {"kind": {"const": "a"}}}, "unevaluatedProperties": False}
        assert validate_recipe_inputs({"inputs_schema": no_branch}, {"kind": "a"}) == []
        assert any("$.kind" in error for error in validate_recipe_inputs({"inputs_schema": no_branch}, {"kind": "b"}))
        nested_errors = validate_recipe_inputs({"inputs_schema": nested}, {"outer": {"kind": "b"}})
        assert any("$.outer" in error for error in nested_errors)

    def test_unique_items_is_bounded_without_item_schemas(self) -> None:
        values = list(range(_RecipeSchemaValidator._MAX_CONTAINER_ITEMS + 1))
        assert validate_recipe_inputs({"inputs_schema": {"uniqueItems": True}}, values) == [
            "$: Recipe input schema is invalid"
        ]

    def test_schema_size_budget_counts_utf8_bytes(self, monkeypatch: pytest.MonkeyPatch) -> None:
        schema = {"const": "界"}
        encoded_size = len(json.dumps(schema, ensure_ascii=False).encode("utf-8"))
        monkeypatch.setattr(_RecipeSchemaValidator, "_MAX_SCHEMA_SIZE", encoded_size - 1)
        assert validate_recipe_inputs({"inputs_schema": schema}, "界") == ["$: Recipe input schema is invalid"]

    def test_instance_size_budget_counts_utf8_bytes(self, monkeypatch: pytest.MonkeyPatch) -> None:
        instance = {"text": "界"}
        encoded_size = len(json.dumps(instance, ensure_ascii=False).encode("utf-8"))
        monkeypatch.setattr(_RecipeSchemaValidator, "_MAX_INSTANCE_SIZE", encoded_size - 1)
        assert validate_recipe_inputs({"inputs_schema": {"type": "object"}}, instance) == [
            "$: Recipe input schema is invalid"
        ]

    def test_catastrophic_pattern_is_rejected_before_matching(self) -> None:
        for pattern, instance in (
            ("(a+)+$", "a" * 27 + "!"),
            ("((ab)*)*$", "ab" * 24 + "!"),
        ):
            started = time.perf_counter()
            errors = validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance)
            elapsed = time.perf_counter() - started
            assert errors == ["$: Recipe input schema is invalid"]
            assert elapsed < 1.0

    def test_non_mapping_schema_is_not_coerced_to_empty_schema(self) -> None:
        for malformed in ([], "schema", None):
            assert validate_recipe_inputs({"inputs_schema": malformed}, {}) == ["$: Recipe input schema is invalid"]

    def test_registered_handlers_reject_non_mapping_published_schema(self, tmp_path: Path) -> None:
        recipe_path = tmp_path / "malformed.yaml"
        recipe_path.write_text(
            json.dumps({"recipes": [{"name": "bad", "inputs_schema": [], "steps": []}]}), encoding="utf-8"
        )
        skill_dir = tmp_path / "bad-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "bad-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        params = {"skill": "bad-skill", "recipe": "bad", "inputs": {"secret": "redacted"}}
        validated = handlers["recipes__validate"](json.dumps(params))
        applied = handlers["recipes__apply"](json.dumps(params))
        assert validated["context"]["valid"] is False
        assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
        assert applied["success"] is False
        assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    def test_multiple_of_and_unique_items_use_exact_json_number_comparison(self) -> None:
        schema = {
            "type": "object",
            "properties": {
                "value": {"type": "number", "multipleOf": 0.1},
                "integer": {"type": "integer"},
                "values": {"uniqueItems": True},
            },
        }
        assert validate_recipe_inputs({"inputs_schema": schema}, {"value": 0.3, "values": [1, 1.0]}) == [
            "$.values[1]: Array items must be unique"
        ]
        assert validate_recipe_inputs({"inputs_schema": schema}, {"integer": 1.0}) == []
        huge = 10**2000
        assert validate_recipe_inputs({"inputs_schema": {"type": "integer", "multipleOf": huge}}, huge) == []
        assert validate_recipe_inputs({"inputs_schema": {"type": "number", "multipleOf": 0.1}}, 1e308) == []
        assert validate_recipe_inputs(
            {
                "inputs_schema": {
                    "type": "object",
                    "dependentRequired": {"source": ["format"]},
                    "dependentSchemas": {"source": {"properties": {"format": {"const": "json"}}}},
                }
            },
            {"source": True},
        ) == ["$: Property 'format' is required when 'source' is present"]

    def test_no_registry_logs_warning(self) -> None:
        class _BadServer:
            @property
            def registry(self):
                raise AttributeError("no registry")

        import logging

        with patch.object(logging.getLogger("dcc_mcp_core.recipes"), "warning") as mock_warn:
            register_recipes_tools(_BadServer(), skills=[])
        mock_warn.assert_called_once()
