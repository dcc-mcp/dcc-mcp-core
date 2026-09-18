"""Deterministic, payload-bounded verification toolset (issue #2261).

Pure-Python, standard-library-only contract that turns captures and scene
state into machine-checkable signals — image statistics, deterministic pixel
metrics, and scene-vs-spec validation — without shipping pixels to a model.

The released-product production acceptance matrix lives here too:
:mod:`dcc_mcp_core.verification.acceptance` owns the shared, versioned record
and its fail-closed evaluator, and
:mod:`dcc_mcp_core.verification.acceptance_fixtures` supplies editor-free
engine fixtures for probing it.

Everything here is import-safe on Python 3.7 (Maya 2022 / Blender 2.83) and
ships in both the native and ``py37-lite`` wheels.
"""

from __future__ import annotations

from dcc_mcp_core.verification.acceptance import EVIDENCE_SOURCES
from dcc_mcp_core.verification.acceptance import LEVELS
from dcc_mcp_core.verification.acceptance import PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION
from dcc_mcp_core.verification.acceptance import STATUSES
from dcc_mcp_core.verification.acceptance import TRANSITION_CHAIN
from dcc_mcp_core.verification.acceptance import AcceptanceEvaluation
from dcc_mcp_core.verification.acceptance import AcceptanceFinding
from dcc_mcp_core.verification.acceptance import AcceptanceValidationError
from dcc_mcp_core.verification.acceptance import build_report
from dcc_mcp_core.verification.acceptance import dumps_report
from dcc_mcp_core.verification.acceptance import evaluate_acceptance
from dcc_mcp_core.verification.acceptance import evaluate_many
from dcc_mcp_core.verification.acceptance import make_level
from dcc_mcp_core.verification.acceptance import normalize_digest
from dcc_mcp_core.verification.acceptance import production_acceptance_v1_json_schema
from dcc_mcp_core.verification.acceptance import recompute_sha256
from dcc_mcp_core.verification.acceptance import record_from_catalog_entry
from dcc_mcp_core.verification.acceptance import sanitize_evidence_link
from dcc_mcp_core.verification.acceptance import validate_acceptance_schema
from dcc_mcp_core.verification.acceptance import verify_release_sha256
from dcc_mcp_core.verification.acceptance_fixtures import MAX_GODOT_CONFIG_VERSION
from dcc_mcp_core.verification.acceptance_fixtures import MIN_GODOT_CONFIG_VERSION
from dcc_mcp_core.verification.acceptance_fixtures import MIN_UNITY_EDITOR_MAJOR
from dcc_mcp_core.verification.acceptance_fixtures import MIN_UNREAL_ENGINE_MAJOR
from dcc_mcp_core.verification.acceptance_fixtures import godot_project
from dcc_mcp_core.verification.acceptance_fixtures import godot_version_probe
from dcc_mcp_core.verification.acceptance_fixtures import unity_project_version
from dcc_mcp_core.verification.acceptance_fixtures import unreal_project
from dcc_mcp_core.verification.image_stats import ImageStats
from dcc_mcp_core.verification.image_stats import ImageStatsFlags
from dcc_mcp_core.verification.image_stats import ImageStatsThresholds
from dcc_mcp_core.verification.image_stats import classify_image_stats
from dcc_mcp_core.verification.image_stats import compute_image_stats
from dcc_mcp_core.verification.image_stats import decode_ppm
from dcc_mcp_core.verification.pixel_metrics import ahash_64
from dcc_mcp_core.verification.pixel_metrics import delta_e_2000
from dcc_mcp_core.verification.pixel_metrics import dhash_64
from dcc_mcp_core.verification.pixel_metrics import edge_density
from dcc_mcp_core.verification.pixel_metrics import hamming_distance
from dcc_mcp_core.verification.pixel_metrics import phash_64
from dcc_mcp_core.verification.pixel_metrics import silhouette_iou
from dcc_mcp_core.verification.pixel_metrics import sobel_edges
from dcc_mcp_core.verification.pixel_metrics import ssim
from dcc_mcp_core.verification.scene_spec import SceneSpecFailure
from dcc_mcp_core.verification.scene_spec import SceneSpecResult
from dcc_mcp_core.verification.scene_spec import validate_scene_vs_spec

__all__ = [
    "AcceptanceEvaluation",
    "AcceptanceFinding",
    "AcceptanceValidationError",
    "EVIDENCE_SOURCES",
    "ImageStats",
    "ImageStatsFlags",
    "ImageStatsThresholds",
    "LEVELS",
    "MAX_GODOT_CONFIG_VERSION",
    "MIN_GODOT_CONFIG_VERSION",
    "MIN_UNITY_EDITOR_MAJOR",
    "MIN_UNREAL_ENGINE_MAJOR",
    "PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION",
    "STATUSES",
    "SceneSpecFailure",
    "SceneSpecResult",
    "TRANSITION_CHAIN",
    "ahash_64",
    "build_report",
    "classify_image_stats",
    "compute_image_stats",
    "decode_ppm",
    "delta_e_2000",
    "dhash_64",
    "dumps_report",
    "edge_density",
    "evaluate_acceptance",
    "evaluate_many",
    "godot_project",
    "godot_version_probe",
    "hamming_distance",
    "make_level",
    "normalize_digest",
    "phash_64",
    "production_acceptance_v1_json_schema",
    "recompute_sha256",
    "record_from_catalog_entry",
    "sanitize_evidence_link",
    "silhouette_iou",
    "sobel_edges",
    "ssim",
    "unity_project_version",
    "unreal_project",
    "validate_acceptance_schema",
    "validate_scene_vs_spec",
    "verify_release_sha256",
]
