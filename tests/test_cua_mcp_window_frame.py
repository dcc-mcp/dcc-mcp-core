"""Metadata-only frame contracts using in-memory MCP pipes, never native UI."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import replace
import json
from typing import Any

import pytest

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionRequest
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_client import validate_window_frame
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import FakeMcpProcess
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import options
from test_cua_mcp_pixels import pixel_response
from test_cua_mcp_pixels import window_response

FRAME = {"x": -1200, "y": -200, "width": 640, "height": 480}
WINDOW_SCOPE = {
    "action": "set_window_frame",
    "input_kind": "window_state",
    "secret_input": False,
    "authorization_category": "window_state",
}


class FrameMcpProcess(FakeMcpProcess):
    """Model only public metadata/frame envelopes; Popen is always replaced."""

    def __init__(self) -> None:
        super().__init__()
        self.metadata_reads = 0
        self.frame_attempts = 0
        self.frame_error = None
        self.drop_frame_response = False
        self.mutate_metadata = lambda raw: None
        self.mutate_frame = lambda raw: None
        self.mutate_window = lambda raw: raw["state"].update(visible_bounds=list(raw["state"]["bounds"]))

    def write(self, data: bytes) -> None:
        request = json.loads(data)
        arguments = request.get("params", {}).get("arguments", {})
        method = arguments.get("method")
        if request.get("method") != "tools/call" or method not in {"get_window_state", "set_window_frame"}:
            return super().write(data)
        self.requests.append(request)
        assert request["params"]["name"] == "dcc_cua_task_call"
        assert arguments["task_id"] == "public-task"
        if method == "get_window_state":
            assert arguments["params"] == {}
            self.metadata_reads += 1
            state = window_response("activate")["state"]
            state.update(
                foreground=False,
                visible_bounds=[-627, 0, 614, 467],
                window_state_id="native-state-" + str(self.metadata_reads),
            )
            raw = {"type": "window_state", "session_id": "mcp-public-task", "state": state}
            self.mutate_metadata(raw)
        else:
            self.frame_attempts += 1
            if self.drop_frame_response:
                return
            params = arguments["params"]
            assert set(params) == {"window_state_id", "frame"}
            frame = params["frame"]
            applied = [frame[key] for key in ("x", "y", "width", "height")]
            state = window_response("activate")["state"]
            state.update(bounds=applied, visible_bounds=applied, foreground=False)
            result = {
                "success": True,
                "effect": "confirmed",
                "operation": "set_window_frame",
                "requested_frame": dict(frame),
                "applied_frame": applied,
                "state": deepcopy(state),
                "target": dict(TARGET),
                "native_instance": deepcopy(state["native_instance"]),
                "window_state_id": params["window_state_id"],
                "fresh_observation_required": True,
                "automatic_input": False,
                "process_terminated": False,
                "cua": {"path": "windows_exact_instance_set_window_pos"},
            }
            raw = {
                "type": "window_frame_set",
                "session_id": "mcp-public-task",
                "state": state,
                "result": result,
            }
            if self.frame_error is not None:
                raw = deepcopy(self.frame_error)
            self.mutate_frame(raw)
        raw.setdefault("task_context", pixel_response()["task_context"])
        self.lines.put(
            (
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "structuredContent": raw,
                            "content": [],
                            "isError": raw.get("type") == "error",
                        },
                    }
                )
                + "\n"
            ).encode("utf-8")
        )


@pytest.fixture
def frame_runtime(tmp_path, monkeypatch):
    process = FrameMcpProcess()
    launches = []

    def popen(command, **kwargs):
        launches.append((command, kwargs))
        return process

    monkeypatch.setattr("dcc_mcp_core.host.cua_mcp_transport.subprocess.Popen", popen)
    return options(tmp_path, allowed_actions=(), window_operations=("set_frame",)), process, launches


def test_frame_grant_is_explicit_window_scope_and_never_adds_raw_input(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(config)
    try:
        proposal = next(
            item["params"]["arguments"]
            for item in process.requests
            if item.get("params", {}).get("name") == "start_task"
        )
        assert proposal["allowed_methods"] == ["snapshot", "get_window_state", "set_window_frame"]
        assert proposal["allowed_actions"] == [WINDOW_SCOPE]
        assert "allow_recording" not in proposal
        assert "execute_action" not in proposal["allowed_methods"]
    finally:
        owned.stop()


def test_frame_requires_own_metadata_consumes_it_and_does_not_capture(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(config)
    try:
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id="unowned")
        assert process.frame_attempts == 0
        metadata = owned.window_state()
        token = metadata["state"]["window_state_id"]
        changed = owned.set_frame(FRAME, window_state_id=token)
        assert changed["state"]["bounds"] == list(FRAME.values())
        assert changed["state"]["foreground"] is False
        assert changed["result"]["window_state_id"] == token
        assert "window_state_id" not in changed["state"]
        assert owned._observation_id is None
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=token)
        assert process.frame_attempts == 1 and process.metadata_reads == 1 and process.snapshots == 0
    finally:
        owned.stop()


def test_metadata_cannot_authorize_content_input_and_old_metadata_is_refused(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(replace(config, allowed_actions=("click",)))
    try:
        first = owned.window_state()["state"]["window_state_id"]
        latest = owned.window_state()["state"]["window_state_id"]
        assert first != latest and owned._observation_id is None
        with pytest.raises(CuaCliError, match="fresh pixels"):
            owned.execute({"action": "click", "input_kind": "raw_input", "x": 0, "y": 0})
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=first)
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=latest)
        assert process.frame_attempts == 0 and process.snapshots == 0
    finally:
        owned.stop()


def test_default_options_refuse_frame_without_mutation(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(replace(config, window_operations=()))
    try:
        owned.window_state()
        with pytest.raises(CuaCliError, match="owner"):
            owned.set_frame(FRAME, window_state_id="native-state-1")
        assert process.frame_attempts == 0
    finally:
        owned.stop()


def test_frame_owner_grant_cannot_use_legacy_window_operation(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(config)
    try:
        with pytest.raises(CuaCliError, match="authorize"):
            owned.change_window_state("set_frame")
        assert not any(
            item.get("params", {}).get("arguments", {}).get("method") == "change_window_state"
            for item in process.requests
        )
    finally:
        owned.stop()


@pytest.mark.parametrize("mutation", ["restore", "click"])
def test_other_mutation_consumes_frame_metadata(frame_runtime, mutation):
    config, process, _ = frame_runtime
    owned = client(replace(config, allowed_actions=("click",), window_operations=("set_frame", "restore_activate")))
    try:
        if mutation == "click":
            owned.snapshot(max_depth=1, max_nodes=1)
        token = owned.window_state()["state"]["window_state_id"]
        if mutation == "click":
            owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 1})
        else:
            owned.change_window_state("restore")
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=token)
        assert process.frame_attempts == 0
    finally:
        owned.stop()


@pytest.mark.parametrize(
    "key,value",
    [
        ("x", True),
        ("y", False),
        ("width", True),
        ("height", False),
        ("x", 1.0),
        ("y", "1"),
        ("width", None),
        ("height", 0),
        ("width", -1),
        ("x", -(2**31) - 1),
        ("y", 2**31),
        ("width", 2**31),
        ("height", 2**31),
        ("x", 2**31 - 1),
        ("y", 2**31 - 1),
    ],
)
def test_frame_rejects_coercion_nonpositive_and_overflow(key, value):
    frame = {**FRAME, key: value}
    with pytest.raises(CuaCliError, match="integers"):
        validate_window_frame(frame)


@pytest.mark.parametrize("frame", [None, [], {}, {"x": 0, "y": 0, "width": 1}, {**FRAME, "activate": True}])
def test_frame_requires_complete_closed_shape(frame):
    with pytest.raises(CuaCliError):
        validate_window_frame(frame)


def test_negative_monitor_and_signed_boundary_frames_are_supported():
    assert validate_window_frame(FRAME) == FRAME
    boundary = {"x": -(2**31), "y": 2**31 - 2, "width": 2**31 - 1, "height": 1}
    assert validate_window_frame(boundary) == boundary


def test_invalid_frame_attempt_consumes_metadata_and_pixel_evidence(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(config)
    try:
        owned.snapshot(max_depth=1, max_nodes=1)
        token = owned.window_state()["state"]["window_state_id"]
        with pytest.raises(CuaCliError, match="integers"):
            owned.set_frame({**FRAME, "width": False}, window_state_id=token)
        assert owned._observation_id is None
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=token)
        assert process.frame_attempts == 0 and process.metadata_reads == 1 and process.snapshots == 1
    finally:
        owned.stop()


@pytest.mark.parametrize(
    "field,value",
    [
        ("visible_bounds", None),
        ("visible_bounds", [0, 0, 0, 1]),
        ("visible_bounds", [2**31 - 1, 0, 1, 1]),
        ("visible_bounds", [False, 0, 1, 1]),
        ("bounds", [0, 0, 1, 2**31]),
        ("bounds", [0, 2**31 - 1, 1, 1]),
    ],
)
def test_frame_metadata_requires_real_valid_dwm_and_win32_geometry(frame_runtime, field, value):
    config, process, _ = frame_runtime
    owned = client(config)
    process.mutate_metadata = lambda raw: raw["state"].update({field: value})
    with pytest.raises(CuaCliError, match="geometry"):
        owned.window_state()
    assert process.frame_attempts == 0 and process.returncode == 0


@pytest.mark.parametrize("unavailable", ["minimized", "hidden", "geometry"])
def test_unavailable_metadata_without_token_is_readonly_and_keeps_same_session_recovery(frame_runtime, unavailable):
    config, process, launches = frame_runtime
    owned = client(replace(config, window_operations=("set_frame", "restore_activate")))

    def unavailable_metadata(raw):
        state = raw["state"]
        state.pop("window_state_id")
        state["visible_bounds"] = None
        if unavailable != "geometry":
            state.update(minimized=unavailable == "minimized", visible=False, bounds=None)

    process.mutate_metadata = unavailable_metadata
    try:
        state = owned.window_state()["state"]
        assert "window_state_id" not in state and owned._window_state_id is None
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id="unowned")
        owned.change_window_state("restore")
        process.mutate_metadata = lambda raw: None
        token = owned.window_state()["state"]["window_state_id"]
        assert owned.set_frame(FRAME, window_state_id=token)["result"]["success"] is True
        assert process.frame_attempts == 1 and process.snapshots == 0 and len(launches) == 1
    finally:
        owned.stop()


@pytest.mark.parametrize("field", ["session", "target", "instance", "token", "ax", "repeat"])
def test_metadata_wrong_identity_or_reused_id_closes_owned_task(frame_runtime, field):
    config, process, _ = frame_runtime
    owned = client(config)
    if field in {"instance", "repeat"}:
        owned.window_state()

    def corrupt(raw):
        if field == "session":
            raw["session_id"] = "mcp-foreign"
        elif field == "target":
            raw["state"]["window_handle"] += 1
        elif field == "instance":
            raw["state"]["native_instance"]["window_thread_id"] += 1
        elif field == "token":
            raw["state"]["window_state_id"] = ""
        elif field == "ax":
            raw["accessibility_state_id"] = "fabricated"
        else:
            raw["state"]["window_state_id"] = "native-state-1"

    process.mutate_metadata = corrupt
    with pytest.raises(CuaCliError):
        owned.window_state()
    assert process.returncode == 0 and process.frame_attempts == 0 and process.snapshots == 0


@pytest.mark.parametrize(
    "field",
    [
        "session",
        "task",
        "target",
        "instance",
        "token",
        "requested",
        "applied",
        "actual",
        "missing_dwm",
        "new_token",
        "automatic",
        "effect",
    ],
)
def test_frame_success_requires_exact_native_completion_and_no_new_token(frame_runtime, field):
    config, process, _ = frame_runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]

    def corrupt(raw):
        if field == "session":
            raw["session_id"] = "mcp-foreign"
        elif field == "task":
            raw["task_context"] = {**pixel_response()["task_context"], "task_id": "foreign-task"}
        elif field == "target":
            raw["result"]["target"]["window_handle"] += 1
        elif field == "instance":
            raw["result"]["native_instance"]["window_class_hash"] += 1
        elif field == "token":
            raw["result"]["window_state_id"] = "foreign"
        elif field == "requested":
            raw["result"]["requested_frame"]["width"] = True
        elif field == "applied":
            raw["result"]["applied_frame"][0] += 1
        elif field == "actual":
            raw["result"]["state"]["dpi"] += 1
        elif field == "missing_dwm":
            raw["state"]["visible_bounds"] = None
            raw["result"]["state"]["visible_bounds"] = None
        elif field == "new_token":
            raw["state"]["window_state_id"] = "new-unrequested-token"
        elif field == "automatic":
            raw["result"]["automatic_input"] = True
        else:
            raw["result"]["effect"] = "unverifiable"

    process.mutate_frame = corrupt
    with pytest.raises(CuaCliError):
        owned.set_frame(FRAME, window_state_id=token)
    assert process.returncode == 0 and process.frame_attempts == 1 and process.snapshots == 0


@pytest.mark.parametrize("attempted,completion", [(False, "known"), (True, "unknown")])
def test_native_frame_error_retains_attempt_state_consumes_token_without_replay(frame_runtime, attempted, completion):
    config, process, _ = frame_runtime
    owned = client(config)
    try:
        token = owned.window_state()["state"]["window_state_id"]
        process.frame_error = {
            "type": "error",
            "code": "stale_observation",
            "message": "native frame refused",
            "details": {
                "action_attempted": attempted,
                "completion": completion,
                "effect_unknown": attempted,
                "input_sent": "not_sent",
                "automatic_input": False,
                "blind_retry": False,
            },
        }
        with pytest.raises(CuaCliError) as raised:
            owned.set_frame(FRAME, window_state_id=token)
        assert raised.value.native_evidence["details"]["action_attempted"] is attempted
        assert raised.value.native_evidence["details"]["completion"] == completion
        with pytest.raises(CuaCliError, match="fresh get_window_state"):
            owned.set_frame(FRAME, window_state_id=token)
        assert process.frame_attempts == 1 and process.snapshots == 0
    finally:
        owned.stop()


def test_uncertain_frame_timeout_closes_child_and_never_replays(frame_runtime):
    config, process, _ = frame_runtime
    owned = client(replace(config, timeout_seconds=0.03))
    token = owned.window_state()["state"]["window_state_id"]
    process.drop_frame_response = True
    with pytest.raises(CuaCliError):
        owned.set_frame(FRAME, window_state_id=token)
    with pytest.raises(CuaCliError):
        owned.set_frame(FRAME, window_state_id=token)
    assert process.frame_attempts == 1 and process.returncode == 0 and owned._observation_id is None


def test_frame_is_mutating_policy_but_not_raw_coordinates_or_keyboard():
    request = UiActionRequest(control_id=None, action=UiActionKind.SET_FRAME)
    assert UiControlPolicy(allow_raw_coordinates=False, allow_keyboard_shortcuts=False).allows_request(request)
    assert not UiControlPolicy(allow_mutating_actions=False).allows_request(request)
    assert not UiControlPolicy(scope_denied=True).allows_request(request)
