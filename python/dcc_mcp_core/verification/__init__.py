"""Deterministic, payload-bounded verification toolset (issue #2261).

Pure-Python, standard-library-only contract that turns captures and scene
state into machine-checkable signals — image statistics, deterministic pixel
metrics, and scene-vs-spec validation — without shipping pixels to a model.

Everything here is import-safe on Python 3.7 (Maya 2022 / Blender 2.83) and
ships in both the native and ``py37-lite`` wheels.
"""

from __future__ import annotations

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
    "ImageStats",
    "ImageStatsFlags",
    "ImageStatsThresholds",
    "SceneSpecFailure",
    "SceneSpecResult",
    "ahash_64",
    "classify_image_stats",
    "compute_image_stats",
    "decode_ppm",
    "delta_e_2000",
    "dhash_64",
    "edge_density",
    "hamming_distance",
    "phash_64",
    "silhouette_iou",
    "sobel_edges",
    "ssim",
    "validate_scene_vs_spec",
]
