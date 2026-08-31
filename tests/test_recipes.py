"""Tests for the recipes system (issue #428)."""

from __future__ import annotations

import json
from pathlib import Path
import textwrap
import time
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core._runtime.recipe_schema_patterns import pattern_is_safe
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

    def _make_recipe_handlers(
        self,
        tmp_path: Path,
        *,
        skill_name: str,
        recipes: dict[str, dict[str, object]],
    ) -> dict:
        """Register an in-memory recipe pack and return its handlers."""
        recipe_path = tmp_path / f"{skill_name}.yaml"
        recipe_path.write_text(
            json.dumps(
                {"recipes": [{"name": name, "inputs_schema": schema, "steps": []} for name, schema in recipes.items()]}
            ),
            encoding="utf-8",
        )
        skill_dir = tmp_path / skill_name
        skill_dir.mkdir()
        metadata = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        metadata.name = skill_name
        server, handlers = self._make_server([metadata])
        register_recipes_tools(server, skills=[metadata])
        return handlers

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

    @pytest.mark.parametrize(
        ("schema", "instance"),
        [
            ({"examples": [{"type": "integer"}], "$ref": "#/examples/0"}, 1),
            (
                {
                    "type": "object",
                    "examples": [{"type": "integer"}],
                    "properties": {"value": {"$ref": "#/examples/0"}},
                },
                {"value": 1},
            ),
        ],
    )
    def test_local_refs_must_target_schema_locations(self, schema: dict[str, object], instance: object) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, instance) == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        ("target", "instance"),
        [
            ({"type": "string", "pattern": "^(a+)+$"}, "a"),
            (
                {"type": "object", "patternProperties": {"^(a+)+$": {"type": "string"}}},
                {"a": "value"},
            ),
        ],
    )
    def test_local_ref_annotation_targets_cannot_bypass_pattern_admission(
        self, target: dict[str, object], instance: object
    ) -> None:
        schema = {"examples": [target], "$ref": "#/examples/0"}
        assert validate_recipe_inputs({"inputs_schema": schema}, instance) == ["$: Recipe input schema is invalid"]

    def test_local_ref_annotation_target_has_validate_apply_parity(self, tmp_path: Path) -> None:
        schema = {"examples": [{"type": "object"}], "$ref": "#/examples/0"}
        recipe_path = tmp_path / "annotation-ref.yaml"
        recipe_path.write_text(
            json.dumps({"recipes": [{"name": "annotation-ref", "inputs_schema": schema, "steps": []}]}),
            encoding="utf-8",
        )
        skill_dir = tmp_path / "annotation-ref-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "annotation-ref-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        params = {"skill": "annotation-ref-skill", "recipe": "annotation-ref", "inputs": {}}

        validated = handlers["recipes__validate"](json.dumps(params))
        applied = handlers["recipes__apply"](json.dumps(params))

        assert validated["context"]["valid"] is False
        assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
        assert applied["success"] is False
        assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

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

    def test_unique_items_uses_bounded_canonical_hashing(self) -> None:
        values = list(range(_RecipeSchemaValidator._MAX_CONTAINER_ITEMS))
        started = time.perf_counter()
        assert validate_recipe_inputs({"inputs_schema": {"uniqueItems": True}}, values) == []
        assert time.perf_counter() - started < 2.0

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
            ("a*a*$", "a" * 1000 + "!"),
            ("(a|b)+c$", "a" * 8000 + "!"),
            ("(.+)x$", "a" * 8000 + "!"),
            ("^" + "(a|b)+" * 11 + "$", "a" * 31 + "!"),
        ):
            started = time.perf_counter()
            errors = validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance)
            elapsed = time.perf_counter() - started
            assert errors == ["$: Recipe input schema is invalid"]
            assert elapsed < 1.0

    def test_linear_alternation_pattern_remains_supported(self) -> None:
        errors = validate_recipe_inputs(
            {"inputs_schema": {"type": "string", "pattern": "^(ab|cd)+$"}},
            "abcdab",
        )
        assert errors == []

    def test_grouped_adjacent_quantifiers_are_rejected_during_admission(self) -> None:
        for pattern in ("^(a*)a*$", "^(a*)(a*)$"):
            assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "a") == [
                "$: Recipe input schema is invalid"
            ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^a{,}a+$",
            "^a{,2}a+$",
            "^a{2,}a+$",
            "^a{1,2}a+$",
            "^a{,}?a+$",
            "^a{,2}?a+$",
            "^a{2,}?a+$",
            "^a{1,2}?a+$",
        ],
    )
    def test_braced_quantifier_forms_are_admitted_conservatively(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aa") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^a{,}b+$",
            "^a{,2}b+$",
            "^a{2,}b+$",
            "^a{1,2}b+$",
            "^a{,}?b+$",
            "^a{,2}?b+$",
            "^a{2,}?b+$",
            "^a{1,2}?b+$",
        ],
    )
    def test_braced_quantifier_forms_keep_disjoint_boundaries(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aab") == []

    @pytest.mark.parametrize(
        "pattern",
        [
            "^a+.+b$",
            r"^\w+\d+z$",
            r"^a+\Ba+z$",
            "^a+[]a]+z$",
        ],
    )
    def test_overlapping_consumers_and_zero_width_assertions_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaaz") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        ("pattern", "instance"),
        [
            ("^[a-z]+[0-9]+$", "abc123"),
            ("^(ab)+c+$", "ababcc"),
            ("^[]]+[a]+$", "]aaa"),
        ],
    )
    def test_disjoint_quantified_boundaries_remain_supported(self, pattern: str, instance: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance) == []

    @pytest.mark.parametrize(
        "pattern",
        [
            "^a+aa+$",
            "^a+(a|aa)a+$",
            "^a+(?:a|aa)a+$",
        ],
    )
    def test_connected_quantifiers_separated_by_fixed_consumers_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaaa") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize("pattern", ["^a+(a|aa)a+$", "^a+(?:a|aa)a+$"])
    def test_pattern_properties_connected_quantifier_sandwiches_fail_closed(self, pattern: str) -> None:
        schema = {"type": "object", "patternProperties": {pattern: {"type": "string"}}}
        assert validate_recipe_inputs({"inputs_schema": schema}, {"aaaa": "value"}) == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        ("pattern", "instance"),
        [
            ("^a+bb+$", "abbb"),
            ("^a+(?:b|cc)d+$", "abdd"),
            ("^(a|b)+ab+$", "aabb"),
        ],
    )
    def test_disjoint_fixed_consumers_clear_quantified_overlap(self, pattern: str, instance: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance) == []

    def test_unanchored_pattern_uses_draft_search_semantics(self) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": "foo"}}, "afoob") == []

    @pytest.mark.parametrize("pattern", ["a+b", "^x|a+b"])
    def test_unanchored_quantified_search_paths_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaab") == [
            "$: Recipe input schema is invalid"
        ]

    def test_anchored_quantified_search_path_remains_supported(self) -> None:
        assert (
            validate_recipe_inputs(
                {"inputs_schema": {"type": "string", "pattern": "^a+b$"}},
                "aaab",
            )
            == []
        )

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(?i:(ab|AB)+)$",
            "(?i)^(ab|AB)+$",
        ],
    )
    def test_casefold_equivalent_quantified_alternations_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "abAB") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(?i:(ab|cd)+)$",
            "(?i)^(ab|cd)+$",
        ],
    )
    def test_casefold_disjoint_quantified_alternations_remain_supported(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "abCD") == []

    def test_scoped_casefold_disable_restores_disjointness(self) -> None:
        pattern = "(?i)^(?-i:(ab|AB)+)$"
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "abAB") == []

    @pytest.mark.parametrize("pattern", ["(?m)^a+$", "^(?x:a +)$"])
    def test_unsupported_inline_flag_modes_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaa") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(a)(?(1)(a|aa)|x)+b$",
            "^(?P<lead>a)(?(lead)b|c)$",
        ],
    )
    def test_conditional_groups_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaab") == [
            "$: Recipe input schema is invalid"
        ]

    def test_pattern_properties_conditional_groups_fail_closed(self) -> None:
        schema = {
            "type": "object",
            "patternProperties": {"^(a)(?(1)(a|aa)|x)+b$": {"type": "string"}},
        }
        assert validate_recipe_inputs({"inputs_schema": schema}, {"aaab": "value"}) == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(a|)+$",
            "^(a|(?=a))+$",
            "^((a|))+$",
            "^(a|(?:))+$",
        ],
    )
    def test_nullable_quantified_groups_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "a") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        ("pattern", "instance"),
        [
            ("^(?:ab)+$", "abab"),
            ("^(?P<chunk>ab)+$", "abab"),
            ("^(?=a)a+$", "aaa"),
        ],
    )
    def test_supported_group_forms_remain_admitted(self, pattern: str, instance: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance) == []

    def test_repeated_prefix_overlapping_alternation_chain_fails_closed(self) -> None:
        pattern = "^(a|aa)(a|aa)(a|aa)b$"
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaaaaab") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize("pattern", ["^(a|)(a|)(a|)b$", "^(?:a|)(?:a|)(?:a|)b$"])
    def test_nullable_alternative_chains_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaab") == [
            "$: Recipe input schema is invalid"
        ]

    def test_pattern_properties_nullable_alternative_chain_fails_closed(self) -> None:
        schema = {
            "type": "object",
            "patternProperties": {"^(a|)(a|)(a|)b$": {"type": "string"}},
        }
        assert validate_recipe_inputs({"inputs_schema": schema}, {"aaab": "value"}) == [
            "$: Recipe input schema is invalid"
        ]

    def test_disjoint_alternation_chain_remains_supported(self) -> None:
        pattern = "^(a|b)(c|d)(e|f)g$"
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aceg") == []

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(a|aa)a(a|aa)a(a|aa)b$",
            "^(?:a|aa)a(?:a|aa)a(?:a|aa)b$",
            "^(a|)a(a|)a(a|)b$",
        ],
    )
    def test_connected_ambiguity_chains_separated_by_fixed_consumers_fail_closed(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aaaaab") == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^(a|aa)a(a|aa)a(a|aa)b$",
            "^(?:a|aa)a(?:a|aa)a(?:a|aa)b$",
            "^(a|)a(a|)a(a|)b$",
        ],
    )
    def test_pattern_properties_connected_ambiguity_chains_fail_closed(self, pattern: str) -> None:
        schema = {"type": "object", "patternProperties": {pattern: {"type": "string"}}}
        assert validate_recipe_inputs({"inputs_schema": schema}, {"aaaaab": "value"}) == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        ("pattern", "instance"),
        [
            ("^(a|aa)b(a|aa)c(a|aa)d$", "abacad"),
            ("^(?:a|aa)b(?:a|aa)c(?:a|aa)d$", "abacad"),
            ("^(a|)b(a|)c(a|)d$", "abacad"),
            ("^(a|aa|b|bb)a(b|bb)c$", "aabc"),
        ],
    )
    def test_disjoint_fixed_consumers_clear_ambiguous_overlap(self, pattern: str, instance: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, instance) == []

    def test_quantified_character_classes_are_bounded(self) -> None:
        pattern = "^[a-z]*[a-y]*$"
        started = time.perf_counter()
        errors = validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "a" * 40000 + "!")
        assert errors == ["$: Recipe input schema is invalid"]
        assert time.perf_counter() - started < 1.0

    @pytest.mark.parametrize(
        "schema",
        [
            {"type": "string", "format": "email"},
            {"type": "string", "contentEncoding": "base64"},
            {"$vocabulary": {"https://json-schema.org/draft/2020-12/vocab/format-assertion": True}},
            {"type": "string", "unknownAssertion": True},
        ],
    )
    def test_unsupported_assertion_vocabularies_fail_closed(self, schema: dict[str, object]) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, "not-an-email") == [
            "$: Recipe input schema is invalid"
        ]

    def test_unevaluated_true_annotations_propagate_through_all_of(self) -> None:
        object_schema = {"allOf": [{"unevaluatedProperties": True}], "unevaluatedProperties": False}
        array_schema = {"allOf": [{"unevaluatedItems": True}], "unevaluatedItems": False}
        assert validate_recipe_inputs({"inputs_schema": object_schema}, {"x": 1}) == []
        assert validate_recipe_inputs({"inputs_schema": array_schema}, [1]) == []

    def test_unevaluated_schema_annotations_propagate_after_success(self) -> None:
        object_schema = {
            "allOf": [{"unevaluatedProperties": {"type": "integer"}}],
            "unevaluatedProperties": False,
        }
        array_schema = {
            "allOf": [{"unevaluatedItems": {"type": "integer"}}],
            "unevaluatedItems": False,
        }
        assert validate_recipe_inputs({"inputs_schema": object_schema}, {"x": 1}) == []
        assert validate_recipe_inputs({"inputs_schema": array_schema}, [1]) == []

    @pytest.mark.parametrize(
        "schema",
        [
            {
                "type": "object",
                "properties": {"a": {"type": "integer"}},
                "dependentSchemas": {"a": {"unevaluatedProperties": False}},
            },
            {
                "type": "object",
                "properties": {"a": {"type": "integer"}},
                "if": {"properties": {"a": {"const": 1}}},
                "then": {"unevaluatedProperties": False},
            },
            {
                "type": "object",
                "properties": {"a": {"type": "integer"}},
                "allOf": [{"unevaluatedProperties": False}],
            },
        ],
    )
    def test_nested_subschemas_do_not_inherit_parent_annotations(self, schema: dict[str, object]) -> None:
        errors = validate_recipe_inputs({"inputs_schema": schema}, {"a": 1})
        assert errors == ["$.a: Unevaluated properties are not allowed"]

    def test_pattern_properties_rejects_catastrophic_keys_before_matching(self) -> None:
        schema = {"type": "object", "patternProperties": {"(a+)+$": {"type": "string"}}}
        started = time.perf_counter()
        errors = validate_recipe_inputs({"inputs_schema": schema}, {"a" * 27 + "!": "value"})
        elapsed = time.perf_counter() - started
        assert errors == ["$: Recipe input schema is invalid"]
        assert elapsed < 1.0

    def test_json_pointer_resolves_array_indices(self) -> None:
        schema = {"prefixItems": [{"type": "integer"}], "$ref": "#/prefixItems/0"}
        assert validate_recipe_inputs({"inputs_schema": schema}, 1) == []
        assert validate_recipe_inputs({"inputs_schema": schema}, "bad") == ["$: Expected type integer, got str"]

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

    @pytest.mark.parametrize(
        "schema",
        [
            {
                "$defs": {"name": {"$dynamicAnchor": "name", "type": "string"}},
                "$dynamicRef": "#name",
            },
            {"$dynamicAnchor": "root", "type": "string"},
        ],
    )
    def test_dynamic_reference_vocabulary_is_rejected_fail_closed(self, schema: dict[str, object]) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, 42) == ["$: Recipe input schema is invalid"]

    def test_dynamic_reference_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        recipe_path = tmp_path / "dynamic.yaml"
        recipe_path.write_text(
            json.dumps(
                {
                    "recipes": [
                        {
                            "name": "dynamic",
                            "inputs_schema": {"$dynamicRef": "#missing"},
                            "steps": [],
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        skill_dir = tmp_path / "dynamic-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "dynamic-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        params = {"skill": "dynamic-skill", "recipe": "dynamic", "inputs": {"secret": "redacted"}}
        validated = handlers["recipes__validate"](json.dumps(params))
        applied = handlers["recipes__apply"](json.dumps(params))
        assert validated["context"]["valid"] is False
        assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
        assert applied["success"] is False
        assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    def test_connected_quantifier_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        recipe_path = tmp_path / "connected-quantifiers.yaml"
        recipe_path.write_text(
            json.dumps(
                {
                    "recipes": [
                        {
                            "name": "connected-quantifiers",
                            "inputs_schema": {
                                "type": "object",
                                "properties": {
                                    "value": {"type": "string", "pattern": "^a+(a|aa)a+$"},
                                },
                            },
                            "steps": [],
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        skill_dir = tmp_path / "connected-quantifiers-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "connected-quantifiers-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        params = {
            "skill": "connected-quantifiers-skill",
            "recipe": "connected-quantifiers",
            "inputs": {"value": "aaaa"},
        }

        validated = handlers["recipes__validate"](json.dumps(params))
        applied = handlers["recipes__apply"](json.dumps(params))

        assert validated["context"]["valid"] is False
        assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
        assert applied["success"] is False
        assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    def test_connected_ambiguity_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        recipe_path = tmp_path / "connected-ambiguity.yaml"
        recipe_path.write_text(
            json.dumps(
                {
                    "recipes": [
                        {
                            "name": "connected-ambiguity",
                            "inputs_schema": {
                                "type": "object",
                                "properties": {
                                    "value": {"type": "string", "pattern": "^(a|aa)a(a|aa)a(a|aa)b$"},
                                },
                            },
                            "steps": [],
                        }
                    ]
                }
            ),
            encoding="utf-8",
        )
        skill_dir = tmp_path / "connected-ambiguity-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "connected-ambiguity-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])
        params = {
            "skill": "connected-ambiguity-skill",
            "recipe": "connected-ambiguity",
            "inputs": {"value": "aaaaab"},
        }

        validated = handlers["recipes__validate"](json.dumps(params))
        applied = handlers["recipes__apply"](json.dumps(params))

        assert validated["context"]["valid"] is False
        assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
        assert applied["success"] is False
        assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    def test_id_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        schemas = {
            "root-resource": {"$id": "recipe.json", "type": "object"},
            "nested-resource": {
                "type": "object",
                "properties": {"child": {"$id": "nested.json", "type": "object"}},
            },
            "invalid-id-null": {"$id": None},
            "invalid-id-array": {"$id": []},
        }
        recipe_path = tmp_path / "id.yaml"
        recipe_path.write_text(
            json.dumps(
                {"recipes": [{"name": name, "inputs_schema": schema, "steps": []} for name, schema in schemas.items()]}
            ),
            encoding="utf-8",
        )
        skill_dir = tmp_path / "id-skill"
        skill_dir.mkdir()
        md = _make_metadata(str(skill_dir), str(recipe_path), nested=True)
        md.name = "id-skill"
        server, handlers = self._make_server([md])
        register_recipes_tools(server, skills=[md])

        for recipe_name in schemas:
            params = {"skill": "id-skill", "recipe": recipe_name, "inputs": {}}
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            assert validated["context"]["valid"] is False
            assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
            assert applied["success"] is False
            assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^((a|aa)a)+$",
            "^(?:(?:a|aa)a)+$",
        ],
    )
    def test_quantified_wrappers_preserve_nested_ambiguity_at_admission(self, pattern: str) -> None:
        pattern_schema = {"type": "string", "pattern": pattern}
        pattern_properties_schema = {
            "type": "object",
            "patternProperties": {pattern: {"type": "string"}},
        }

        assert validate_recipe_inputs({"inputs_schema": pattern_schema}, "aaaa") == [
            "$: Recipe input schema is invalid"
        ]
        assert validate_recipe_inputs({"inputs_schema": pattern_properties_schema}, {"aaaa": "value"}) == [
            "$: Recipe input schema is invalid"
        ]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^((a|aa)b)+$",
            "^(?:(?:a|aa)b)+$",
        ],
    )
    def test_quantified_wrappers_allow_disjoint_fixed_consumer_control(self, pattern: str) -> None:
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "abaab") == []
        assert (
            validate_recipe_inputs(
                {
                    "inputs_schema": {
                        "type": "object",
                        "patternProperties": {pattern: {"type": "string"}},
                    }
                },
                {"abaab": "value"},
            )
            == []
        )

    def test_quantified_wrapper_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        pattern = "^((a|aa)a)+$"
        handlers = self._make_recipe_handlers(
            tmp_path,
            skill_name="quantified-wrapper-skill",
            recipes={
                "pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": pattern}},
                },
                "pattern-properties": {
                    "type": "object",
                    "patternProperties": {pattern: {"type": "string"}},
                },
            },
        )
        inputs_by_recipe = {
            "pattern": {"value": "aaaa"},
            "pattern-properties": {"aaaa": "value"},
        }

        for recipe_name, inputs in inputs_by_recipe.items():
            params = {"skill": "quantified-wrapper-skill", "recipe": recipe_name, "inputs": inputs}
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            assert validated["context"]["valid"] is False
            assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
            assert applied["success"] is False
            assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        ("schema", "inputs"),
        [
            ({"type": "string", "pattern": "^(?>ab)+$"}, "abab"),
            (
                {
                    "type": "object",
                    "patternProperties": {"^(?>ab)+$": {"type": "string"}},
                },
                {"abab": "value"},
            ),
            ({"type": "string", "pattern": "^ab++$"}, "ab"),
            ({"type": "string", "pattern": r"^\z$"}, ""),
            ({"type": "string", "pattern": r"^\N{EM DASH}$"}, "—"),
        ],
    )
    def test_python_version_specific_regex_syntax_fails_closed_during_admission(
        self,
        schema: dict[str, object],
        inputs: object,
    ) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, inputs) == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        "pattern",
        [
            "^a(?i)b$",
            "^(?:(?i)ab)$",
        ],
    )
    def test_nonleading_global_inline_flags_fail_closed_across_schema_keywords(self, pattern: str) -> None:
        assert pattern_is_safe(pattern) is False
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, "aB") == [
            "$: Recipe input schema is invalid"
        ]
        assert validate_recipe_inputs(
            {
                "inputs_schema": {
                    "type": "object",
                    "patternProperties": {pattern: {"type": "string"}},
                }
            },
            {"aB": "value"},
        ) == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        ("pattern", "value"),
        [
            ("(?i)^ab$", "AB"),
            ("^a(?i:b)$", "aB"),
            (r"^\(\?i\)$", "(?i)"),
            (r"^[()?i]+$", "(?i)"),
        ],
    )
    def test_portable_inline_flag_and_literal_controls_remain_supported(self, pattern: str, value: str) -> None:
        assert pattern_is_safe(pattern) is True
        assert validate_recipe_inputs({"inputs_schema": {"type": "string", "pattern": pattern}}, value) == []
        assert (
            validate_recipe_inputs(
                {
                    "inputs_schema": {
                        "type": "object",
                        "patternProperties": {pattern: {"type": "string"}},
                    }
                },
                {value: "value"},
            )
            == []
        )

    def test_inline_flag_portability_has_validate_apply_parity(self, tmp_path: Path) -> None:
        handlers = self._make_recipe_handlers(
            tmp_path,
            skill_name="inline-flag-portability-skill",
            recipes={
                "prefixed-global-pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": "^a(?i)b$"}},
                },
                "nested-global-pattern-properties": {
                    "type": "object",
                    "patternProperties": {"^(?:(?i)ab)$": {"type": "string"}},
                },
                "leading-global-pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": "(?i)^ab$"}},
                },
                "scoped-pattern-properties": {
                    "type": "object",
                    "patternProperties": {"^a(?i:b)$": {"type": "string"}},
                },
                "escaped-literal-pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": r"^\(\?i\)$"}},
                },
                "character-class-pattern-properties": {
                    "type": "object",
                    "patternProperties": {r"^[()?i]+$": {"type": "string"}},
                },
            },
        )
        cases = [
            ("prefixed-global-pattern", {"value": "aB"}, False),
            ("nested-global-pattern-properties", {"aB": "value"}, False),
            ("leading-global-pattern", {"value": "AB"}, True),
            ("scoped-pattern-properties", {"aB": "value"}, True),
            ("escaped-literal-pattern", {"value": "(?i)"}, True),
            ("character-class-pattern-properties", {"(?i)": "value"}, True),
        ]

        for recipe_name, inputs, expected_valid in cases:
            params = {"skill": "inline-flag-portability-skill", "recipe": recipe_name, "inputs": inputs}
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            assert validated["context"]["valid"] is expected_valid
            assert applied["success"] is expected_valid
            if not expected_valid:
                assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
                assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

    @pytest.mark.parametrize(
        ("schema", "inputs"),
        [
            ({"type": "string", "pattern": "^(?:ab)+$"}, "abab"),
            (
                {
                    "type": "object",
                    "patternProperties": {"^(?:ab)+$": {"type": "string"}},
                },
                {"abab": "value"},
            ),
            ({"type": "string", "pattern": r"^\Z$"}, ""),
            ({"type": "string", "pattern": r"^\u2014$"}, "—"),
        ],
    )
    def test_portable_regex_syntax_controls_remain_supported(
        self,
        schema: dict[str, object],
        inputs: object,
    ) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, inputs) == []

    def test_atomic_group_rejection_has_validate_apply_parity(self, tmp_path: Path) -> None:
        atomic_pattern = "^(?>ab)+$"
        portable_pattern = "^(?:ab)+$"
        handlers = self._make_recipe_handlers(
            tmp_path,
            skill_name="portable-regex-skill",
            recipes={
                "atomic-pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": atomic_pattern}},
                },
                "atomic-pattern-properties": {
                    "type": "object",
                    "patternProperties": {atomic_pattern: {"type": "string"}},
                },
                "portable-pattern": {
                    "type": "object",
                    "properties": {"value": {"type": "string", "pattern": portable_pattern}},
                },
            },
        )

        for recipe_name, inputs in (
            ("atomic-pattern", {"value": "abab"}),
            ("atomic-pattern-properties", {"abab": "value"}),
        ):
            params = {"skill": "portable-regex-skill", "recipe": recipe_name, "inputs": inputs}
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            assert validated["context"]["valid"] is False
            assert validated["context"]["errors"] == ["$: Recipe input schema is invalid"]
            assert applied["success"] is False
            assert applied["context"]["errors"] == ["$: Recipe input schema is invalid"]

        portable_params = {
            "skill": "portable-regex-skill",
            "recipe": "portable-pattern",
            "inputs": {"value": "abab"},
        }
        portable_validated = handlers["recipes__validate"](json.dumps(portable_params))
        portable_applied = handlers["recipes__apply"](json.dumps(portable_params))
        assert portable_validated["context"]["valid"] is True
        assert portable_applied["success"] is True

    def test_contains_annotation_branch_work_is_linear(self, monkeypatch: pytest.MonkeyPatch) -> None:
        original_merge = _RecipeSchemaValidator._merge_annotations
        merged_work = 0

        def counted_merge(
            validator: _RecipeSchemaValidator,
            source: dict[str, dict[str, set[object]]],
        ) -> None:
            nonlocal merged_work
            merged_work += sum(len(paths) + sum(len(values) for values in paths.values()) for paths in source.values())
            original_merge(validator, source)

        monkeypatch.setattr(_RecipeSchemaValidator, "_merge_annotations", counted_merge)
        schema = {
            "type": "array",
            "contains": {
                "type": "object",
                "required": ["matched"],
                "properties": {"matched": {"const": True}},
            },
        }
        work_by_size: list[tuple[int, int]] = []

        for size in (4, 8, 16):
            merged_work = 0
            values = [{"matched": True} for _ in range(size)]
            assert validate_recipe_inputs({"inputs_schema": {**schema, "minContains": size}}, values) == []
            work_by_size.append((size, merged_work))

        assert all(work <= size * 16 for size, work in work_by_size), work_by_size

    def test_contains_annotations_preserve_structure_and_validate_apply_parity(self, tmp_path: Path) -> None:
        schema = {
            "type": "object",
            "required": ["values"],
            "properties": {
                "values": {
                    "type": "array",
                    "contains": {
                        "type": "object",
                        "required": ["matched"],
                        "properties": {"matched": {"const": True}},
                    },
                    "minContains": 2,
                    "unevaluatedItems": False,
                }
            },
            "additionalProperties": False,
        }
        valid_inputs = {"values": [{"matched": True}, {"matched": True}]}
        invalid_inputs = {"values": [{"matched": True}, {"matched": False}]}
        assert validate_recipe_inputs({"inputs_schema": schema}, valid_inputs) == []
        assert validate_recipe_inputs({"inputs_schema": schema}, invalid_inputs)

        handlers = self._make_recipe_handlers(
            tmp_path,
            skill_name="contains-annotation-skill",
            recipes={"contains": schema},
        )
        for inputs, expected_valid in ((valid_inputs, True), (invalid_inputs, False)):
            params = {"skill": "contains-annotation-skill", "recipe": "contains", "inputs": inputs}
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            assert validated["context"]["valid"] is expected_valid
            assert applied["success"] is expected_valid

    @pytest.mark.parametrize(
        "schema",
        [
            {"$schema": None},
            {"$schema": []},
            {"$schema": "https://json-schema.org/draft/2019-09/schema"},
            {"$schema": "draft/2020-12/schema"},
            {"$schema": "urn:example:unsupported-dialect"},
            {
                "type": "object",
                "properties": {
                    "value": {
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "string",
                    }
                },
            },
        ],
    )
    def test_schema_dialect_declaration_fails_closed(self, schema: dict[str, object]) -> None:
        assert validate_recipe_inputs({"inputs_schema": schema}, {}) == ["$: Recipe input schema is invalid"]

    def test_supported_schema_dialect_is_allowed_only_at_resource_root(self) -> None:
        schema = {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"value": {"type": "string"}},
        }
        assert validate_recipe_inputs({"inputs_schema": schema}, {"value": "ok"}) == []

    def test_schema_dialect_admission_has_validate_apply_parity(self, tmp_path: Path) -> None:
        supported = "https://json-schema.org/draft/2020-12/schema"
        handlers = self._make_recipe_handlers(
            tmp_path,
            skill_name="schema-dialect-skill",
            recipes={
                "valid-root": {
                    "$schema": supported,
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                },
                "invalid-null": {"$schema": None},
                "invalid-relative": {"$schema": "draft/2020-12/schema"},
                "invalid-dialect": {"$schema": "https://json-schema.org/draft/2019-09/schema"},
                "invalid-nested": {
                    "type": "object",
                    "properties": {"value": {"$schema": supported, "type": "string"}},
                },
            },
        )

        for recipe_name in ("valid-root", "invalid-null", "invalid-relative", "invalid-dialect", "invalid-nested"):
            params = {
                "skill": "schema-dialect-skill",
                "recipe": recipe_name,
                "inputs": {"value": "ok"},
            }
            validated = handlers["recipes__validate"](json.dumps(params))
            applied = handlers["recipes__apply"](json.dumps(params))
            expected_valid = recipe_name == "valid-root"
            assert validated["context"]["valid"] is expected_valid
            assert applied["success"] is expected_valid

    def test_local_anchor_reference_is_supported(self) -> None:
        schema = {"$defs": {"name": {"$anchor": "name", "type": "string"}}, "$ref": "#name"}
        assert validate_recipe_inputs({"inputs_schema": schema}, "ok") == []
        assert validate_recipe_inputs({"inputs_schema": schema}, 42) == ["$: Expected type string, got int"]

    @pytest.mark.parametrize("keyword", ["const", "enum"])
    def test_ref_like_instance_data_is_not_preflighted(self, keyword: str) -> None:
        literal = {"$ref": "#/missing"}
        schema = {keyword: literal if keyword == "const" else [literal]}
        assert validate_recipe_inputs({"inputs_schema": schema}, literal) == []

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
