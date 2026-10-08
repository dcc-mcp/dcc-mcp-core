"""Actual public-MCP recording envelopes and bundled scripts without native UI."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import FrozenInstanceError
from dataclasses import replace
import json
from pathlib import Path
import queue
import subprocess
from typing import Any

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.server import UiControlRecordingOptions
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import VERSION
from test_cua_mcp_pixels import FakeMcpProcess
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import options
from test_cua_mcp_pixels import pixel_response


class RecordingProcess(FakeMcpProcess):
    def __init__(self, root: Path) -> None:
        super().__init__()
        self.output = str(root / "public-task")
        self.mutate_open = lambda raw: raw.update(recording_output_dir=self.output)
        self.mutate_catalog = lambda raw: raw["tools"][0]["inputSchema"]["properties"].update(
            allow_recording={"type": "boolean"}
        )
        self.mutate_recording = lambda raw: None
        self.mutate_stop = lambda raw: None
        self.recording_error = False
        self.waits = []
        self.state = {
            "backend": "native_pixels_video",
            "status": "active",
            "active": True,
            "healthy": True,
            "expected_components": ["video"],
            "issues": [],
            "trajectory_available": False,
            "trajectory": None,
            "video": {
                "active": True,
                "path": str(root / "public-task/video.mp4"),
                "manifest_path": str(root / "public-task/video.json"),
                "segments": [],
                "paused": False,
            },
            "source": {"active": True, "paused": False, "terminal_reason": None},
        }

    def write(self, data: bytes) -> None:
        request = json.loads(data)
        params = request.get("params", {})
        args = params.get("arguments", {})
        method = args.get("method")
        if request.get("method") != "tools/call" or not (
            params.get("name") == "stop_task" or method in {"recording_start", "recording_state", "recording_stop"}
        ):
            return super().write(data)
        self.requests.append(request)
        if self.drop_response:
            return
        if params["name"] == "stop_task":
            raw = {
                **pixel_response()["task_context"],
                "ok": True,
                "status": "stopped",
                "recording_output_dir": self.output,
                "cleanup": {
                    "type": "session_stopped",
                    "session_id": "mcp-public-task",
                    "success": True,
                    "active": False,
                    "cleanup_pending": False,
                    "cleanup_issues": [],
                },
            }
            self.mutate_stop(raw)
        else:
            assert args["task_id"] == "public-task" and args["params"] == {}
            raw = {
                "type": {"recording_start": "recording_started", "recording_stop": "recording_stopped"}.get(
                    method, method
                ),
                "session_id": "mcp-public-task",
                "task_context": pixel_response()["task_context"],
                "result": deepcopy(self.state),
            }
            if method == "recording_stop":
                raw["result"].update(status="stopped", active=False)
                raw["result"]["video"].update(active=False, finalized=True)
            if self.recording_error:
                raw = {
                    "type": "error",
                    "code": "capture_failed",
                    "message": "native finalization failed",
                    "task_context": pixel_response()["task_context"],
                }
            self.mutate_recording(raw)
        result = {
            "structuredContent": raw,
            "content": [],
            "isError": raw.get("ok") is False or raw.get("type") == "error",
        }
        self.lines.put((json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}) + "\n").encode())

    def wait(self, timeout: float) -> int:
        self.waits.append(timeout)
        return super().wait(timeout)


@pytest.fixture
def recording(tmp_path, monkeypatch):
    root = tmp_path / "recordings"
    root.mkdir()
    process = RecordingProcess(root)
    launches = []
    monkeypatch.setattr(
        "dcc_mcp_core.host.cua_mcp_transport.subprocess.Popen",
        lambda command, **kwargs: launches.append((command, kwargs)) or process,
    )
    return options(tmp_path, allowed_actions=(), recording=UiControlRecordingOptions(str(root))), process, launches


def test_recording_is_owner_frozen_optional_and_not_input(recording, monkeypatch):
    config, process, launches = recording
    monkeypatch.setenv("DCC_CUA_RECORDING_OUTPUT_ROOT", "ambient-must-not-authorize")
    owned = client(config)
    proposal = next(
        r["params"]["arguments"] for r in process.requests if r.get("params", {}).get("name") == "start_task"
    )
    assert proposal["allowed_methods"] == [
        "snapshot",
        "get_window_state",
        "recording_start",
        "recording_state",
        "recording_stop",
    ]
    assert proposal["allow_recording"] is True and proposal["allowed_actions"] == []
    assert launches[0][1]["env"]["DCC_CUA_RECORDING_OUTPUT_ROOT"] == config.recording.output_root
    assert not any(
        r.get("params", {}).get("arguments", {}).get("method") == "recording_start" for r in process.requests
    )
    with pytest.raises(FrozenInstanceError):
        config.recording.output_root = "other"
    started = owned.recording_start()
    assert started["trajectory_available"] is False and started["host_session_id"] == "mcp-public-task"
    stopped = owned.stop()
    assert stopped["cleanup"]["session_id"] == "mcp-public-task" and stopped["cleanup"]["success"] is True
    assert owned.stop() == stopped
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    assert 0 < process.waits[0] <= 65


def test_ambient_environment_cannot_enable_recording(recording, monkeypatch):
    config, _process, launches = recording
    monkeypatch.setenv("DCC_CUA_RECORDING_OUTPUT_ROOT", config.recording.output_root)
    owned = client(replace(config, recording=None))
    assert "DCC_CUA_RECORDING_OUTPUT_ROOT" not in launches[0][1]["env"]
    with pytest.raises(CuaCliError, match="owner did not grant"):
        owned.recording_start()
    owned.stop()


@pytest.mark.parametrize("fault", ["foreign_path", "false_video"])
def test_tool_call_cannot_redirect_or_request_trajectory(recording, fault):
    config, process, _ = recording
    owned = client(config)
    try:
        with pytest.raises(CuaCliError):
            owned.recording_start(
                **({"output_dir": config.recording.output_root} if fault == "foreign_path" else {"record_video": False})
            )
        assert not any(
            r.get("params", {}).get("arguments", {}).get("method") == "recording_start" for r in process.requests
        )
    finally:
        owned.stop()


@pytest.mark.parametrize(
    "status,issues",
    [("paused", ["video_paused"]), ("degraded", ["source_terminal"]), ("failed", ["video_finalization_failed"])],
)
def test_truthful_native_state_retains_partial_paths_and_no_trajectory(recording, status, issues):
    config, process, _ = recording
    owned = client(config)
    try:
        process.state.update(status=status, healthy=False, issues=issues)
        process.state["video"].update(finalized=False, paused=status == "paused")
        state = owned.recording_state()
        assert state["status"] == status and state["healthy"] is False
        assert state["video"]["path"].endswith("video.mp4") and state["video"]["finalized"] is False
        assert state["trajectory"] is None
    finally:
        owned.stop()


def test_real_stop_error_does_not_claim_finalized_and_state_remains_available(recording):
    config, process, _ = recording
    owned = client(config)
    try:
        owned.snapshot(max_depth=1, max_nodes=1)
        process.recording_error = True
        with pytest.raises(OwnedCuaMcpError, match="finalization failed") as error:
            owned.recording_stop()
        assert error.value.fresh_observation_required is True and owned._observation_id is None
        process.recording_error = False
        process.state.update(status="failed", healthy=False, issues=["video_finalization_failed"])
        assert owned.recording_state()["status"] == "failed"
    finally:
        owned.stop()


@pytest.mark.parametrize("fault", ["host_session", "task", "target", "path", "backend", "trajectory"])
def test_recording_identity_and_artifact_mismatch_fail_closed(recording, fault):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        if fault == "host_session":
            raw["session_id"] = "native-runtime-session"
        elif fault == "task":
            raw["task_context"]["task_id"] = "foreign-task"
        elif fault == "target":
            raw["task_context"]["target"]["process_id"] = 43
        elif fault == "path":
            raw["result"]["video"]["path"] = str(Path(config.recording.output_root) / "foreign.mp4")
        elif fault == "backend":
            raw["result"]["backend"] = "pure_mock"
        else:
            raw["result"]["trajectory_available"] = True

    process.mutate_recording = mutate
    with pytest.raises(CuaCliError):
        owned.recording_start()
    assert process.returncode == 0


@pytest.mark.parametrize("fault", ["missing", "session", "pending", "failed", "unknown", "path", "target"])
def test_cleanup_failure_or_uncertainty_is_retained_without_retry(recording, fault):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        if fault == "missing":
            raw.pop("cleanup")
        elif fault == "session":
            raw["cleanup"]["session_id"] = "foreign"
        elif fault == "pending":
            raw["cleanup"]["cleanup_pending"] = True
        elif fault in {"failed", "unknown"}:
            raw.update(ok=False, status="cleanup_" + fault)
            raw["cleanup"].update(
                success=False,
                cleanup_issues=[
                    {
                        "phase": "recording_stop",
                        "code": "capture_failed",
                        "message": "private text is not public evidence",
                    }
                ],
            )
        elif fault == "path":
            raw["recording_output_dir"] = config.recording.output_root
        else:
            raw["target"]["window_handle"] = 501

    process.mutate_stop = mutate
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as second:
        owned.stop()
    assert second.value is first.value and process.returncode == 0
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    if fault in {"failed", "unknown"}:
        assert first.value.native_evidence["cleanup_status"] == "cleanup_" + fault
        assert first.value.native_evidence["cleanup"]["cleanup_issues"] == [
            {"phase": "recording_stop", "code": "capture_failed"}
        ]
        assert "private text" not in json.dumps(first.value.native_evidence)


def test_opt_in_stop_uses_owner_timeout_and_forced_exit_never_passes(recording, monkeypatch):
    config, process, _ = recording
    owned = client(config)
    timeouts = []
    original = owned._transport.rpc

    def rpc(method, params, *, timeout=None):
        timeouts.append(timeout)
        return original(method, params, timeout=timeout)

    monkeypatch.setattr(owned._transport, "rpc", rpc)
    process.stdin.close = lambda: None

    def wait(timeout):
        process.waits.append(timeout)
        if process.returncode is None:
            raise subprocess.TimeoutExpired("owned pipe fixture", timeout)
        return process.returncode

    process.wait = wait
    with pytest.raises(CuaCliError, match="forced termination") as first:
        owned.stop()
    with pytest.raises(CuaCliError) as second:
        owned.stop()
    assert first.value is second.value and timeouts == [65]
    assert first.value.native_evidence["process_cleanup_failed"] is True


def test_actual_bundled_recording_scripts_use_same_task_and_keep_stop_ack(recording):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = recording
    bridge = HostExecutionBridge()

    def call(name):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "video-route"},
            skill_name="ui-control",
            trusted_ui_control_runtime=config,
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
        )

    try:
        started = call("recording_start")
        assert started["success"] is True and started["context"]["output_dir"] == process.output
        assert call("recording_state")["context"]["recording"]["trajectory_available"] is False
        assert call("recording_stop")["context"]["recording"]["video"]["finalized"] is True
        stopped = call("stop_computer_use")
        assert stopped["success"] is True and stopped["context"]["native_cleanup"]["cleanup"]["success"] is True
    finally:
        bridge.shutdown_script_execution()
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "start_task"]) == 1


@pytest.mark.parametrize(
    "change", [{"cleanup_timeout_seconds": 60}, {"cleanup_timeout_seconds": 66}, {"output_root": "relative"}]
)
def test_recording_options_reject_unbounded_or_unowned_values(tmp_path, change):
    with pytest.raises(ValueError):
        UiControlRecordingOptions(**{"output_root": str(tmp_path), **change})


def test_stop_timeout_is_unknown_and_not_retried(recording, monkeypatch):
    config, process, _ = recording
    owned = client(config)
    waits = []
    process.drop_response = True

    def timeout(*, timeout):
        waits.append(timeout)
        raise queue.Empty

    monkeypatch.setattr(owned._transport._responses, "get", timeout)
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as second:
        owned.stop()
    assert first.value is second.value and first.value.code == "cleanup_unknown"
    assert first.value.native_evidence["cleanup_status"] == "cleanup_unknown"
    assert len(waits) == 1 and 64 < waits[0] <= 65
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1


def test_nonzero_process_exit_keeps_native_ack_but_cannot_pass(recording):
    config, process, _ = recording
    owned = client(config)

    def finish():
        process.returncode = 7
        process.lines.put(b"")

    process.stdin.close = finish
    process.wait = lambda timeout: 7
    with pytest.raises(OwnedCuaMcpError) as failure:
        owned.stop()
    assert failure.value.code == "cleanup_failed"
    assert failure.value.native_evidence["cleanup"]["success"] is True
    assert failure.value.native_evidence["process_cleanup_failed"] is True
    with pytest.raises(CuaCliError):
        owned.stop()


def test_schema_capability_is_required_only_for_explicit_recording(recording):
    config, process, _ = recording
    process.mutate_catalog = lambda raw: None
    with pytest.raises(CuaCliError, match="public bounded-task"):
        client(config)
    assert process.returncode == 0
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


def test_public_stop_failure_retains_conclusion_and_legacy_path_stays_required(recording):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = recording
    bridge = HostExecutionBridge()

    def call(name, runtime_config=config):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "stop-failure"},
            skill_name="ui-control",
            trusted_ui_control_runtime=runtime_config,
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
        )

    try:
        legacy = call("recording_start", None)
        assert legacy["success"] is False and legacy["error"] == "invalid_request"
        assert not process.requests
        assert call("recording_start")["success"] is True

        def fail(raw):
            raw.update(ok=False, status="cleanup_failed")
            raw["cleanup"].update(success=False, cleanup_issues=[{"phase": "recording_stop", "code": "capture_failed"}])

        process.mutate_stop = fail
        first = call("stop_computer_use")
        second = call("stop_computer_use")
        assert first["success"] is False and second["success"] is False
        assert first["context"]["cleanup_status"] == second["context"]["cleanup_status"] == "cleanup_failed"
        assert first["context"]["cleanup"]["cleanup_issues"] == [{"phase": "recording_stop", "code": "capture_failed"}]
        assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.parametrize("method", ["recording_start", "recording_state", "recording_stop"])
def test_public_disabled_owner_ceiling_does_not_launch_a_child(recording, method):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, launches = recording
    bridge = HostExecutionBridge()
    try:
        result = bridge.execute_script(
            str(SCRIPTS / (method + ".py")),
            {},
            skill_name="ui-control",
            trusted_ui_control_runtime=replace(config, recording=None),
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
        )
        assert result["success"] is False and result["error"] == "unsupported_action"
        assert not launches and not process.requests
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.parametrize("pending", [False, True])
def test_typed_native_cleanup_retains_actual_partial_video_and_source_summary(recording, pending):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        raw.update(ok=False, status="cleanup_unknown" if pending else "cleanup_failed")
        ack = raw["cleanup"]
        ack.update(
            success=False,
            cleanup_issues=[{"phase": "live_observation_stop", "code": "capture_failed"}],
            recording_video={
                "active": False,
                "finalized": False,
                "path": process.output + "/video.mp4",
                "current_partial": process.output + "/partial.mp4",
                "segment_paths": [process.output + "/segment-1.mp4"],
                "error_code": "capture_failed",
                "capture_sidecar": {
                    "path": process.output + "/capture.jsonl",
                    "sha256": "a" * 64,
                    "records": 7,
                    "frames": 3,
                    "finalized": False,
                },
            },
            live_observation={
                "active": False,
                "cleanup_complete": not pending,
                "cleanup_pending": pending,
                "stream_id": 9,
            },
        )
        if pending:
            original = deepcopy(ack)
            original["cleanup_pending"] = True
            ack.clear()
            ack.update(
                type="session_stopped",
                session_id="mcp-public-task",
                success=False,
                active=None,
                cleanup_pending=True,
                host_response=original,
            )

    process.mutate_stop = mutate
    with pytest.raises(OwnedCuaMcpError) as failed:
        owned.stop()
    actual = failed.value.native_evidence["cleanup"]
    if pending:
        actual = actual["host_response"]
    assert actual["recording_video"]["finalized"] is False
    assert actual["recording_video"]["current_partial"] == process.output + "/partial.mp4"
    assert actual["recording_video"]["capture_sidecar"]["frames"] == 3
    assert actual["live_observation"]["cleanup_pending"] is pending
    assert failed.value.native_evidence["cleanup_status"] == ("cleanup_unknown" if pending else "cleanup_failed")
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert repeated.value is failed.value


@pytest.mark.parametrize("fault", ["foreign_path", "active_source", "sidecar_hash", "null_video"])
def test_malformed_component_receipt_cannot_certify_success(recording, fault):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        ack = raw["cleanup"]
        ack["recording_video"] = {"active": False, "finalized": True, "segment_paths": []}
        if fault == "foreign_path":
            ack["recording_video"]["path"] = config.recording.output_root + "/foreign.mp4"
        elif fault == "active_source":
            ack["live_observation"] = {"active": True, "cleanup_complete": False, "cleanup_pending": False}
        elif fault == "sidecar_hash":
            ack["recording_video"]["capture_sidecar"] = {
                "finalized": True,
                "path": process.output + "/capture.jsonl",
                "sha256": "unverified",
                "records": 1,
                "frames": 1,
            }
        else:
            ack["recording_video"] = None

    process.mutate_stop = mutate
    with pytest.raises(OwnedCuaMcpError) as failed:
        owned.stop()
    assert failed.value.code == "cleanup_unknown"
    assert failed.value.native_evidence["component_evidence_valid"] is False
    assert "foreign.mp4" not in json.dumps(failed.value.native_evidence)


@pytest.mark.parametrize("fault", ["unfinalized_video", "error_code", "current_partial", "unfinalized_sidecar"])
def test_success_ack_cannot_override_typed_recording_failure_evidence(recording, fault):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        video = {"active": False, "finalized": True, "segment_paths": []}
        if fault == "unfinalized_video":
            video["finalized"] = False
        elif fault == "error_code":
            video["error_code"] = "capture_failed"
        elif fault == "current_partial":
            video["current_partial"] = process.output + "/partial.mp4"
        else:
            video["capture_sidecar"] = {
                "finalized": False,
                "path": process.output + "/capture.jsonl",
                "sha256": "b" * 64,
                "records": 2,
                "frames": 1,
            }
        raw["cleanup"]["recording_video"] = video

    process.mutate_stop = mutate
    with pytest.raises(OwnedCuaMcpError) as failed:
        owned.stop()
    assert failed.value.code == "cleanup_unknown"
    evidence = failed.value.native_evidence
    assert evidence["reported_cleanup_status"] == "stopped" and evidence["cleanup_status"] == "cleanup_unknown"
    assert evidence["component_evidence_valid"] is False and evidence["cleanup"]["success"] is True
    assert evidence["cleanup"]["recording_video"]
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert failed.value is repeated.value
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1


def test_finalized_typed_recording_ack_passes_without_artifact_existence_inference(recording):
    config, process, _ = recording
    owned = client(config)
    video = {
        "active": False,
        "finalized": True,
        "path": process.output + "/video.mp4",
        "segment_paths": [process.output + "/video.mp4"],
        "capture_sidecar": {
            "finalized": True,
            "path": process.output + "/capture.jsonl",
            "sha256": "b" * 64,
            "records": 2,
            "frames": 1,
        },
    }
    process.mutate_stop = lambda raw: raw["cleanup"].update(recording_video=video)
    stopped = owned.stop()
    assert stopped["cleanup"]["recording_video"] == video and not Path(video["path"]).exists()


@pytest.mark.parametrize("component", ["path", "current_partial", "segment_paths", "capture_sidecar"])
@pytest.mark.parametrize(
    "alias",
    ["/./video.mp4", "/video.mp4:ads", "/video.mp4.", "/video.mp4 ", "/nested /video.mp4", "/v\x00.mp4"],
)
def test_cleanup_rejects_artifact_path_aliases_and_controls(recording, component, alias):
    config, process, _ = recording
    owned = client(config)

    def mutate(raw):
        video = {"active": False, "finalized": False, "segment_paths": []}
        path = process.output + alias
        if component == "segment_paths":
            video[component] = [path]
        elif component == "capture_sidecar":
            video[component] = {"finalized": False, "path": path, "sha256": "b" * 64, "records": 1, "frames": 1}
        else:
            video[component] = path
        raw.update(ok=False, status="cleanup_failed")
        raw["cleanup"].update(success=False, recording_video=video)

    process.mutate_stop = mutate
    with pytest.raises(OwnedCuaMcpError) as failed:
        owned.stop()
    assert failed.value.code == "cleanup_unknown" and failed.value.native_evidence["component_evidence_valid"] is False
    assert "recording_video" not in failed.value.native_evidence["cleanup"]


@pytest.mark.parametrize("component", ["path", "current_partial", "segment_paths"])
@pytest.mark.parametrize("alias", ["/./video.mp4", "/video.mp4:ads", "/video.mp4.", "/v\n.mp4"])
def test_recording_state_uses_same_ordinary_artifact_path_contract(recording, component, alias):
    config, process, _ = recording
    owned = client(config)
    value = process.output + alias
    process.state["video"][component] = [value] if component == "segment_paths" else value
    with pytest.raises(CuaCliError, match="invalid artifact path"):
        owned.recording_state()
    assert process.returncode == 0
