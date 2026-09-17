"""Validation for the cross-adapter mesh-ops modeling recipe pack (#2259).

The mesh-ops skill ships a single ``RECIPES.yaml`` recipe pack that captures
the shared cross-DCC modeling vocabulary (primitive creation, hard-surface
edits, topology cleanup, UV generation, and material assignment). This module
asserts the pack is structurally sound, is genuinely cross-adapter (``dcc:
any`` + ``toolset_profiles: [mesh-ops]``), and that every published
``inputs_schema`` accepts valid inputs and rejects invalid ones through the
dependency-free Draft 2020-12 validator.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

import dcc_mcp_core
from dcc_mcp_core.recipes import find_recipe_entry
from dcc_mcp_core.recipes import list_recipe_entries
from dcc_mcp_core.recipes import load_recipe_pack
from dcc_mcp_core.recipes import register_recipes_tools
from dcc_mcp_core.recipes import validate_recipe_inputs

REPO_ROOT = Path(__file__).resolve().parent.parent
MESH_OPS_SKILL_DIR = REPO_ROOT / "skills" / "mesh-ops"
RECIPES_PATH = MESH_OPS_SKILL_DIR / "RECIPES.yaml"

EXPECTED_RECIPES = [
    "create_primitive",
    "bevel_edges",
    "extrude_faces",
    "inset_faces",
    "boolean_op",
    "add_edge_loop",
    "merge_vertices",
    "triangulate_mesh",
    "cleanup_mesh",
    "combine_meshes",
    "mirror",
    "loft_sections",
    "lathe_profile",
    "auto_uv",
    "uv_project",
    "assign_material",
    "set_pivot",
    "get_poly_count",
]

#: One canonical valid input payload per recipe.
VALID_INPUTS: dict[str, dict[str, Any]] = {
    "create_primitive": {"primitive_type": "cube", "name": "MyCube"},
    "bevel_edges": {"object_name": "Cube", "edge_indices": [0, 1], "width": 0.5},
    "extrude_faces": {"object_name": "Cube", "face_indices": [0], "distance": 1.0, "direction": [0, 0, 1]},
    "inset_faces": {"object_name": "Cube", "face_indices": [0], "thickness": 0.25},
    "boolean_op": {"input_a": "A", "input_b": "B", "operation": "union"},
    "add_edge_loop": {"object_name": "Cube", "edge_indices": [0]},
    "merge_vertices": {"object_name": "Cube"},
    "triangulate_mesh": {"object_name": "Cube"},
    "cleanup_mesh": {"object_name": "Cube"},
    "combine_meshes": {"object_names": ["A", "B"]},
    "mirror": {"object_name": "Cube"},
    "loft_sections": {"sections": ["loop0", "loop1"]},
    "lathe_profile": {"profile": "profileCurve"},
    "auto_uv": {"object_name": "Cube"},
    "uv_project": {"object_name": "Cube", "projection": "planar"},
    "assign_material": {"object_name": "Cube", "material_name": "mat"},
    "set_pivot": {"object_name": "Cube", "position": [0, 0, 0]},
    "get_poly_count": {"object_name": "Cube"},
}

#: (recipe_name, invalid_inputs, expected_error_fragment)
INVALID_CASES: list[tuple[str, dict[str, Any], str]] = [
    ("create_primitive", {"primitive_type": "blob", "name": "X"}, "Value is not one of the allowed options"),
    ("bevel_edges", {"object_name": "C", "edge_indices": [0], "width": 0}, "below the exclusive minimum"),
    ("boolean_op", {"input_a": "A", "input_b": "B", "operation": "xor"}, "Value is not one of the allowed options"),
    ("combine_meshes", {"object_names": ["A"]}, "fewer than the minimum"),
    ("mirror", {"object_name": "C", "axis": "w"}, "Value is not one of the allowed options"),
    ("get_poly_count", {}, "Missing required input: object_name"),
]


@pytest.fixture(scope="module")
def recipe_pack() -> list[Any]:
    """Load the cross-adapter mesh-ops recipe pack once per module."""
    assert RECIPES_PATH.is_file(), f"missing recipe pack: {RECIPES_PATH}"
    recipes = load_recipe_pack(str(RECIPES_PATH), skill_name="mesh-ops")
    assert recipes, "recipe pack loaded no recipes"
    return recipes


@pytest.fixture(scope="module")
def recipes_by_name(recipe_pack: list[Any]) -> dict[str, dict[str, Any]]:
    return {recipe.to_dict()["name"]: recipe.to_dict() for recipe in recipe_pack}


def test_skill_declares_recipe_pack() -> None:
    assert (MESH_OPS_SKILL_DIR / "SKILL.md").is_file()
    report = dcc_mcp_core.validate_skill(str(MESH_OPS_SKILL_DIR))
    assert report.is_clean, f"mesh-ops skill is not clean: {report.issues}"


def test_recipe_pack_has_expected_recipes(recipes_by_name: dict[str, dict[str, Any]]) -> None:
    assert list(recipes_by_name) == EXPECTED_RECIPES


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_recipe_is_cross_adapter(recipes_by_name: dict[str, dict[str, Any]], recipe_name: str) -> None:
    recipe = recipes_by_name[recipe_name]
    assert recipe["dcc"] == "any", f"{recipe_name} must declare dcc: any"
    assert recipe["toolset_profiles"] == ["mesh-ops"], f"{recipe_name} must bind to the mesh-ops toolset"


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_recipe_has_typed_input_schema(recipes_by_name: dict[str, dict[str, Any]], recipe_name: str) -> None:
    schema = recipes_by_name[recipe_name]["inputs_schema"]
    assert isinstance(schema, dict) and schema.get("type") == "object"
    assert "properties" in schema
    assert "required" in schema


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_recipe_steps_use_canonical_tool_names(recipes_by_name: dict[str, dict[str, Any]], recipe_name: str) -> None:
    steps = recipes_by_name[recipe_name]["steps"]
    assert steps, f"{recipe_name} has no steps"
    for step in steps:
        tool = step["tool"]
        assert tool.startswith("mesh_ops__"), f"{recipe_name} step tool '{tool}' must use the canonical namespace"
        assert tool == f"mesh_ops__{recipe_name}", f"{recipe_name} step tool '{tool}' must match its canonical verb"
        assert isinstance(step.get("arguments"), dict)


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_valid_inputs_pass(recipes_by_name: dict[str, dict[str, Any]], recipe_name: str) -> None:
    recipe = recipes_by_name[recipe_name]
    errors = validate_recipe_inputs(recipe, VALID_INPUTS[recipe_name])
    assert errors == [], f"{recipe_name} rejected valid inputs: {errors}"


@pytest.mark.parametrize(("recipe_name", "invalid_inputs", "fragment"), INVALID_CASES)
def test_invalid_inputs_fail(
    recipes_by_name: dict[str, dict[str, Any]],
    recipe_name: str,
    invalid_inputs: dict[str, Any],
    fragment: str,
) -> None:
    recipe = recipes_by_name[recipe_name]
    errors = validate_recipe_inputs(recipe, invalid_inputs)
    assert errors, f"{recipe_name} accepted invalid inputs: {invalid_inputs}"
    assert any(fragment in error for error in errors), f"unexpected errors for {recipe_name}: {errors}"


def test_uv_project_axis_constraint(recipes_by_name: dict[str, dict[str, Any]]) -> None:
    """Non-planar projections force the z sentinel axis."""
    recipe = recipes_by_name["uv_project"]
    assert validate_recipe_inputs(recipe, {"object_name": "C", "projection": "spherical"}) == []
    errors = validate_recipe_inputs(recipe, {"object_name": "C", "projection": "spherical", "axis": "x"})
    assert any("Value does not match the required constant" in error for error in errors)


def _make_metadata(skill_path: str) -> MagicMock:
    md = MagicMock()
    md.name = "mesh-ops"
    md.skill_path = skill_path
    md.metadata = {"dcc-mcp": {"recipes": "RECIPES.yaml"}}
    return md


def test_list_recipe_entries_serves_structured_pack() -> None:
    md = _make_metadata(str(MESH_OPS_SKILL_DIR))
    entries = list_recipe_entries(md)
    assert [entry["name"] for entry in entries] == EXPECTED_RECIPES
    assert all(entry["provenance"]["format"] == "recipe-pack" for entry in entries)
    assert find_recipe_entry(md, "bevel_edges") is not None


def test_recipes_tools_serve_cross_adapter_search() -> None:
    md = _make_metadata(str(MESH_OPS_SKILL_DIR))
    server = MagicMock()
    handlers: dict[str, Any] = {}
    server.registry = MagicMock()
    server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
    register_recipes_tools(server, skills=[md], dcc_name="any")

    # A query for a modeling verb finds the cross-adapter recipe.
    listed = handlers["recipes__list"](json.dumps({"skill": "mesh-ops"}))
    assert listed["success"] is True
    assert listed["context"]["recipes"][0]["name"] == "create_primitive"

    found = handlers["recipes__search"](json.dumps({"query": "bevel"}))
    assert found["success"] is True
    assert found["context"]["recipes"][0]["name"] == "bevel_edges"

    applied = handlers["recipes__apply"](
        json.dumps({"skill": "mesh-ops", "recipe": "bevel_edges", "inputs": VALID_INPUTS["bevel_edges"]}),
    )
    assert applied["success"] is True
    assert applied["context"]["steps"][0]["tool"] == "mesh_ops__bevel_edges"
    assert applied["context"]["output_contract"] == "mesh_component_edit"
