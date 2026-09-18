"""Validation for the cross-adapter mesh-ops modeling recipe pack (#2259).

The mesh-ops skill ships a single ``RECIPES.yaml`` recipe pack that captures
the shared cross-DCC modeling vocabulary (primitive creation, hard-surface
edits, topology cleanup, radial arraying, UV generation, and material
assignment). This module asserts the pack is:

* structurally sound and genuinely cross-adapter (``dcc: any`` +
  ``toolset_profiles: [mesh-ops]``);
* published with **machine-checkable** ``output_contract`` Draft 2020-12
  schemas instead of untyped labels — including the #2259 acceptance rule
  that the UV recipe's contract fails on a mesh without UVs;
* routed only to tools the adapters really publish (``tool_routing`` is
  checked against ``tests/fixtures/mesh_ops_adapter_catalog.json``);
* materializable — every ``${param}`` placeholder in a step names an input
  that is required or carries a default, so a dispatched plan never leaks a
  literal ``${...}``.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

from jsonschema import Draft202012Validator
from jsonschema import ValidationError
import pytest

import dcc_mcp_core
from dcc_mcp_core import yaml_loads
from dcc_mcp_core._runtime.recipe_schema import _RecipeSchemaValidator
from dcc_mcp_core.recipes import find_recipe_entry
from dcc_mcp_core.recipes import list_recipe_entries
from dcc_mcp_core.recipes import load_recipe_pack
from dcc_mcp_core.recipes import register_recipes_tools
from dcc_mcp_core.recipes import validate_recipe_inputs

REPO_ROOT = Path(__file__).resolve().parent.parent
MESH_OPS_SKILL_DIR = REPO_ROOT / "skills" / "mesh-ops"
RECIPES_PATH = MESH_OPS_SKILL_DIR / "RECIPES.yaml"
ADAPTER_CATALOG_PATH = REPO_ROOT / "tests" / "fixtures" / "mesh_ops_adapter_catalog.json"

#: The canonical verb inventory. Adding a recipe to the pack requires adding
#: its name here (and its routing entry in ``tool_routing``), so the pack
#: cannot grow silently.
EXPECTED_RECIPES = [
    "create_primitive",
    "bevel_edges",
    "extrude_faces",
    "inset",
    "boolean_op",
    "add_edge_loop",
    "merge_vertices",
    "triangulate_mesh",
    "cleanup_mesh",
    "combine_meshes",
    "mirror",
    "loft_sections",
    "lathe_profile",
    "radial_array",
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
    "inset": {"object_name": "Cube", "face_indices": [0], "thickness": 0.25},
    "boolean_op": {"input_a": "A", "input_b": "B", "operation": "union", "output_name": "BoolResult"},
    "add_edge_loop": {"object_name": "Cube", "edge_indices": [0]},
    "merge_vertices": {"object_name": "Cube"},
    "triangulate_mesh": {"object_name": "Cube"},
    "cleanup_mesh": {"object_name": "Cube"},
    "combine_meshes": {"object_names": ["A", "B"], "new_name": "Combined"},
    "mirror": {"object_name": "Cube"},
    "loft_sections": {"sections": ["loop0", "loop1"], "output_name": "Hull"},
    "lathe_profile": {"profile": "profileCurve", "output_name": "Lathed"},
    "radial_array": {
        "object_name": "Blade",
        "count": 3,
        "pivot": [0, 0, 0],
        "rotate_step": [0, 120, 0],
        "name_prefix": "blade",
    },
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
    ("boolean_op", {"input_a": "A", "input_b": "B", "operation": "union"}, "Missing required input: output_name"),
    ("combine_meshes", {"object_names": ["A"], "new_name": "C"}, "fewer than the minimum"),
    ("combine_meshes", {"object_names": ["A", "B"]}, "Missing required input: new_name"),
    ("mirror", {"object_name": "C", "axis": "w"}, "Value is not one of the allowed options"),
    ("radial_array", {"object_name": "B", "count": 1, "pivot": [0, 0, 0]}, "Number is below the minimum"),
    ("get_poly_count", {}, "Missing required input: object_name"),
]

#: One observed tool receipt per recipe that satisfies its ``output_contract``.
SAMPLE_RECEIPTS: dict[str, dict[str, Any]] = {
    "create_primitive": {
        "object_name": "MyCube",
        "primitive_type": "cube",
        "changed": True,
        "vertex_count": 8,
        "face_count": 6,
    },
    "bevel_edges": {"object_name": "Cube", "changed": True, "beveled_edges": [0, 1], "face_count": 14},
    "extrude_faces": {"object_name": "Cube", "changed": True, "extruded_faces": [0], "face_count": 10},
    "inset": {"object_name": "Cube", "changed": True, "inset_faces": [0], "face_count": 10},
    "boolean_op": {
        "object_name": "BoolResult",
        "operation": "union",
        "source_objects": ["A", "B"],
        "changed": True,
        "face_count": 24,
    },
    "add_edge_loop": {"object_name": "Cube", "changed": True, "cuts": 1, "edge_count": 16},
    "merge_vertices": {"object_name": "Cube", "changed": True, "vertices_removed": 2, "vertex_count": 6},
    "triangulate_mesh": {
        "object_name": "Cube",
        "changed": True,
        "triangle_count": 12,
        "non_triangle_face_count": 0,
    },
    "cleanup_mesh": {
        "object_name": "Cube",
        "changed": True,
        "removed": {"vertices": 2, "edges": 0, "faces": 0},
        "face_count": 6,
    },
    "combine_meshes": {"object_name": "Combined", "changed": True, "source_count": 2, "face_count": 12},
    "mirror": {"object_name": "Cube", "axis": "x", "changed": True, "face_count": 12},
    "loft_sections": {"object_name": "Hull", "changed": True, "section_count": 2, "face_count": 64},
    "lathe_profile": {"object_name": "Lathed", "changed": True, "segment_count": 32, "face_count": 256},
    "radial_array": {
        "source": "Blade",
        "objects": ["blade_01", "blade_02", "blade_03"],
        "verified_count": 3,
        "changed": True,
    },
    "auto_uv": {
        "object_name": "Cube",
        "uv_set": "map1",
        "uv_count": 128,
        "uv_digest": "a" * 64,
        "changed": True,
        "margin": 0.001,
    },
    "uv_project": {
        "object_name": "Cube",
        "uv_set": "map1",
        "uv_count": 24,
        "projection": "planar",
        "axis": "z",
        "changed": True,
    },
    "assign_material": {"object_name": "Cube", "material_name": "mat", "slot_index": 0, "changed": True},
    "set_pivot": {"object_name": "Cube", "position": [0, 0, 0], "changed": True},
    "get_poly_count": {
        "object_name": "Cube",
        "vertex_count": 8,
        "edge_count": 12,
        "face_count": 6,
        "triangle_count": 12,
        "changed": False,
    },
}


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


@pytest.fixture(scope="module")
def pack_document() -> dict[str, Any]:
    """Load the raw RECIPES.yaml document, including pack-level routing metadata."""
    document = yaml_loads(RECIPES_PATH.read_text(encoding="utf-8"))
    assert isinstance(document, dict)
    return document


@pytest.fixture(scope="module")
def adapter_catalog() -> dict[str, Any]:
    return json.loads(ADAPTER_CATALOG_PATH.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def handlers() -> dict[str, Any]:
    """Register the recipes__* tools against the real pack and return handlers."""
    metadata = _make_metadata(str(MESH_OPS_SKILL_DIR))
    server = MagicMock()
    collected: dict[str, Any] = {}
    server.registry = MagicMock()
    server.register_handler.side_effect = lambda name, fn: collected.__setitem__(name, fn)
    register_recipes_tools(server, skills=[metadata], dcc_name="any")
    return collected


def _make_metadata(skill_path: str) -> MagicMock:
    md = MagicMock()
    md.name = "mesh-ops"
    md.skill_path = skill_path
    md.metadata = {"dcc-mcp": {"recipes": "RECIPES.yaml"}}
    return md


def _published_tool_names(catalog: dict[str, Any], adapter: str) -> set[str]:
    """Fully-qualified tool names an adapter publishes across its mesh-ops skills."""
    published: set[str] = set()
    for skill_slug, tools in catalog["adapters"][adapter]["skills"].items():
        prefix = skill_slug.replace("-", "_")
        published.update(f"{prefix}__{tool}" for tool in tools)
    return published


def _inputs_with_defaults(recipe: dict[str, Any], inputs: dict[str, Any]) -> dict[str, Any]:
    """Apply published schema defaults under caller-supplied inputs."""
    properties = recipe["inputs_schema"].get("properties", {})
    merged = {name: spec["default"] for name, spec in properties.items() if "default" in spec}
    merged.update(inputs)
    return merged


def _materialize(value: Any, inputs: dict[str, Any]) -> Any:
    """Resolve a ``${param}`` placeholder against validated inputs."""
    if isinstance(value, str) and value.startswith("${") and value.endswith("}"):
        return inputs[value[2:-1]]
    return value


def _materialized_arguments(step: dict[str, Any], inputs: dict[str, Any]) -> dict[str, Any]:
    return {name: _materialize(value, inputs) for name, value in step["arguments"].items()}


def _residual_placeholders(arguments: dict[str, Any]) -> list[str]:
    return [
        name
        for name, value in arguments.items()
        if isinstance(value, str) and value.startswith("${") and value.endswith("}")
    ]


# ── Pack structure ────────────────────────────────────────────────────────


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
def test_recipe_publishes_a_schema_output_contract(
    recipes_by_name: dict[str, dict[str, Any]],
    recipe_name: str,
) -> None:
    """`output_contract` must be a Draft 2020-12 schema, not an untyped label."""
    contract = recipes_by_name[recipe_name]["output_contract"]
    assert isinstance(contract, dict), f"{recipe_name} publishes a non-schema output_contract: {contract!r}"
    Draft202012Validator.check_schema(contract)
    assert contract.get("type") == "object"
    assert contract.get("required"), f"{recipe_name} contract can never fail without required fields"


# ── Dispatch routing ──────────────────────────────────────────────────────


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_recipe_steps_dispatch_canonical_verbs(
    recipes_by_name: dict[str, dict[str, Any]],
    pack_document: dict[str, Any],
    recipe_name: str,
) -> None:
    """Steps use the canonical namespace and every verb has a routing entry.

    Asserting ``tool == f"mesh_ops__{recipe_name}"`` would be a tautology; the
    meaningful invariant is that the verb is canonical *and* declared in
    ``tool_routing`` so an adapter can resolve it.
    """
    routed = pack_document["tool_routing"]["tools"]
    steps = recipes_by_name[recipe_name]["steps"]
    assert steps, f"{recipe_name} has no steps"
    for step in steps:
        tool = step["tool"]
        assert tool.startswith("mesh_ops__"), f"{recipe_name} step tool '{tool}' must use the canonical namespace"
        assert tool.split("mesh_ops__", 1)[1] in routed, f"{recipe_name} dispatches unrouted verb '{tool}'"
        assert isinstance(step.get("arguments"), dict)


def test_tool_routing_covers_every_recipe(
    recipes_by_name: dict[str, dict[str, Any]],
    pack_document: dict[str, Any],
) -> None:
    routed = pack_document["tool_routing"]["tools"]
    assert set(EXPECTED_RECIPES) <= set(routed), f"unrouted recipes: {sorted(set(EXPECTED_RECIPES) - set(routed))}"
    assert set(routed) >= {
        step["tool"].split("mesh_ops__", 1)[1] for recipe in recipes_by_name.values() for step in recipe["steps"]
    }


def test_tool_routing_declares_every_adapter(
    pack_document: dict[str, Any],
    adapter_catalog: dict[str, Any],
) -> None:
    routing = pack_document["tool_routing"]
    assert set(routing["adapters"]) == set(adapter_catalog["adapters"])
    assert routing["scripting_fallback"] == adapter_catalog["scripting_fallback"]
    for verb, per_adapter in routing["tools"].items():
        assert set(per_adapter) == set(routing["adapters"]), f"{verb} routing does not cover every adapter"


@pytest.mark.parametrize("adapter", ["blender", "houdini", "maya", "3dsmax"])
def test_routed_tools_are_published_by_their_adapter(
    pack_document: dict[str, Any],
    adapter_catalog: dict[str, Any],
    adapter: str,
) -> None:
    """routed_tools ⊆ published_tools for every adapter, per issued #2259 review."""
    published = _published_tool_names(adapter_catalog, adapter)
    for verb, per_adapter in pack_document["tool_routing"]["tools"].items():
        tool = per_adapter.get(adapter)
        if tool is None:
            continue
        assert tool in published, f"{adapter} does not publish '{tool}' routed by '{verb}'"


