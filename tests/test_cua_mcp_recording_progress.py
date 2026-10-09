"""Native-frozen sample evidence contracts, without an encoder or Host mock."""

from __future__ import annotations

from copy import deepcopy
import hashlib
import json
from pathlib import Path

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_contracts import typed_equal
from dcc_mcp_core.host.cua_mcp_recording_progress import RECORDING_PROGRESS_KEY
from dcc_mcp_core.host.cua_mcp_recording_progress import compare_media_sample_progress
from dcc_mcp_core.host.cua_mcp_recording_progress import media_sample_progress
from dcc_mcp_core.host.cua_mcp_recording_progress import recording_progress_descriptor
from dcc_mcp_core.host.cua_mcp_recording_progress import require_recording_progress_capability

FIXTURES = Path(__file__).parent / "fixtures/cua_recording_progress"
CONTRACT = json.loads((FIXTURES / "CONTRACT-FIXTURES.json").read_text(encoding="utf-8"))


@pytest.mark.parametrize("case", CONTRACT["wire"], ids=lambda case: case["name"])
def test_native_wire_fixture(case):
    assert (media_sample_progress(case["value"]) is not None) is case["valid"]


@pytest.mark.parametrize("case", CONTRACT["comparison"], ids=lambda case: case["name"])
def test_native_comparison_fixture(case):
    assert compare_media_sample_progress(case["previous"], case["current"]) == case["expected"]


@pytest.mark.parametrize(
    "filename,sha256",
    [
        ("CAPABILITY-DESCRIPTOR.json", "90e540e134339c6e32eedf9934c5f4e368505aeda426fbad72300ebb89885aa9"),
        ("CONTRACT-FIXTURES.json", "45b3898cfe970665da9c17aa9a363e78220d260c712c3c6abd94d822b6b60db4"),
        ("MEDIA-SAMPLE-PROGRESS.schema.json", "8251009f150d118f2acb646157baeabe013fc77b150a142a9dcb219f35732b3d"),
    ],
)
def test_native_fixture_bytes_are_preserved(filename, sha256):
    assert hashlib.sha256((FIXTURES / filename).read_bytes()).hexdigest() == sha256


def test_descriptor_includes_exact_frozen_closed_result_schema():
    descriptor = json.loads((FIXTURES / "CAPABILITY-DESCRIPTOR.json").read_text(encoding="utf-8"))
    schema = json.loads((FIXTURES / "MEDIA-SAMPLE-PROGRESS.schema.json").read_text(encoding="utf-8"))
    assert typed_equal(recording_progress_descriptor(), descriptor)
    assert typed_equal(descriptor["media_sample_progress_schema"], schema)
    require_recording_progress_capability({"experimental": {RECORDING_PROGRESS_KEY: descriptor}})


@pytest.mark.parametrize(
    "fault", ["missing_schema", "boolean_minimum", "loose_object", "extra", "encoded", "wrong_path"]
)
def test_inexact_descriptor_fails_closed(fault):
    descriptor = recording_progress_descriptor()
    if fault == "missing_schema":
        descriptor.pop("media_sample_progress_schema")
    elif fault == "boolean_minimum":
        descriptor["media_sample_progress_schema"]["properties"]["media_samples_admitted"]["minimum"] = False
    elif fault == "loose_object":
        descriptor["media_sample_progress_schema"]["additionalProperties"] = True
    elif fault == "extra":
        descriptor["extra"] = True
    elif fault == "encoded":
        descriptor["sample_semantics"] = "encoded_frames"
    else:
        descriptor["recording_state_path"] = "video.file_size"
    with pytest.raises(CuaCliError) as error:
        require_recording_progress_capability({"experimental": {RECORDING_PROGRESS_KEY: descriptor}})
    assert error.value.code == "unsupported"


@pytest.mark.parametrize("capabilities", [{}, {"experimental": None}, {"experimental": {}}, {"experimental": []}])
def test_missing_capability_fails_closed(capabilities):
    with pytest.raises(CuaCliError):
        require_recording_progress_capability(capabilities)


def test_parser_and_descriptor_do_not_share_mutable_state():
    current = {"recording_interval_id": "A", "media_samples_admitted": 1, "latest_admitted_source_sequence": 0}
    parsed = media_sample_progress(current)
    parsed["media_samples_admitted"] = 7
    assert current["media_samples_admitted"] == 1
    descriptor = recording_progress_descriptor()
    old = deepcopy(descriptor)
    descriptor["media_sample_progress_schema"]["properties"]["media_samples_admitted"]["maximum"] = 0
    assert recording_progress_descriptor() == old
