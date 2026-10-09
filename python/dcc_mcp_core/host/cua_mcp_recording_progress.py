"""Native media sample admission evidence, independent of recording health."""

from __future__ import annotations

from copy import deepcopy
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_contracts import typed_equal

RECORDING_PROGRESS_KEY = "dcc-cua.recording-sample-progress"
RECORDING_PROGRESS_CONTRACT = "dcc-cua.recording-sample-progress.v1"
_U64_MAX = 18446744073709551615
_PROGRESS_FIELDS = frozenset({"recording_interval_id", "media_samples_admitted", "latest_admitted_source_sequence"})


def recording_progress_descriptor() -> dict[str, Any]:
    """Return the exact capability, including its closed result schema."""
    sequence = {"type": "integer", "minimum": 0, "maximum": _U64_MAX}
    return {
        "contract": RECORDING_PROGRESS_CONTRACT,
        "platform": "windows",
        "recording_state_path": "video.media_sample_progress",
        "sample_semantics": "openh264_media_sample_admission_with_source_provenance",
        "media_sample_progress_schema": {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": RECORDING_PROGRESS_CONTRACT,
            "type": "object",
            "additionalProperties": False,
            "required": ["latest_admitted_source_sequence", "media_samples_admitted", "recording_interval_id"],
            "properties": {
                "recording_interval_id": {"type": "string", "minLength": 1},
                "media_samples_admitted": dict(sequence),
                "latest_admitted_source_sequence": {**sequence, "type": ["integer", "null"]},
            },
            "allOf": [
                {
                    "if": {"properties": {"media_samples_admitted": {"const": 0}}},
                    "then": {"properties": {"latest_admitted_source_sequence": {"type": "null"}}},
                    "else": {"properties": {"latest_admitted_source_sequence": dict(sequence)}},
                }
            ],
        },
    }


def require_recording_progress_capability(capabilities: dict[str, Any]) -> None:
    """Reject unsupported progress contracts without guessing from a version."""
    experimental = capabilities.get("experimental")
    if not isinstance(experimental, dict) or not typed_equal(
        experimental.get(RECORDING_PROGRESS_KEY), recording_progress_descriptor()
    ):
        raise CuaCliError("unsupported", "The selected runtime lacks the exact recording progress contract.")


def media_sample_progress(value: Any) -> dict[str, Any] | None:
    """Parse only the closed admitted-sample tuple; file metadata is irrelevant."""
    if not isinstance(value, dict) or value.keys() != _PROGRESS_FIELDS:
        return None
    interval = value["recording_interval_id"]
    count = value["media_samples_admitted"]
    sequence = value["latest_admitted_source_sequence"]
    if not isinstance(interval, str) or not interval or type(count) is not int or not 0 <= count <= _U64_MAX:
        return None
    if count == 0:
        if sequence is not None:
            return None
    elif type(sequence) is not int or not 0 <= sequence <= _U64_MAX:
        return None
    return deepcopy(value)


def compare_media_sample_progress(previous: Any, current: Any) -> str:
    """Compare one interval; callers must independently bind the task and source."""
    before, after = media_sample_progress(previous), media_sample_progress(current)
    if after is None:
        return "unavailable"
    if before is None or before["recording_interval_id"] != after["recording_interval_id"]:
        return "baseline_required"
    count_before, count_after = before["media_samples_admitted"], after["media_samples_admitted"]
    source_before, source_after = before["latest_admitted_source_sequence"], after["latest_admitted_source_sequence"]
    if count_after < count_before or (
        source_before is not None and source_after is not None and source_after < source_before
    ):
        return "regressed"
    if count_after > count_before:
        return "advanced"
    return "stalled" if source_before == source_after else "unavailable"