def test_every_routed_verb_has_at_least_one_published_adapter(
    pack_document: dict[str, Any],
) -> None:
    routing = pack_document["tool_routing"]
    for verb, per_adapter in routing["tools"].items():
        assert any(per_adapter.get(adapter) for adapter in routing["adapters"]), (
            f"'{verb}' is routed to no published tool on any adapter"
        )


def test_adapter_recipe_aliases_resolve_to_real_adapter_recipes(
    pack_document: dict[str, Any],
    adapter_catalog: dict[str, Any],
) -> None:
    """Aliases keep a verb search unambiguous when an adapter ships its own pack."""
    aliases = pack_document["adapter_recipe_aliases"]
    for adapter, mapping in aliases.items():
        published_recipes = adapter_catalog["adapters"][adapter]["recipes"]
        for canonical, adapter_local in mapping.items():
            assert canonical in EXPECTED_RECIPES, f"'{canonical}' is not a recipe in this pack"
            assert adapter_local in published_recipes, f"{adapter} publishes no recipe '{adapter_local}'"
    # The ambiguous query called out in #2259 must be declared.
    assert aliases["maya"]["loft_sections"] == "loft_hull_from_sections"


# ── Placeholder materialization ───────────────────────────────────────────


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_step_placeholders_resolve_to_declared_inputs(
    recipes_by_name: dict[str, dict[str, Any]],
    recipe_name: str,
) -> None:
    """Every ``${param}`` names an input that is required or carries a default.

    Core returns steps verbatim and applies no schema defaults, so a
    placeholder pointing at an optional input without a default could never be
    materialized.
    """
    recipe = recipes_by_name[recipe_name]
    properties = recipe["inputs_schema"]["properties"]
    required = set(recipe["inputs_schema"].get("required") or [])
    for step in recipe["steps"]:
        for name, value in step["arguments"].items():
            if not (isinstance(value, str) and value.startswith("${") and value.endswith("}")):
                continue
            param = value[2:-1]
            assert param in properties, f"{recipe_name} step argument '{name}' references unknown input '{param}'"
            assert param in required or "default" in properties[param], (
                f"{recipe_name} placeholder '${{{param}}}' is neither required nor defaulted"
            )


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_apply_plan_materializes_without_residual_placeholders(
    recipes_by_name: dict[str, dict[str, Any]],
    handlers: dict[str, Any],
    recipe_name: str,
) -> None:
    """recipes__apply returns a plan a caller can dispatch verbatim."""
    applied = handlers["recipes__apply"](
        json.dumps({"skill": "mesh-ops", "recipe": recipe_name, "inputs": VALID_INPUTS[recipe_name]}),
    )
    assert applied["success"] is True, applied
    inputs = _inputs_with_defaults(recipes_by_name[recipe_name], VALID_INPUTS[recipe_name])
    for step in applied["context"]["steps"]:
        arguments = _materialized_arguments(step, inputs)
        assert _residual_placeholders(arguments) == [], f"{recipe_name} step left unresolved placeholders: {arguments}"


