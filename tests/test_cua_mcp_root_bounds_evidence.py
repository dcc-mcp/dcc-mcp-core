"""Retain typed failed-root facts without widening capture or input authority."""

from copy import deepcopy

import pytest

from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_errors import safe_failure_evidence


def failure_payload():
    return {
        "details": {
            "phase": "evidence_dispatch",
            "capture": {
                "stage": "publication_validation",
                "reason": "root_bounds_invalid",
                "target_process_id": 42,
                "target_window_handle": 77,
                "target_bounds": [-16, -16, 3872, 2312],
                "blocker_process_id": 100,
                "blocker_window_handle": 91,
                "root_bounds_failure": {
                    "root_role": "above_target_root",
                    "proof_target_root_window_handle": 77,
                    "dwm_raw_rect_edges": [10, 20, 10, 30],
                    "dwm_classification": "zero_area",
                    "visible": True,
                    "cloaked": 0,
                    "win32_read_after_dwm_rejection": True,
                    "win32_raw_rect_edges": [10, 20, 30, 40],
                    "win32_classification": "positive",
                    "win32_os_error": None,
                    "zero_area_status_mismatch": True,
                },
            },
        },
    }


@pytest.mark.parametrize(
    ("edges", "classification"),
    [([10, 20, 10, 30], "zero_area"), ([30, 20, 10, 40], "inverted"), ([-(2**31), 0, 2**31 - 1, 1], "overflow")],
)
def test_owned_error_preserves_actual_failed_root_without_substituting_target(edges, classification):
    payload = failure_payload()
    root = payload["details"]["capture"]["root_bounds_failure"]
    root.update(dwm_raw_rect_edges=edges, dwm_classification=classification)
    original = deepcopy(payload)
    error = OwnedCuaMcpError("capture_failed", "pixels discarded", payload)
    assert error.native_evidence == original
    assert payload == original
    assert error.code == "capture_failed"


def test_missing_win32_followup_stays_unknown_not_positive():
    payload = failure_payload()
    root = payload["details"]["capture"]["root_bounds_failure"]
    root.update(win32_raw_rect_edges=None, win32_classification=None, win32_os_error=-5, zero_area_status_mismatch=None)
    assert safe_failure_evidence(payload) == payload


@pytest.mark.parametrize(
    ("key", "invalid"),
    [
        ("dwm_classification", "PRIVATE_VALUE"),
        ("root_role", "PRIVATE_VALUE"),
        ("win32_classification", "PRIVATE_VALUE"),
        ("dwm_raw_rect_edges", [0, 1, 2]),
        ("dwm_raw_rect_edges", [False, 0, 1, 2]),
        ("dwm_raw_rect_edges", [-(2**31) - 1, 0, 1, 2]),
    ],
)
def test_root_diagnostic_rejects_unknown_enums_and_invalid_rects(key, invalid):
    payload = failure_payload()
    root = payload["details"]["capture"]["root_bounds_failure"]
    root[key] = invalid
    root["window_title"] = "PRIVATE_TITLE"
    root["image_base64"] = "PRIVATE_IMAGE"
    projected = safe_failure_evidence(payload)["details"]["capture"]["root_bounds_failure"]
    assert key not in projected
    assert "PRIVATE" not in repr(projected)
    assert projected["proof_target_root_window_handle"] == 77


def test_legacy_capture_error_does_not_fabricate_root_diagnostic():
    payload = failure_payload()
    del payload["details"]["capture"]["root_bounds_failure"]
    assert safe_failure_evidence(payload) == payload
