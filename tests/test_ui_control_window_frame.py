"""Public bundled set_frame dispatch with fake MCP and no native runtime."""

from __future__ import annotations

from dataclasses import replace
import json

import pytest

from dcc_mcp_core import parse_skill_md
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_window_frame import FRAME
from test_cua_mcp_window_frame import WINDOW_SCOPE
from test_cua_mcp_window_frame import frame_runtime


@pytest.fixture
def frame_tool(frame_runtime, monkeypatch, tmp_path):
    config, process, launches = frame_runtime
    for key in (
        "DCC_MCP_UI_CONTROL_PROCESS_ID",
        "DCC_MCP_UI_CONTROL_WINDOW_HANDLE",
        "DCC_MCP_UI_CONTROL_PROCESS_NAME",
        "DCC_MCP_UI_CONTROL_WINDOW_TITLE",
    ):
        monkeypatch.delenv(key, raising=False)
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "false")
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(tmp_path / "audit"))
    bridge = HostExecutionBridge()

    def call(args, *, runtime=config):
        return bridge.execute_script(
            str(SCRIPTS / "act.py"),
            {"session_id": "frame-tool", **args},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=runtime,
        )

    try:
        yield call, config, process, launches
    finally:
        bridge.shutdown_script_execution()


def test_formal_frame_action_uses_metadata_without_snapshot_and_retains_outcome(frame_tool):
    call, _, process, _ = frame_tool
    state = call({"action": "get_window_state"})
    assert state["success"] is True
    token = state["context"]["window_state"]["window_state_id"]
    changed = call({"action": "set_frame", "window_state_id": token, "frame": FRAME})
    assert changed["success"] is True
    assert changed["context"]["window_state"]["bounds"] == list(FRAME.values())
    assert changed["context"]["window_state"]["foreground"] is False
    assert changed["context"]["native_outcome"]["window_state_id"] == token
    assert changed["context"]["task_context"]["task_id"] == "public-task"
    assert changed["context"]["fresh_observation_required"] is True
    stale = call({"action": "set_frame", "window_state_id": token, "frame": FRAME})
    assert stale["success"] is False and stale["error"] == "stale_observation"
    assert process.frame_attempts == 1 and process.metadata_reads == 1 and process.snapshots == 0
    proposal = next(
        r["params"]["arguments"] for r in process.requests if r.get("params", {}).get("name") == "start_task"
    )
    assert proposal["allowed_actions"] == [WINDOW_SCOPE]
    assert "execute_action" not in proposal["allowed_methods"]