# ── Input contracts ───────────────────────────────────────────────────────


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


@pytest.mark.parametrize("projection", ["planar", "cylindrical", "spherical", "cube"])
@pytest.mark.parametrize("axis", ["x", "y", "z"])
def test_uv_project_accepts_every_axis_for_every_projection(
    recipes_by_name: dict[str, dict[str, Any]],
    projection: str,
    axis: str,
) -> None:
    """Axis is a real degree of freedom: a cylindrical wrap around X is not a Z wrap."""
    recipe = recipes_by_name["uv_project"]
    errors = validate_recipe_inputs(recipe, {"object_name": "C", "projection": projection, "axis": axis})
    assert errors == [], f"{projection}/{axis} rejected: {errors}"


# ── Output contracts ──────────────────────────────────────────────────────


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_output_contract_accepts_a_valid_receipt(
    recipes_by_name: dict[str, dict[str, Any]],
    recipe_name: str,
) -> None:
    contract = recipes_by_name[recipe_name]["output_contract"]
    receipt = SAMPLE_RECEIPTS[recipe_name]
    Draft202012Validator(contract).validate(receipt)
    # The same check must work on Python 3.7 hosts with no jsonschema installed.
    assert _RecipeSchemaValidator(contract).validate(receipt) == []


@pytest.mark.parametrize("recipe_name", EXPECTED_RECIPES)
def test_output_contract_rejects_a_no_op_result(
    recipes_by_name: dict[str, dict[str, Any]],
    recipe_name: str,
) -> None:
    """A tool that reports success without touching the mesh must fail the contract."""
    contract = recipes_by_name[recipe_name]["output_contract"]
    receipt = SAMPLE_RECEIPTS[recipe_name]
    flipped = {**receipt, "changed": not receipt["changed"]}
    with pytest.raises(ValidationError):
        Draft202012Validator(contract).validate(flipped)
    assert _RecipeSchemaValidator(contract).validate(flipped)


