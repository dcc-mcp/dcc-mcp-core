"""Deterministic, payload-bounded verification toolset (issue #2261).

Pure-Python, standard-library-only contract that turns captures and scene
state into machine-checkable signals — image statistics, deterministic pixel
metrics, and scene-vs-spec validation — without shipping pixels to a model.

The released-product production acceptance matrix lives here too:
:mod:`dcc_mcp_core.verification.acceptance` owns the shared, versioned record
and its fail-closed evaluator, and
:mod:`dcc_mcp_core.verification.acceptance_fixtures` supplies editor-free
engine fixtures for probing it.

The behavior verification contract (core#2269) is layered on top:

* :mod:`dcc_mcp_core.verification.schemas` — versioned state-export schemas
  (``dcc-mcp/anim-curves@1``, ``dcc-mcp/rig-state@1``, ``dcc-mcp/sim-status@1``,
  ``dcc-mcp/graph-state@1``), a dependency-free structural validator, and
  animation-curve sampling helpers.
* :mod:`dcc_mcp_core.verification.assertions` — the dual-layer assertion
  library (exact structural counts, tolerance-band numerics, existence and
  resolution checks) with both fail-fast helpers and a collecting
  :class:`BehaviorVerifier` that reports a pass rate.
* :mod:`dcc_mcp_core.verification.lint` — the declaration-lint pairing rule
  that flags write verbs without a paired read-only state export.

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
from dcc_mcp_core.verification.assertions import AssertionFailure
from dcc_mcp_core.verification.assertions import BehaviorReport
from dcc_mcp_core.verification.assertions import BehaviorVerifier
from dcc_mcp_core.verification.assertions import Check
from dcc_mcp_core.verification.assertions import assert_exact
from dcc_mcp_core.verification.assertions import assert_exists
from dcc_mcp_core.verification.assertions import assert_in_band
from dcc_mcp_core.verification.assertions import assert_resolution
from dcc_mcp_core.verification.assertions import assert_state_schema
from dcc_mcp_core.verification.assertions import assert_within
from dcc_mcp_core.verification.image_stats import ImageStats
from dcc_mcp_core.verification.image_stats import ImageStatsFlags
from dcc_mcp_core.verification.image_stats import ImageStatsThresholds
from dcc_mcp_core.verification.image_stats import classify_image_stats
from dcc_mcp_core.verification.image_stats import compute_image_stats
from dcc_mcp_core.verification.image_stats import decode_ppm
from dcc_mcp_core.verification.lint import DeclarationFinding
from dcc_mcp_core.verification.lint import find_unpaired_write_verbs
from dcc_mcp_core.verification.lint import lint_tool_table
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
from dcc_mcp_core.verification.schemas import SCHEMA_ANIM_CURVES
from dcc_mcp_core.verification.schemas import SCHEMA_GRAPH_STATE
from dcc_mcp_core.verification.schemas import SCHEMA_RIG_STATE
from dcc_mcp_core.verification.schemas import SCHEMA_SIM_STATUS
from dcc_mcp_core.verification.schemas import SCHEMA_VERSIONS
from dcc_mcp_core.verification.schemas import SchemaValidationError
from dcc_mcp_core.verification.schemas import sample_curve
from dcc_mcp_core.verification.schemas import schema_document
from dcc_mcp_core.verification.schemas import schema_names
from dcc_mcp_core.verification.schemas import validate_state_export
from dcc_mcp_core.verification.schemas import value_at

__all__ = [
    "EVIDENCE_SOURCES",
    "LEVELS",
    "MAX_GODOT_CONFIG_VERSION",
    "MIN_GODOT_CONFIG_VERSION",
    "MIN_UNITY_EDITOR_MAJOR",
    "MIN_UNREAL_ENGINE_MAJOR",
    "PRODUCTION_ACCEPTANCE_V1_SCHEMA_VERSION",
    "SCHEMA_ANIM_CURVES",
    "SCHEMA_GRAPH_STATE",
    "SCHEMA_RIG_STATE",
    "SCHEMA_SIM_STATUS",
    "SCHEMA_VERSIONS",
    "STATUSES",
    "TRANSITION_CHAIN",
    "AcceptanceEvaluation",
    "AcceptanceFinding",
    "AcceptanceValidationError",
    "AssertionFailure",
    "BehaviorReport",
    "BehaviorVerifier",
    "Check",
    "DeclarationFinding",
    "ImageStats",
    "ImageStatsFlags",
    "ImageStatsThresholds",
    "SceneSpecFailure",
    "SceneSpecResult",
    "SchemaValidationError",
    "ahash_64",
    "assert_exact",
    "assert_exists",
    "assert_in_band",
    "assert_resolution",
    "assert_state_schema",
    "assert_within",
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
    "find_unpaired_write_verbs",
    "godot_project",
    "godot_version_probe",
    "hamming_distance",
    "lint_tool_table",
    "make_level",
    "normalize_digest",
    "phash_64",
    "production_acceptance_v1_json_schema",
    "recompute_sha256",
    "record_from_catalog_entry",
    "sample_curve",
    "sanitize_evidence_link",
    "schema_document",
    "schema_names",
    "silhouette_iou",
    "sobel_edges",
    "ssim",
    "unity_project_version",
    "unreal_project",
    "validate_acceptance_schema",
    "validate_scene_vs_spec",
    "validate_state_export",
    "value_at",
    "verify_release_sha256",
]
