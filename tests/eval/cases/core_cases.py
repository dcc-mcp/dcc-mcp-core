"""Twenty deterministic modeling cases for the offline evaluation contract."""

from __future__ import annotations

from typing import Any
from typing import Dict
from typing import List

from dcc_mcp_core import SCHEMA_MODEL_STATE
from dcc_mcp_core import BehaviorVerifier
from dcc_mcp_core import EvalCase
from dcc_mcp_core import classify_image_stats
from dcc_mcp_core import compute_image_stats
from dcc_mcp_core import hamming_distance
from dcc_mcp_core import phash_64
from dcc_mcp_core import silhouette_iou
from dcc_mcp_core import ssim
from dcc_mcp_core import validate_scene_vs_spec
from dcc_mcp_core import validate_state_export


def _check(name: str, passed: bool, expected: Any = None, actual: Any = None) -> Dict[str, Any]:
    """Build one schema-compatible check record."""
    return {"name": name, "passed": passed, "expected": expected, "actual": actual}


def _result(checks: List[Dict[str, Any]], reason: str = "") -> Dict[str, Any]:
    result: Dict[str, Any] = {"checks": checks}
    if reason and not all(bool(check.get("passed")) for check in checks):
        result["verdict"] = "fail"
        result["reason"] = reason
    return result


def _behavior_exact() -> Dict[str, Any]:
    verifier = BehaviorVerifier("core-exact")
    verifier.exact("part_count", 3, 3)
    report = verifier.report()
    return {"checks": [check.to_dict() for check in report.checks]}


def _behavior_tolerance() -> Dict[str, Any]:
    verifier = BehaviorVerifier("core-tolerance")
    verifier.within("mean_luma", 0.51, 0.5, tolerance=0.02)
    report = verifier.report()
    return {"checks": [check.to_dict() for check in report.checks]}


def _scene() -> Dict[str, Any]:
    return {
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


def _spec() -> Dict[str, Any]:
    return {
        "required_parts": ["body", "rotor_main"],
        "required_hierarchy": ["root/body"],
        "required_materials": ["mat_metal", "mat_camo"],
        "min_uv_coverage": 0.9,
        "allow_non_manifold": False,
        "euler_max_abs_degrees": 360.0,
    }


def _scene_spec() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), _spec())
    return {"checks": [{"name": "scene_spec", "passed": result.status == "pass", "actual": result.status}]}


def _checkerboard(value: int = 255) -> bytes:
    pixels = bytearray()
    for y in range(8):
        for x in range(8):
            level = value if (x + y) % 2 == 0 else 0
            pixels.extend((level, level, level))
    return bytes(pixels)


def _image_stats() -> Dict[str, Any]:
    flags = classify_image_stats(compute_image_stats(8, 8, _checkerboard()))
    return _result([_check("image_not_blank", not flags.is_blank, False, flags.is_blank)])


def _model_state() -> Dict[str, Any]:
    return {
        "schema_name": SCHEMA_MODEL_STATE,
        "schema_version": 1,
        "partial": False,
        "unavailable": [],
        "parts": ["body", "rotor_main"],
        "hierarchy": ["root", "root/body"],
        "meshes": [
            {
                "name": "body",
                "uv_sets": 1,
                "material": "mat_metal",
                "is_non_manifold": False,
                "vertex_count": 128,
            }
        ],
        "objects": [{"name": "body", "transform": {}, "euler": [0.0, 0.0, 0.0]}],
    }


def _valid_model_state() -> Dict[str, Any]:
    validate_state_export(_model_state(), SCHEMA_MODEL_STATE)
    return _result([_check("model_state_schema", True, True, True)])


def _scene_hierarchy() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), {"required_hierarchy": ["root/body"]})
    return _result([_check("hierarchy", result.status == "pass", "pass", result.status)])


def _image_contrast() -> Dict[str, Any]:
    stats = compute_image_stats(8, 8, _checkerboard())
    return _result([_check("contrast", stats.stddev_luma > 0.0, ">0", stats.stddev_luma)])


def _identical_metrics() -> Dict[str, Any]:
    pixels = _checkerboard()
    first_hash = phash_64(8, 8, pixels)
    return _result(
        [
            _check("phash_distance", hamming_distance(first_hash, first_hash) == 0, 0, 0),
            _check("silhouette_iou", silhouette_iou([1, 0, 1], [1, 0, 1]) == 1.0, 1.0, 1.0),
        ]
    )


def _identical_ssim() -> Dict[str, Any]:
    values = [float(value) / 255.0 for value in _checkerboard(255)[::3]]
    score = ssim(values, values, 8, 8)
    return _result([_check("ssim", score == 1.0, 1.0, score)])