def test_auto_uv_output_contract_fails_on_a_mesh_without_uvs(
    recipes_by_name: dict[str, dict[str, Any]],
) -> None:
    """#2259 acceptance: the UV recipe's output contract fails without UVs."""
    contract = recipes_by_name["auto_uv"]["output_contract"]
    receipt = SAMPLE_RECEIPTS["auto_uv"]

    Draft202012Validator(contract).validate(receipt)
    assert _RecipeSchemaValidator(contract).validate(receipt) == []

    no_uvs = {**receipt, "uv_count": 0}
    unreported = {key: value for key, value in receipt.items() if key != "uv_count"}
    undigested = {**receipt, "uv_digest": "not-a-digest"}
    for invalid in (no_uvs, unreported, undigested):
        with pytest.raises(ValidationError):
            Draft202012Validator(contract).validate(invalid)
        assert _RecipeSchemaValidator(contract).validate(invalid), f"core validator passed {invalid}"


def test_uv_project_output_contract_fails_on_a_mesh_without_uvs(
    recipes_by_name: dict[str, dict[str, Any]],
) -> None:
    contract = recipes_by_name["uv_project"]["output_contract"]
    receipt = SAMPLE_RECEIPTS["uv_project"]
    Draft202012Validator(contract).validate(receipt)

    no_uvs = {**receipt, "uv_count": 0}
    unreported = {key: value for key, value in receipt.items() if key != "uv_count"}
    off_axis = {**receipt, "axis": "w"}
    for invalid in (no_uvs, unreported, off_axis):
        with pytest.raises(ValidationError):
            Draft202012Validator(contract).validate(invalid)
        assert _RecipeSchemaValidator(contract).validate(invalid), f"core validator passed {invalid}"


