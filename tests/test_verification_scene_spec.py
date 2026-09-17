"""Tests for scene-vs-spec validation (issue #2261)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import SceneSpecResult
from dcc_mcp_core import validate_scene_vs_spec


def _scene(**overrides):
    scene = {
        "parts": ["body", "rotor_main", "tail"],
        "hierarchy": ["root", "root/body", "root/rotor_main"],
        "materials": ["mat_metal", "mat_camo"],
        "meshes": [
            {"name": "body", "uv_sets": 1, "material": "mat_metal", "is_non_manifold": False},
            {"name": "rotor_main", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
            {"name": "tail", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
        ],
        "objects": [{"name": "body", "euler": [0.0, 0.0, 0.0]}],
    }
    scene.update(overrides)
    return scene


def _spec(**overrides):
    spec = {
        "required_parts": ["body", "rotor_main"],
        "required_hierarchy": ["root/body"],
        "required_materials": ["mat_metal", "mat_camo"],
        "min_uv_coverage": 0.9,
        "allow_non_manifold": False,
        "euler_max_abs_degrees": 360.0,
    }
    spec.update(overrides)
    return spec


def _failure_checks(result: SceneSpecResult) -> set:
    return {failure.check for failure in result.failures}


def test_fully_compliant_scene_passes() -> None:
    result = validate_scene_vs_spec(_scene(), _spec())
    assert result.passed is True
    assert result.failures == []
    assert len(result.checks) == 6
    assert all(check["passed"] for check in result.checks)


def test_uv_coverage_catches_meshes_without_uvs() -> None:
    scene = _scene(
        meshes=[
            {"name": "body", "uv_sets": 1, "material": "mat_metal", "is_non_manifold": False},
            {"name": "rotor_main", "uv_sets": 0, "material": "mat_camo", "is_non_manifold": False},
            {"name": "tail", "uv_sets": 0, "material": "mat_camo", "is_non_manifold": False},
        ]
    )
    result = validate_scene_vs_spec(scene, _spec())
    assert result.passed is False
    assert "uv_coverage" in _failure_checks(result)
    failure = next(f for f in result.failures if f.check == "uv_coverage")
    assert failure.detail["coverage"] == pytest.approx(1 / 3)
    assert failure.detail["meshes_without_uvs"] == ["rotor_main", "tail"]


def test_missing_part_is_reported() -> None:
    result = validate_scene_vs_spec(_scene(parts=["body"]), _spec())
    assert "parts" in _failure_checks(result)


def test_missing_hierarchy_path_is_reported() -> None:
    result = validate_scene_vs_spec(_scene(hierarchy=["root"]), _spec())
    assert "hierarchy" in _failure_checks(result)


def test_unbound_material_is_reported() -> None:
    scene = _scene(
        meshes=[
            {"name": "body", "uv_sets": 1, "material": None, "is_non_manifold": False},
            {"name": "rotor_main", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
            {"name": "tail", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
        ]
    )
    result = validate_scene_vs_spec(scene, _spec())
    assert "materials" in _failure_checks(result)


def test_non_manifold_mesh_is_reported() -> None:
    scene = _scene(
        meshes=[
            {"name": "body", "uv_sets": 1, "material": "mat_metal", "is_non_manifold": True},
            {"name": "rotor_main", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
            {"name": "tail", "uv_sets": 1, "material": "mat_camo", "is_non_manifold": False},
        ]
    )
    result = validate_scene_vs_spec(scene, _spec())
    assert "non_manifold" in _failure_checks(result)


def test_euler_out_of_bounds_is_reported() -> None:
    scene = _scene(objects=[{"name": "body", "euler": [720.0, 0.0, 0.0]}])
    result = validate_scene_vs_spec(scene, _spec(euler_max_abs_degrees=360.0))
    assert "euler" in _failure_checks(result)


def test_euler_non_finite_is_reported() -> None:
    scene = _scene(objects=[{"name": "body", "euler": [float("nan"), 0.0, 0.0]}])
    result = validate_scene_vs_spec(scene, _spec())
    assert "euler" in _failure_checks(result)


def test_result_to_dict_is_json_safe() -> None:
    result = validate_scene_vs_spec(_scene(parts=["body"]), _spec())
    payload = result.to_dict()
    assert payload["passed"] is False
    assert isinstance(payload["failures"], list)
    assert payload["failures"][0]["check"] == "parts"


def test_non_mapping_inputs_are_rejected() -> None:
    with pytest.raises(TypeError, match="mapping"):
        validate_scene_vs_spec([], _spec())
    with pytest.raises(TypeError, match="mapping"):
        validate_scene_vs_spec(_scene(), [])