def _uv_coverage() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), {"min_uv_coverage": 1.0})
    return _result([_check("uv_coverage", result.status == "pass", "pass", result.status)])


def _manifold() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), {"allow_non_manifold": False})
    return _result([_check("non_manifold", result.status == "pass", "pass", result.status)])


def _euler() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), {"euler_max_abs_degrees": 360.0})
    return _result([_check("euler", result.status == "pass", "pass", result.status)])


def _materials() -> Dict[str, Any]:
    result = validate_scene_vs_spec(_scene(), {"required_materials": ["mat_metal", "mat_camo"]})
    return _result([_check("materials", result.status == "pass", "pass", result.status)])


def _missing_part_defect() -> Dict[str, Any]:
    scene = _scene()
    scene["parts"].remove("rotor_main")
    result = validate_scene_vs_spec(scene, {"required_parts": ["rotor_main"]})
    return _result(
        [_check("required_part_rotor_main", result.status == "pass", "pass", result.status)],
        "defect: required part readback is missing",
    )


def _uv_defect() -> Dict[str, Any]:
    scene = _scene()
    scene["meshes"][0]["uv_sets"] = 0
    result = validate_scene_vs_spec(scene, {"min_uv_coverage": 1.0})
    return _result(
        [_check("uv_coverage_floor", result.status == "pass", "pass", result.status)],
        "defect: UV coverage falls below the modeling quality floor",
    )


def _non_manifold_defect() -> Dict[str, Any]:
    scene = _scene()
    scene["meshes"][0]["is_non_manifold"] = True
    result = validate_scene_vs_spec(scene, {"allow_non_manifold": False})
    return _result(
        [_check("non_manifold_geometry", result.status == "pass", "pass", result.status)],
        "defect: non-manifold geometry is present in the model export",
    )


def _blank_capture_defect() -> Dict[str, Any]:
    flags = classify_image_stats(compute_image_stats(8, 8, bytes(8 * 8 * 3)))
    return _result(
        [_check("capture_not_blank", not flags.is_blank, False, flags.is_blank)],
        "defect: capture is blank and cannot provide visual evidence",
    )


def _similarity_defect() -> Dict[str, Any]:
    first = [float(value) / 255.0 for value in _checkerboard(255)[::3]]
    second = [0.0] * len(first)
    score = ssim(first, second, 8, 8)
    return _result(
        [_check("reference_similarity", score >= 0.95, ">=0.95", score)],
        "defect: rendered evidence is below the reference similarity threshold",
    )


CASES = (
    EvalCase("core-exact-count", "Exact structural count", _behavior_exact, tags=("behavior",)),
    EvalCase("core-tolerance-band", "Numeric tolerance band", _behavior_tolerance, tags=("behavior",)),
    EvalCase("core-scene-spec", "Scene matches the written spec", _scene_spec, tags=("scene",)),
    EvalCase("core-image-stats", "Capture contains visual contrast", _image_stats, tags=("image",)),
    EvalCase("core-model-state", "Model state export satisfies its schema", _valid_model_state, tags=("state",)),
    EvalCase("core-hierarchy", "Hierarchy readback is complete", _scene_hierarchy, tags=("scene",)),
    EvalCase("core-image-contrast", "Image standard deviation is positive", _image_contrast, tags=("image",)),
    EvalCase("core-identical-metrics", "Identical captures have zero distance", _identical_metrics, tags=("image",)),
    EvalCase("core-identical-ssim", "Identical captures have perfect SSIM", _identical_ssim, tags=("image",)),
    EvalCase("core-uv-coverage", "UV coverage meets the floor", _uv_coverage, tags=("scene",)),
    EvalCase("core-manifold", "Meshes are manifold", _manifold, tags=("scene",)),
    EvalCase("core-euler", "Object rotations are within bounds", _euler, tags=("scene",)),
    EvalCase("core-materials", "Materials are bound", _materials, tags=("scene",)),
    EvalCase("core-schema-repeat", "Model state schema is repeatable", _valid_model_state, tags=("state",)),
    EvalCase("core-spec-repeat", "Scene spec evaluation is repeatable", _scene_spec, tags=("scene",)),
    EvalCase("defect-missing-part", "Missing required part is discriminated", _missing_part_defect, tags=("defect",)),
    EvalCase("defect-low-uv-coverage", "Low UV coverage is discriminated", _uv_defect, tags=("defect",)),
    EvalCase("defect-non-manifold", "Non-manifold geometry is discriminated", _non_manifold_defect, tags=("defect",)),
    EvalCase("defect-blank-capture", "Blank capture is discriminated", _blank_capture_defect, tags=("defect",)),
    EvalCase(
        "defect-low-similarity", "Low reference similarity is discriminated", _similarity_defect, tags=("defect",)
    ),
)
