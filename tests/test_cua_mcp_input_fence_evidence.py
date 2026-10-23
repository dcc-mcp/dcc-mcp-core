"""Rejected native input evidence is bounded metadata, never an authority token."""

from __future__ import annotations

from copy import deepcopy

import pytest

from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_errors import safe_failure_evidence


def diagnostic():
    return {
        "schema_version": 1,
        "authorizes_input": False,
        "target_process_id": 42,
        "target_window_handle": 77,
        "point": [1461, 1060],
        "unobscured": False,
        "point_proof_complete": True,
        "point_visible": False,
        "hit_window_handle": 91,
        "hit_process_id": 100,
        "hit_root_window_handle": 90,
        "hit_is_target_or_child": False,
        "redacted_rectangles": [[1200, 260, 200, 100]],
        "redaction_count": 1,
        "redactions_complete": True,
    }


def payload(value):
    return {"details": {"phase": "input_fence", "native_input_fence": value, "input_sent": "none"}}


def test_rejected_fence_facts_survive_without_mutating_source_or_granting_input():
    source = payload(diagnostic())
    original = deepcopy(source)
    error = OwnedCuaMcpError("target_occluded", "input rejected", source)
    assert error.code == "target_occluded"
    assert error.native_evidence == original
    assert source == original
    error.native_evidence["details"]["native_input_fence"]["point"][0] = 0
    error.native_evidence["details"]["native_input_fence"]["redacted_rectangles"][0][0] = 0
    assert source == original
    assert error.native_evidence["details"]["native_input_fence"]["authorizes_input"] is False


def test_unavailable_point_evidence_stays_explicitly_unknown():
    value = diagnostic()
    value.update(
        point=None,
        point_proof_complete=False,
        point_visible=None,
        hit_window_handle=None,
        hit_process_id=None,
        hit_root_window_handle=None,
        hit_is_target_or_child=None,
        redacted_rectangles=[],
        redaction_count=0,
        redactions_complete=False,
    )
    assert safe_failure_evidence(payload(value)) == payload(value)


def test_bounded_truncated_redactions_preserve_count_without_claiming_completeness():
    value = diagnostic()
    value.update(
        redacted_rectangles=[[-(2**31), 2**31 - 1, 0, 0]] * 16, redaction_count=2**32 - 1, redactions_complete=False
    )
    value.update(target_process_id=2**32 - 1, target_window_handle=2**64 - 1, point=[-(2**31), 2**31 - 1])
    assert safe_failure_evidence(payload(value)) == payload(value)


@pytest.mark.parametrize("field", list(diagnostic()))
def test_incomplete_diagnostic_is_rejected_as_a_unit(field):
    value = diagnostic()
    del value[field]
    assert safe_failure_evidence(payload(value)) == {"details": {"phase": "input_fence", "input_sent": "none"}}


@pytest.mark.parametrize(
    ("field", "invalid"),
    [
        ("schema_version", True),
        ("schema_version", 2),
        ("authorizes_input", True),
        ("authorizes_input", 0),
        ("target_process_id", 0),
        ("target_process_id", 2**32),
        ("target_process_id", True),
        ("target_window_handle", -1),
        ("target_window_handle", 2**64),
        ("hit_process_id", 2**32),
        ("hit_window_handle", False),
        ("hit_root_window_handle", -1),
        ("point", [1]),
        ("point", [False, 2]),
        ("point", [2**31, 0]),
        ("point", [0, -(2**31) - 1]),
        ("point", {"x": 1, "y": 2}),
        ("unobscured", 0),
        ("point_proof_complete", "false"),
        ("point_visible", 1),
        ("hit_is_target_or_child", "false"),
        ("redacted_rectangles", [[0, 0, 1, 1]] * 17),
        ("redacted_rectangles", [[0, 0, 1]]),
        ("redacted_rectangles", [[0, 0, 1, True]]),
        ("redacted_rectangles", [[0, 0, 1, 2**31]]),
        ("redaction_count", True),
        ("redaction_count", -1),
        ("redaction_count", 2**32),
        ("redaction_count", 0),
        ("redaction_count", 2),
        ("redactions_complete", None),
        ("window_title", "PRIVATE_WINDOW_TITLE"),
        ("image_base64", "PRIVATE_IMAGE_BYTES"),
    ],
)
def test_malformed_or_content_bearing_diagnostic_is_rejected_as_a_unit(field, invalid):
    value = diagnostic()
    value[field] = invalid
    assert safe_failure_evidence(payload(value)) == {"details": {"phase": "input_fence", "input_sent": "none"}}


def test_diagnostic_only_projects_at_its_exact_failure_location():
    assert safe_failure_evidence({"native_input_fence": diagnostic()}) == {}
    assert safe_failure_evidence({"details": {"capture": {"native_input_fence": diagnostic()}}}) == {
        "details": {"capture": {}}
    }
    assert safe_failure_evidence({"details": {"authorizes_input": True}}) == {"details": {}}


@pytest.mark.parametrize("parent", ["native_outcome", "task_context"])
def test_other_top_level_parents_cannot_publish_a_fence_diagnostic(parent):
    source = {**payload(diagnostic()), parent: {"native_input_fence": diagnostic()}}
    assert safe_failure_evidence(source) == {**payload(diagnostic()), parent: {}}