def test_public_bridge_recipe_get_frame_explicit_restore_then_stop(frame_runtime, monkeypatch, tmp_path):
    config, process, launches = frame_runtime
    config = replace(config, window_operations=("set_frame", "restore_activate"))
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(tmp_path / "audit"))
    for key in ("PROCESS_ID", "WINDOW_HANDLE", "PROCESS_NAME", "WINDOW_TITLE"):
        monkeypatch.delenv("DCC_MCP_UI_CONTROL_" + key, raising=False)
    bridge = HostExecutionBridge()

    def call(script, params):
        return bridge.execute_script(
            str(SCRIPTS / script),
            {"session_id": "frame-recipe", **params},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    try:
        state = call("act.py", {"action": "get_window_state"})
        token = state["context"]["window_state"]["window_state_id"]
        changed = call("act.py", {"action": "set_frame", "window_state_id": token, "frame": FRAME})
        restored = call("act.py", {"action": "restore_window"})
        stopped = call("stop_computer_use.py", {})
        assert all(step["success"] is True for step in (state, changed, restored, stopped))
        assert restored["context"]["window_state"]["foreground"] is True
        assert changed["context"]["window_state"]["foreground"] is False
        assert stopped["context"]["session_id"] == "frame-recipe"
        assert process.returncode == 0 and process.snapshots == 0 and len(launches) == 1
        changes = [
            r["params"]["arguments"] for r in process.requests if r.get("params", {}).get("name") == "dcc_cua_task_call"
        ]
        assert [(item["method"], item["params"]) for item in changes] == [
            ("get_window_state", {}),
            ("set_window_frame", {"window_state_id": token, "frame": FRAME}),
            ("change_window_state", {"operation": "restore_activate"}),
        ]
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.parametrize("minimized", [True, False])
def test_formal_hidden_or_minimized_read_without_token_preserves_explicit_restore(frame_tool, minimized):
    call, config, process, launches = frame_tool
    config = replace(config, window_operations=("set_frame", "restore_activate"))

    def unavailable(raw):
        raw["state"].pop("window_state_id")
        raw["state"].update(visible=False, minimized=minimized, bounds=None, visible_bounds=None)

    process.mutate_metadata = unavailable
    state = call({"action": "get_window_state"}, runtime=config)
    assert state["success"] is True and "window_state_id" not in state["context"]["window_state"]
    assert call({"action": "restore_window"}, runtime=config)["success"] is True
    process.mutate_metadata = lambda raw: None
    token = call({"action": "get_window_state"}, runtime=config)["context"]["window_state"]["window_state_id"]
    assert call({"action": "set_frame", "window_state_id": token, "frame": FRAME}, runtime=config)["success"] is True
    assert process.snapshots == 0 and process.frame_attempts == 1 and len(launches) == 1


@pytest.mark.parametrize(
    "frame",
    [
        {**FRAME, "x": True},
        {**FRAME, "y": 1.0},
        {**FRAME, "width": 0},
        {**FRAME, "height": -1},
        {**FRAME, "x": 2**31 - 1},
        {**FRAME, "restore": True},
        {"x": 0, "y": 0, "width": 1},
        None,
    ],
)
def test_invalid_frame_is_rejected_before_any_owned_child(frame_tool, frame):
    call, _, process, launches = frame_tool
    rejected = call({"action": "set_frame", "window_state_id": "unowned", "frame": frame})
    assert rejected["success"] is False and rejected["error"] == "invalid_action"
    assert launches == [] and process.requests == []


@pytest.mark.parametrize(
    "field,value",
    [
        ("snapshot_id", "pixels-token"),
        ("control_id", "semantic-control"),
        ("accessibility_state_id", "ax-token"),
        ("element_token", "semantic-token"),
        ("element_index", 1),
        ("secret_handle", "secret-token"),
    ],
)
def test_frame_cannot_use_pixel_or_semantic_tokens(frame_tool, field, value):
    call, _, _, launches = frame_tool
    rejected = call({"action": "set_frame", "frame": FRAME, "window_state_id": "metadata", field: value})
    assert rejected["success"] is False and rejected["error"] == "unsupported_action"
    assert launches == []


@pytest.mark.parametrize("token", [None, "", True, 123])
def test_missing_metadata_token_is_rejected_without_child(frame_tool, token):
    call, _, _, launches = frame_tool
    rejected = call({"action": "set_frame", "window_state_id": token, "frame": FRAME})
    assert rejected["success"] is False and rejected["error"] == "stale_observation"
    assert launches == []


def test_default_legacy_transport_and_empty_owner_ceiling_cannot_enable_frame(frame_tool):
    call, config, _, launches = frame_tool
    args = {"action": "set_frame", "window_state_id": "metadata", "frame": FRAME}
    for runtime in (None, replace(config, window_operations=())):
        rejected = call(args, runtime=runtime)
        assert rejected["success"] is False and rejected["error"] == "unsupported_action"
    assert launches == []


def test_request_cannot_replace_owner_options_or_bypass_mutating_policy(frame_tool):
    call, config, _, launches = frame_tool
    rejected = call(
        {
            "action": "set_frame",
            "window_state_id": "metadata",
            "frame": FRAME,
            "trusted_ui_control_runtime": config,
        },
        runtime=replace(config, window_operations=()),
    )
    assert rejected["success"] is False and rejected["error"] == "unsupported_action"
    denied = call(
        {
            "action": "set_frame",
            "frame": FRAME,
            "window_state_id": "metadata",
            "policy": {"allow_mutating_actions": False},
        }
    )
    assert denied["success"] is False and denied["error"] == "policy_disabled"
    assert launches == []


def test_frame_native_uncertainty_is_error_and_never_replayed(frame_tool):
    call, _, process, _ = frame_tool
    token = call({"action": "get_window_state"})["context"]["window_state"]["window_state_id"]
    process.frame_error = {
        "type": "error",
        "code": "input_failed",
        "message": "native readback uncertain",
        "details": {
            "action_attempted": True,
            "completion": "unknown",
            "effect_unknown": True,
            "automatic_input": False,
            "blind_retry": False,
            "input_sent": "not_sent",
        },
    }
    failed = call({"action": "set_frame", "window_state_id": token, "frame": FRAME})
    assert failed["success"] is False
    assert failed["context"]["details"]["effect_unknown"] is True
    assert failed["context"]["fresh_observation_required"] is True
    assert call({"action": "set_frame", "window_state_id": token, "frame": FRAME})["success"] is False
    assert process.frame_attempts == 1 and process.snapshots == 0


@pytest.mark.parametrize(
    "invalid",
    [
        {"frame": {**FRAME, "width": False}},
        {"window_state_id": None},
        {"snapshot_id": "pixels"},
    ],
)
def test_invalid_skill_attempt_discards_existing_metadata_without_calling_native(frame_tool, invalid):
    call, _, process, _ = frame_tool
    token = call({"action": "get_window_state"})["context"]["window_state"]["window_state_id"]
    args = {"action": "set_frame", "window_state_id": token, "frame": FRAME}
    assert call({**args, **invalid})["success"] is False
    reused = call(args)
    assert reused["success"] is False and reused["error"] == "stale_observation"
    assert process.frame_attempts == 0 and process.metadata_reads == 1 and process.snapshots == 0


def _check_frame_schema(schema):
    assert "set_frame" in schema["properties"]["action"]["enum"]
    frame = schema["properties"]["frame"]
    assert frame["required"] == ["x", "y", "width", "height"] and frame["additionalProperties"] is False
    assert all(item["type"] == "integer" for item in frame["properties"].values())
    assert frame["properties"]["width"]["minimum"] == 1
    assert frame["properties"]["x"]["minimum"] == -(2**31)
    assert frame["properties"]["height"]["maximum"] == 2**31 - 1
    frame_conditions = [
        condition
        for condition in schema["allOf"]
        if condition.get("if") == {
            "properties": {"action": {"const": "set_frame"}},
            "required": ["action"],
        }
    ]
    assert len(frame_conditions) == 1
    assert frame_conditions[0]["then"]["required"] == ["window_state_id", "frame"]


def test_public_act_yaml_advertises_closed_integer_frame_and_metadata_requirement():
    yaml = pytest.importorskip("yaml")
    metadata = yaml.safe_load((SCRIPTS.parent / "tools.yaml").read_text(encoding="utf-8"))
    _check_frame_schema(next(tool for tool in metadata["tools"] if tool["name"] == "act")["input_schema"])


def test_official_native_skill_parser_preserves_frame_schema():
    pytest.importorskip("dcc_mcp_core._core", reason="Requires a separately verified official native payload.")
    meta = parse_skill_md(str(SCRIPTS.parent))
    schema = json.loads(next(tool for tool in meta.tools if tool.name == "act").input_schema)
    _check_frame_schema(schema)
