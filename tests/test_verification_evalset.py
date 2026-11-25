"""Tests for the deterministic offline evaluation set contract."""

from __future__ import annotations

from dcc_mcp_core import VERDICT_INCONCLUSIVE
from dcc_mcp_core import VERDICT_PASS
from dcc_mcp_core import BehaviorVerifier
from dcc_mcp_core import EvalCase
from dcc_mcp_core import classify_image_stats
from dcc_mcp_core import compute_image_stats
from dcc_mcp_core import run_eval_set
from dcc_mcp_core import validate_scene_vs_spec


def _scene():
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


def _spec():
    return {
        "required_parts": ["body", "rotor_main"],
        "required_hierarchy": ["root/body"],
        "required_materials": ["mat_metal", "mat_camo"],
        "min_uv_coverage": 0.9,
        "allow_non_manifold": False,
        "euler_max_abs_degrees": 360.0,
    }


def _behavior_exact():
    verifier = BehaviorVerifier("exact-count")
    verifier.exact("part_count", 3, 3)
    return verifier.report()


def _behavior_tolerance():
    verifier = BehaviorVerifier("numeric-band")
    verifier.within("mean_luma", 0.51, 0.5, tolerance=0.02)
    return verifier.report()


def _image_case():
    pixels = bytearray()
    for y in range(8):
        for x in range(8):
            value = 255 if (x + y) % 2 == 0 else 0
            pixels.extend((value, value, value))
    flags = classify_image_stats(compute_image_stats(8, 8, bytes(pixels)))
    return {
        "checks": [
            {"name": "image_not_blank", "passed": not flags.is_blank, "expected": False, "actual": flags.is_blank}
        ]
    }


def test_five_seed_cases_are_deterministic_and_headless() -> None:
    cases = (
        EvalCase("behavior-exact", "Exact structural count", _behavior_exact, tags=("behavior",)),
        EvalCase("behavior-tolerance", "Numeric tolerance band", _behavior_tolerance, tags=("behavior",)),
        EvalCase(
            "scene-spec-pass",
            "Scene satisfies the written modeling spec",
            lambda: validate_scene_vs_spec(_scene(), _spec()),
            tags=("scene",),
        ),
        EvalCase("image-not-blank", "Checkerboard capture contains contrast", _image_case, tags=("image",)),
        EvalCase(
            "scene-materials-present",
            "Scene material inventory remains complete",
            lambda: {"checks": [{"name": "materials", "passed": len(_scene()["materials"]) == 2}]},
            tags=("scene",),
        ),
    )

    first = run_eval_set(cases, generated_at="2026-01-01T00:00:00Z", clock=lambda: 0.0)
    second = run_eval_set(cases, generated_at="2026-01-01T00:00:00Z", clock=lambda: 0.0)

    assert len(first.runs) == 5
    assert all(run.verdict == VERDICT_PASS for run in first.runs)
    assert first.pass_rate == 1.0
    assert first.dumps() == second.dumps()


def test_grader_error_is_inconclusive_and_reported() -> None:
    report = run_eval_set(
        [EvalCase("broken", "Broken fixture", lambda: (_ for _ in ()).throw(RuntimeError("fixture broken")))],
        generated_at="2026-01-01T00:00:00Z",
        clock=lambda: 0.0,
    )

    assert report.runs[0].verdict == VERDICT_INCONCLUSIVE
    assert report.runs[0].reason == "fixture broken"
    assert report.failure_histogram == {"fixture broken": 1, "grader_error": 1}


def test_unknown_scene_evidence_is_inconclusive() -> None:
    report = run_eval_set(
        [
            EvalCase(
                "partial-scene",
                "Partial state export",
                lambda: validate_scene_vs_spec({"partial": True}, _spec()),
            )
        ],
        clock=lambda: 0.0,
    )

    assert report.runs[0].verdict == VERDICT_INCONCLUSIVE
    assert report.runs[0].reason == "verification returned unknown"
