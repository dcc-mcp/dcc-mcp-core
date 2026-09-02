"""Deterministic capture response contract tests (issue #2261)."""

from __future__ import annotations

import base64

import pytest

from dcc_mcp_core import CaptureReturnMode
from dcc_mcp_core import CaptureTargetSpec
from dcc_mcp_core import build_capture_response

FRAME = b"deterministic-pixels"


def test_artifact_only_returns_bounded_metadata_without_pixels() -> None:
    result = build_capture_response(
        FRAME,
        width=16,
        height=9,
        format="png",
        backend="mock",
        target=CaptureTargetSpec.active_viewport(),
        return_mode=CaptureReturnMode.ARTIFACT_ONLY,
    )

    assert set(result) == {"artifact", "metadata"}
    assert "image" not in result
    assert "data" not in result
    assert result["metadata"] == {
        "dimensions": [16, 9],
        "bytes": len(FRAME),
        "sha256": "0ef02515ac75c95c2a3b82754173d5874c16fd75db3d6d04dc6fef3ea3cb8742",
        "format": "png",
        "backend": "mock",
        "target": "active_viewport",
    }
    assert result["artifact"]["uri"].startswith("artefact://sha256/")


def test_image_and_both_modes_have_expected_inline_payload() -> None:
    image = build_capture_response(
        FRAME,
        width=1,
        height=1,
        format="png",
        backend="mock",
        target="dcc_window",
        return_mode="image",
    )
    assert set(image) == {"image", "metadata"}
    assert base64.b64decode(image["image"]) == FRAME

    both = build_capture_response(
        FRAME,
        width=1,
        height=1,
        format="png",
        backend="mock",
        target="dcc_window",
        return_mode="both",
    )
    assert set(both) == {"artifact", "image", "metadata"}


def test_unchanged_hash_suppresses_pixels_deterministically() -> None:
    initial = build_capture_response(
        FRAME,
        width=1,
        height=1,
        format="png",
        backend="mock",
        target="scene_viewer",
        return_mode="both",
    )
    repeated = build_capture_response(
        FRAME,
        width=1,
        height=1,
        format="png",
        backend="mock",
        target="scene_viewer",
        return_mode="both",
        unchanged_if_hash=initial["metadata"]["sha256"],
    )
    assert set(repeated) == {"artifact", "metadata", "unchanged"}
    assert repeated["unchanged"] is True
    assert "image" not in repeated
    assert repeated["metadata"] == initial["metadata"]


def test_target_crop_and_invalid_values_are_explicit() -> None:
    target = CaptureTargetSpec.crop(1, 2, 30, 40)
    result = build_capture_response(
        FRAME,
        width=30,
        height=40,
        format="jpeg",
        backend="mock",
        target=target,
        return_mode="artifact_only",
    )
    assert result["metadata"]["target"] == "crop"
    assert result["metadata"]["crop"] == [1, 2, 30, 40]

    with pytest.raises(ValueError, match="target"):
        build_capture_response(FRAME, width=1, height=1, format="png", backend="mock", target="modal")
    with pytest.raises(ValueError, match="hidden"):
        CaptureTargetSpec("dcc_window", visible=False)
    with pytest.raises(ValueError, match="return_mode"):
        build_capture_response(FRAME, width=1, height=1, format="png", backend="mock", return_mode="pixels")
    with pytest.raises(ValueError, match="unchanged_if_hash"):
        build_capture_response(
            FRAME,
            width=1,
            height=1,
            format="png",
            backend="mock",
            unchanged_if_hash="not-a-sha256",
        )
