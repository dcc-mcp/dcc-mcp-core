"""Owner opt-in progress verification through the real Core client code."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import replace

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_recording_progress import RECORDING_PROGRESS_CONTRACT
from dcc_mcp_core.host.cua_mcp_recording_progress import RECORDING_PROGRESS_KEY
from dcc_mcp_core.host.cua_mcp_recording_progress import recording_progress_descriptor
from dcc_mcp_core.server import UiControlRecordingOptions
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import pixel_response
from test_cua_mcp_recording import recording as recording


def sample(process, count=1, sequence=0, interval="interval-A"):
    process.state["video"]["media_sample_progress"] = {
        "recording_interval_id": interval,
        "media_samples_admitted": count,
        "latest_admitted_source_sequence": sequence,
    }


@pytest.fixture
def strict_recording(recording):
    config, process, launches = recording
    config = replace(config, recording=replace(config.recording, require_progress=True))
    proof = {"host_connection_id": "connection-A", "connected_host_capabilities": [RECORDING_PROGRESS_CONTRACT]}
    process.mutate_initialize = lambda raw: raw["capabilities"].update(
        experimental={RECORDING_PROGRESS_KEY: recording_progress_descriptor()}
    )
    process.mutate_open = lambda raw: raw.update(recording_output_dir=process.output, **deepcopy(proof))
    process.mutate_recording = lambda raw: raw["task_context"].update(deepcopy(proof))
    instance = pixel_response()["observation"]["capture_provenance"]["native_instance"]
    process.state["video"].update(
        backend="embedded-openh264",
        first_encoded_frame={
            "capture_provenance": {
                "kind": "native_exact_window",
                "source": "wgc",
                **TARGET,
                "native_instance": deepcopy(instance),
                "stream_id": 7,
            }
        },
    )
    process.state["source"]["stream_id"] = 7
    sample(process)
    return config, process, launches


@pytest.fixture
def strict_client(strict_recording):
    config, process, _ = strict_recording
    owned = client(config)
    try:
        yield owned, process
    finally:
        if not owned._closed:
            owned.stop()


@pytest.mark.parametrize("value", [None, 0, 1, 0.0, 1.0, "true", [], {}])
def test_require_progress_is_strict_boolean(tmp_path, value):
    with pytest.raises(TypeError, match="require_progress"):
        UiControlRecordingOptions(str(tmp_path), require_progress=value)


def test_progress_option_is_appended_and_default_off(tmp_path, recording):
    assert UiControlRecordingOptions(str(tmp_path), 65).require_progress is False
    assert UiControlRecordingOptions(str(tmp_path), 65, True).require_progress is True
    config, _, _ = recording
    owned = client(config)
    assert "recording_progress" not in owned.recording_start()
    owned.stop()


def test_missing_descriptor_refuses_before_start_task(strict_recording):
    config, process, _ = strict_recording
    process.mutate_initialize = lambda raw: None
    with pytest.raises(CuaCliError) as failure:
        client(config)
    assert failure.value.code == "unsupported"
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


@pytest.mark.parametrize(
    "proof",
    [
        {},
        {"host_connection_id": "A", "connected_host_capabilities": []},
        {"host_connection_id": "A", "connected_host_capabilities": ["wrong.contract"]},
        {"host_connection_id": True, "connected_host_capabilities": [RECORDING_PROGRESS_CONTRACT]},
        {"host_connection_id": "A", "connected_host_capabilities": RECORDING_PROGRESS_CONTRACT},
        {"host_connection_id": "A", "connected_host_capabilities": [True, RECORDING_PROGRESS_CONTRACT]},
    ],
)
def test_actual_host_proof_is_required_after_startup(strict_recording, proof):
    config, process, _ = strict_recording
    process.mutate_open = lambda raw: raw.update(recording_output_dir=process.output, **proof)
    with pytest.raises(CuaCliError) as failure:
        client(config)
    assert failure.value.code == "unsupported"
    assert sum(r.get("params", {}).get("name") == "stop_task" for r in process.requests) == 1
    assert not any(
        r.get("params", {}).get("arguments", {}).get("method") == "recording_start" for r in process.requests
    )


def test_start_baselines_then_only_real_admission_advances(strict_client):
    owned, process = strict_client
    result = owned.recording_start()
    assert result["recording_progress"]["status"] == "baseline_required"
    assert result["recording_progress"]["satisfied"] is False
    with pytest.raises(CuaCliError) as stalled:
        owned.recording_state()
    assert stalled.value.native_evidence["recording"]["recording_progress"]["status"] == "stalled"
    sample(process, 2, 0)
    result = owned.recording_state()
    assert result["recording_progress"]["satisfied"] is True
    assert result["recording_progress"]["current"]["latest_admitted_source_sequence"] == 0
    assert "encoded" not in result["recording_progress"]["sample_semantics"]


@pytest.mark.parametrize(
    "replacement",
    [
        None,
        {},
        {"encoded_frames": 99},
        {
            "recording_interval_id": "A",
            "media_samples_admitted": True,
            "latest_admitted_source_sequence": 0,
        },
    ],
)
def test_missing_or_malformed_sample_cannot_use_file_progress(strict_client, replacement):
    owned, process = strict_client
    owned.recording_start()
    process.state["video"].update(media_sample_progress=replacement, size=999999, mtime=999999, segments=[1, 2])
    with pytest.raises(CuaCliError) as failure:
        owned.recording_state()
    assert failure.value.code == "recording_progress_unavailable"
    assert failure.value.native_evidence["recording"]["recording_progress"]["status"] == "unavailable"


@pytest.mark.parametrize("change", ["interval", "stream"])
def test_new_interval_or_stream_requires_new_baseline(strict_client, change):
    owned, process = strict_client
    owned.recording_start()
    sample(process, 50, 100, "interval-B" if change == "interval" else "interval-A")
    if change == "stream":
        process.state["source"]["stream_id"] = 8
        process.state["video"]["first_encoded_frame"]["capture_provenance"]["stream_id"] = 8
    with pytest.raises(CuaCliError) as baseline:
        owned.recording_state()
    assert baseline.value.native_evidence["recording"]["recording_progress"]["status"] == "baseline_required"
    process.state["video"]["media_sample_progress"]["media_samples_admitted"] += 1
    assert owned.recording_state()["recording_progress"]["satisfied"]


@pytest.mark.parametrize("fault", ["connection", "marker", "task", "instance", "target"])
def test_binding_changes_never_satisfy_progress(strict_client, fault):
    owned, process = strict_client
    owned.recording_start()
    original = process.mutate_recording

    def mutate(raw):
        original(raw)
        if fault == "connection":
            raw["task_context"]["host_connection_id"] = "connection-B"
        elif fault == "marker":
            raw["task_context"]["connected_host_capabilities"] = []
        elif fault == "task":
            raw["task_context"]["task_id"] = "other-task"
        elif fault == "instance":
            raw["result"]["video"]["first_encoded_frame"]["capture_provenance"]["native_instance"][
                "window_thread_id"
            ] += 1
        else:
            raw["result"]["video"]["first_encoded_frame"]["capture_provenance"]["window_handle"] += 1

    process.mutate_recording = mutate
    sample(process, 5, 9)
    with pytest.raises(CuaCliError):
        owned.recording_state()
    assert owned._closed


@pytest.mark.parametrize("status", ["paused", "degraded", "failed", "stopped"])
def test_health_remains_independent_of_counter_growth(strict_client, status):
    owned, process = strict_client
    owned.recording_start()
    sample(process, 2, 1)
    process.state.update(
        status=status, healthy=False, issues=["native_not_ready"], active=status not in {"failed", "stopped"}
    )
    with pytest.raises(CuaCliError) as failure:
        owned.recording_state()
    evidence = failure.value.native_evidence["recording"]
    assert evidence["status"] == status and evidence["healthy"] is False
    assert evidence["recording_progress"]["status"] == "advanced"
    assert evidence["recording_progress"]["satisfied"] is False


def test_failed_rpc_does_not_advance_baseline(strict_client):
    owned, process = strict_client
    owned.recording_start()
    before = deepcopy(owned._recording.progress._previous)
    process.recording_error = True
    with pytest.raises(CuaCliError):
        owned.recording_state()
    assert owned._recording.progress._previous == before
    process.recording_error = False
    sample(process, 2, 1)
    assert owned.recording_state()["recording_progress"]["satisfied"]


def test_stop_and_reopen_never_inherit_progress(strict_client):
    owned, process = strict_client
    owned.recording_start()
    sample(process, 2, 1)
    stopped = owned.recording_stop()
    assert stopped["recording_progress"]["status"] == "advanced"
    assert stopped["recording_progress"]["satisfied"] is False
    assert owned._recording.progress._previous is None
    sample(process, 20, 99, "interval-B")
    assert owned.recording_start()["recording_progress"]["status"] == "baseline_required"
    with pytest.raises(CuaCliError):
        owned.recording_state()
    owned.stop()
    assert owned._recording.progress._previous is None


def test_bundled_skill_route_preserves_strict_owner_progress(strict_recording):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = strict_recording
    bridge = HostExecutionBridge()

    def call(name):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "strict-video-route"},
            skill_name="ui-control",
            trusted_ui_control_runtime=config,
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
        )

    try:
        assert call("recording_start")["success"] is True
        stalled = call("recording_state")
        assert stalled["success"] is False
        assert stalled["context"]["recording"]["recording_progress"]["status"] == "stalled"
        sample(process, 2, 0)
        advanced = call("recording_state")
        assert advanced["success"] is True
        assert advanced["context"]["recording"]["recording_progress"]["satisfied"] is True
        stopped = call("recording_stop")
        assert stopped["success"] is True
        assert stopped["context"]["recording"]["recording_progress"]["satisfied"] is False
        assert call("stop_computer_use")["success"] is True
    finally:
        bridge.shutdown_script_execution()
    assert sum(r.get("params", {}).get("name") == "start_task" for r in process.requests) == 1