def test_read_only_recipe_rejects_a_mutating_result(
    recipes_by_name: dict[str, dict[str, Any]],
) -> None:
    """get_poly_count must not report `changed: true`."""
    contract = recipes_by_name["get_poly_count"]["output_contract"]
    receipt = SAMPLE_RECEIPTS["get_poly_count"]
    assert receipt["changed"] is False
    Draft202012Validator(contract).validate(receipt)
    with pytest.raises(ValidationError):
        Draft202012Validator(contract).validate({**receipt, "changed": True})


# ── MCP tool surface ──────────────────────────────────────────────────────


def test_list_recipe_entries_serves_structured_pack() -> None:
    md = _make_metadata(str(MESH_OPS_SKILL_DIR))
    entries = list_recipe_entries(md)
    assert [entry["name"] for entry in entries] == EXPECTED_RECIPES
    assert all(entry["provenance"]["format"] == "recipe-pack" for entry in entries)
    assert find_recipe_entry(md, "bevel_edges") is not None


def test_recipes_tools_serve_cross_adapter_search(handlers: dict[str, Any]) -> None:
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
    assert applied["context"]["output_contract"]["required"] == [
        "object_name",
        "changed",
        "beveled_edges",
        "face_count",
    ]


@pytest.mark.parametrize(("query", "expected"), [("loft", "loft_sections"), ("radial", "radial_array")])
def test_search_returns_the_cross_adapter_recipe(handlers: dict[str, Any], query: str, expected: str) -> None:
    """#2259 acceptance: a verb search returns the recipe on a `dcc: any` entry."""
    found = handlers["recipes__search"](json.dumps({"query": query}))
    assert found["success"] is True
    match = next((entry for entry in found["context"]["recipes"] if entry["name"] == expected), None)
    assert match is not None, f"search '{query}' did not return '{expected}'"
    assert match["dcc"] == "any"
    assert match["steps"], f"'{expected}' published an empty step plan"
