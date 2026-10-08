"""Public-MCP wire and real bundled-tool contracts without launching a UI runtime."""

from __future__ import annotations

import base64
from copy import deepcopy
from dataclasses import replace
import hashlib
import importlib.util
import json
from pathlib import Path
import queue
import struct
import subprocess
from types import SimpleNamespace
from typing import Any

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.cua_mcp_transport import OwnedCuaMcpTransport
from dcc_mcp_core.server import DccServerOptions
from dcc_mcp_core.server import UiControlRuntimeOptions

VERSION = "1.9.4+test"
TARGET = {"process_id": 42, "window_handle": 500, "window_title": "Owned DCC Session"}
PNG = b"\x89PNG\r\n\x1a\n" + b"\x00\x00\x00\x0dIHDR" + struct.pack(">II", 640, 480) + b"fixture"
SCRIPTS = REPO_ROOT / "python/dcc_mcp_core/skills/ui-control/scripts"


def options(tmp_path: Path, **kwargs: Any) -> UiControlRuntimeOptions:
    binary = tmp_path / "owned-cua.exe"
    binary.write_bytes(b"not executable: the test replaces Popen")
    return UiControlRuntimeOptions(
        binary=str(binary),
        sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
        runtime_version=VERSION,
        allowed_actions=kwargs.pop("allowed_actions", ("click", "keypress", "type")),
        **kwargs,
    )


def pixel_response(index: int = 1) -> dict[str, Any]:
    return {
        "type": "snapshot",
        "observation_mode": "pixels_only",
        "observation_id": f"native-obs-{index}",
        "accessibility_state_id": None,
        "target": dict(TARGET),
        "root": {"accessibility_available": False},
        "image": {"length": len(PNG), "mime_type": "image/png", "encoding": "binary_frame"},
        "task_context": {
            "provider": "dcc-cua",
            "runtime_version": VERSION,
            "task_id": "public-task",
            "target": dict(TARGET),
        },
        "observation": {
            **TARGET,
            "observation_id": f"native-obs-{index}",
            "session_id": "native-runtime-session",
            "width": 640,
            "height": 480,
            "source_rect": [-640, 0, 640, 480],
            "capture_backend": "windows_visible_exact_capture",
            "capture_provenance": {
                **TARGET,
                "observation_mode": "pixels_only",
                "pixels_captured": True,
                "whole_desktop_capture": False,
                "accessibility_available": False,
                "backend": "windows_visible_exact_capture",
                "window_dpi": 144,
                "capture_generation": index,
                "native_window_bounds": [-640, 0, 640, 480],
                "native_instance": {
                    "process_creation_time_100ns": 123,
                    "window_thread_id": 7,
                    "window_class_hash": 8,
                    "owner_window_handle": 0,
                },
            },
        },
    }


def window_response(operation: str = "activate", observation_id: str | None = None) -> dict[str, Any]:
    instance = pixel_response()["observation"]["capture_provenance"]["native_instance"]
    state = {
        **TARGET,
        "exists": True,
        "visible": operation != "minimize",
        "minimized": operation == "minimize",
        "foreground": operation != "minimize",
        "bounds": [-640, 0, 640, 480],
        "dpi": 144,
        "native_instance": instance,
        "backend": "windows-exact-native-state",
    }
    result = {"success": True, "fresh_observation_required": True, "automatic_input": False}
    if operation == "minimize":
        result.update(
            {
                "target": dict(TARGET),
                "operation": "minimize",
                "effect": "confirmed",
                "native_instance": instance,
                "observation_id": observation_id,
                "state": {**state, "instance": instance},
                "process_terminated": False,
            }
        )
    else:
        result["target"] = {
            "pid": TARGET["process_id"],
            "window_id": TARGET["window_handle"],
            "is_foreground": True,
            "is_minimized": False,
            "is_on_screen": True,
        }
    return {
        "type": "window_state_changed",
        "operation": operation,
        "state": state,
        "result": result,
        "task_context": pixel_response()["task_context"],
    }


def tool_catalog() -> list[dict[str, Any]]:
    envelopes = {
        "start_task": {
            "application_label": "string",
            "target_process_id": "integer",
            "target_window_handle": "integer",
            "surface": "string",
            "observation_mode": "string",
            "allowed_methods": "array",
            "allowed_actions": "array",
            "ttl_minutes": "integer",
        },
        "dcc_cua_task_call": {"task_id": "string", "method": "string", "params": "object"},
        "task_status": {"task_id": "string"},
        "stop_task": {"task_id": "string"},
    }
    tools = []
    for name, fields in envelopes.items():
        properties = {key: {"type": kind} for key, kind in fields.items()}
        if name == "start_task":
            properties["observation_mode"]["enum"] = ["semantic", "pixels_only"]
        required = (
            ["application_label", "surface", "allowed_methods", "allowed_actions"]
            if name == "start_task"
            else list(fields)
        )
        tools.append(
            {
                "name": name,
                "inputSchema": {
                    "type": "object",
                    "additionalProperties": False,
                    "required": required,
                    "properties": properties,
                },
            }
        )
    return tools


class FakeMcpProcess:
    """An in-memory child pipe fixture, not a running server or executable."""

    def __init__(self) -> None:
        self.lines: queue.Queue = queue.Queue()
        self.requests: list[dict[str, Any]] = []
        self.returncode = None
        self.stdin = SimpleNamespace(write=self.write, flush=lambda: None, close=self.finish)
        self.stdout = SimpleNamespace(readline=lambda size: self.lines.get(), close=lambda: None)
        self.version = VERSION
        self.mutate_snapshot = lambda raw: None
        self.mutate_open = lambda raw: None
        self.mutate_action = lambda raw: None
        self.mutate_window = lambda raw: None
        self.mutate_initialize = lambda raw: None
        self.mutate_catalog = lambda raw: None
        self.bad_id = False
        self.drop_response = False
        self.stop_error = False
        self.snapshots = 0

    def write(self, data: bytes) -> None:
        request = json.loads(data)
        self.requests.append(request)
        if "id" not in request or self.drop_response:
            return
        method = request["method"]
        if method == "initialize":
            result = {
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "dcc-cua-task-automation", "version": self.version},
                "capabilities": {"tools": {}},
            }
            self.mutate_initialize(result)
        elif method == "tools/list":
            result = {"tools": tool_catalog()}
            self.mutate_catalog(result)
        else:
            assert method == "tools/call"
            name = request["params"]["name"]
            arguments = request["params"]["arguments"]
            content = []
            if name == "start_task":
                raw = {
                    "ok": True,
                    "provider": "dcc-cua",
                    "runtime_version": VERSION,
                    "task_id": "public-task",
                    "status": "started",
                    "target": dict(TARGET),
                }
                self.mutate_open(raw)
            elif name == "stop_task":
                assert arguments == {"task_id": "public-task"}
                raw = {"task_id": "public-task", "status": "stopped"}
            else:
                assert name == "dcc_cua_task_call"
                assert arguments["task_id"] == "public-task"
                if arguments["method"] == "snapshot":
                    assert arguments["params"] == {}
                    self.snapshots += 1
                    raw = pixel_response(self.snapshots)
                    self.mutate_snapshot(raw)
                    content = [
                        {"type": "image", "mimeType": "image/png", "data": base64.b64encode(PNG).decode("ascii")}
                    ]
                elif arguments["method"] in {"change_window_state", "minimize_window", "get_window_state"}:
                    params = arguments["params"]
                    operation = (
                        "minimize" if arguments["method"] == "minimize_window" else params.get("operation", "activate")
                    )
                    raw = window_response(operation, params.get("observation_id"))
                    if arguments["method"] == "get_window_state":
                        raw["type"] = "window_state"
                    self.mutate_window(raw)
                else:
                    assert arguments["method"] == "execute_action"
                    raw = {
                        "type": "action_completed",
                        "success": True,
                        "task_context": pixel_response()["task_context"],
                        "result": {
                            "success": True,
                            "effect": "unverifiable",
                            "verification_required": True,
                            "fresh_observation_required": True,
                            "target": dict(TARGET),
                            "route": "windows_exact_pixel_final_input",
                            "native_instance": pixel_response()["observation"]["capture_provenance"]["native_instance"],
                            "delivery": {"delivery_completed": True, "post_dispatch_validated": True},
                        },
                    }
                    self.mutate_action(raw)
            result = {
                "structuredContent": raw,
                "content": content,
                "isError": (name == "stop_task" and self.stop_error) or raw.get("type") == "error",
            }
        response = {"jsonrpc": "2.0", "id": request["id"] + int(self.bad_id), "result": result}
        self.lines.put((json.dumps(response) + "\n").encode("utf-8"))

    def poll(self) -> Any:
        return self.returncode

    def finish(self) -> None:
        self.returncode = 0
        self.lines.put(b"")

    def wait(self, timeout: float) -> int:
        self.finish()
        return 0

    terminate = finish
    kill = finish


@pytest.fixture
def runtime(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    process = FakeMcpProcess()
    launches = []

    def popen(command: list[str], **kwargs: Any):
        launches.append((command, kwargs))
        return process

    monkeypatch.setattr("dcc_mcp_core.host.cua_mcp_transport.subprocess.Popen", popen)
    return options(tmp_path), process, launches


def client(config: UiControlRuntimeOptions, dcc_type: str = "unreal") -> PixelsMcpHostClient:
    return PixelsMcpHostClient(
        session_id="ui-logical",
        dcc_type=dcc_type,
        process_id=42,
        window_handle=500,
        allow_raw_input=True,
        options=config,
    )


@pytest.mark.parametrize("dcc_type", ["unreal", "maya"])
def test_public_mcp_exact_wire_pixels_and_owned_cleanup(runtime, dcc_type: str) -> None:
    config, process, launches = runtime
    owned = client(config, dcc_type)
    assert launches[0][0] == [config.binary, "mcp-server"]
    assert launches[0][1]["creationflags"] >= 0
    start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
    proposal = start["params"]["arguments"]
    assert proposal["target_process_id"] == 42 and proposal["target_window_handle"] == 500
    assert proposal["application_label"] == dcc_type
    assert proposal["observation_mode"] == "pixels_only"
    assert "task" not in proposal and "grant" not in proposal
    assert all(scope["secret_input"] is False for scope in proposal["allowed_actions"])
    snapshot = owned.snapshot(max_depth=1, max_nodes=1)
    assert snapshot["accessibility_state_id"] is None
    assert snapshot["observation"]["session_id"] == "native-runtime-session"
    assert owned.session_id == "ui-logical" and owned.task_id == "public-task"
    assert snapshot["image_bytes"] == PNG
    assert snapshot["observation"]["capture_provenance"]["window_dpi"] == 144
    action = owned.execute(
        {
            "action": "click",
            "input_kind": "raw_input",
            "intent": "ordinary_edit",
            "x": 20,
            "y": 30,
            "path": [],
            "keys": [],
        }
    )
    request = process.requests[-1]["params"]["arguments"]
    assert request["params"]["observation_id"] == "native-obs-1"
    assert "accessibility_state_id" not in request["params"]
    assert request["params"]["capture_after"] is False
    assert action["effect"] == "unverifiable" and action["fresh_observation_required"] is True
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute({"action": "click", "input_kind": "raw_input"})
    owned.stop()
    owned.stop()
    assert process.returncode == 0
    assert sum(r.get("params", {}).get("name") == "stop_task" for r in process.requests) == 1


@pytest.mark.parametrize("fault", ["task", "target", "native_instance", "dpi", "ax", "rect"])
def test_malformed_or_changed_pixel_fence_closes_owned_runtime(runtime, fault: str) -> None:
    config, process, _ = runtime
    owned = client(config)

    def corrupt(raw):
        if fault == "task":
            raw["task_context"]["task_id"] = "other-task"
        elif fault == "target":
            raw["observation"]["window_handle"] = 501
        elif fault == "native_instance":
            raw["observation"]["capture_provenance"].pop("native_instance")
        elif fault == "dpi":
            raw["observation"]["capture_provenance"]["window_dpi"] = 0
        elif fault == "ax":
            raw["accessibility_state_id"] = "invented-AX"
        else:
            raw["observation"]["source_rect"] = [0, 0, 0, 480]

    process.mutate_snapshot = corrupt
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=1, max_nodes=1)
    assert process.returncode == 0
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)


def test_startup_failure_preserves_original_error_even_cleanup_fails(runtime) -> None:
    config, process, _ = runtime
    process.mutate_open = lambda raw: raw["target"].update(window_handle=501)
    process.stop_error = True
    with pytest.raises(CuaCliError, match="PID/HWND"):
        client(config)
    assert process.returncode == 0


def test_identity_version_and_digest_fail_before_task_or_process(runtime) -> None:
    config, process, launches = runtime
    process.version = "wrong-version"
    with pytest.raises(CuaCliError, match="identity/version"):
        client(config)
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)
    assert process.returncode == 0
    config2 = UiControlRuntimeOptions(binary=config.binary, sha256="0" * 64, runtime_version=VERSION)
    with pytest.raises(CuaCliError, match="SHA-256"):
        OwnedCuaMcpTransport(config2)
    assert len(launches) == 1


def test_timeout_does_not_replay_and_consumes_observation(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    object.__setattr__(config, "timeout_seconds", 0.01)
    process.drop_response = True
    with pytest.raises(CuaCliError, match="not retried"):
        owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    attempts = [
        r for r in process.requests if r.get("params", {}).get("arguments", {}).get("method") == "execute_action"
    ]
    assert len(attempts) == 1 and process.returncode == 0


def test_typed_owner_options_cannot_be_tool_selected(tmp_path: Path) -> None:
    config = options(tmp_path)
    assert DccServerOptions("maya", tmp_path).ui_control is None
    assert DccServerOptions.from_env("maya", tmp_path, ui_control=config).ui_control is config
    with pytest.raises(TypeError, match="UiControlRuntimeOptions"):
        DccServerOptions("maya", tmp_path, ui_control={"binary": config.binary})
    with pytest.raises(ValueError, match="pixels_only"):
        options(tmp_path, observation_mode="semantic")


def test_real_bundled_backend_pixels_snapshot_act_find_stop(runtime, monkeypatch: pytest.MonkeyPatch) -> None:
    config, process, _ = runtime
    spec = importlib.util.spec_from_file_location("_owned_pixels_test_backend", SCRIPTS / "_cua_backend.py")
    assert spec and spec.loader
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    params = {
        "session_id": "formal-ui-route",
        "trusted_ui_control_runtime": config,
        "trusted_adapter_scope": {"dcc_type": "unreal", **TARGET},
    }
    try:
        snapshot = backend.snapshot_tool(params)
        assert snapshot["success"] is True
        context = snapshot["context"]
        assert context["snapshot_id"] == "native-obs-1"
        assert context["accessibility_state_id"] is None and context["snapshot"]["root"] is None
        assert "physical" in snapshot["prompt"]
        find = backend.find_tool(params)
        assert find["success"] is False and find["error"] == "unsupported_action"
        acted = backend.act_tool({**params, "snapshot_id": context["snapshot_id"], "action": "click", "x": 5, "y": 10})
        assert acted["success"] is True
        metadata = acted["context"]["result"]["metadata"]
        assert metadata["effect"] == "unverifiable" and metadata["verification_required"] is True
        stale = backend.act_tool({**params, "snapshot_id": context["snapshot_id"], "action": "click", "x": 5, "y": 10})
        assert stale["success"] is False
        assert backend.stop_computer_use_tool(params)["success"] is True
    finally:
        backend.cleanup()
    assert process.returncode == 0


def test_server_injection_and_executor_strip_untrusted_selection(tmp_path: Path) -> None:
    from dcc_mcp_core._server.execution_bridge import ExecutionBridgeBinder
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config = options(tmp_path)
    owner = SimpleNamespace(
        _dcc_name="maya",
        _dcc_pid=42,
        _dcc_window_handle=500,
        _dcc_window_title=None,
        _options=SimpleNamespace(ui_control=config),
    )
    seen = []
    wrapped = ExecutionBridgeBinder(owner)._with_adapter_context(lambda path, params, **meta: seen.append(meta))
    wrapped("unused", {"trusted_ui_control_runtime": "attacker"}, skill_name="ui-control")
    assert seen[0]["trusted_ui_control_runtime"] is config
    bridge = HostExecutionBridge()
    import dcc_mcp_core._server.inprocess_executor as executor_module

    original = executor_module.run_skill_script
    received = []
    executor_module.run_skill_script = lambda path, params, **kwargs: received.append(dict(params))
    try:
        bridge.execute_script(str(tmp_path / "tool.py"), {"trusted_ui_control_runtime": "attacker"})
        bridge.execute_script(
            str(tmp_path / "tool.py"), {"trusted_ui_control_runtime": "attacker"}, trusted_ui_control_runtime=config
        )
    finally:
        executor_module.run_skill_script = original
        bridge.shutdown_script_execution()
    assert "trusted_ui_control_runtime" not in received[0]
    assert received[1]["trusted_ui_control_runtime"] is config


def test_formal_script_entrypoints_preserve_pixels_provenance_and_stop(runtime, monkeypatch, tmp_path) -> None:
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(tmp_path / "logs"))
    monkeypatch.setenv("DCC_MCP_ARTEFACT_DIR", str(tmp_path / "artifacts"))
    bridge = HostExecutionBridge()
    owner_scope = {"dcc_type": "unreal", **TARGET}

    def call(name, args):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            args,
            skill_name="ui-control",
            action_name="ui_control__" + name,
            trusted_adapter_scope=owner_scope,
            trusted_ui_control_runtime=config,
        )

    try:
        snapshot = call("snapshot", {"session_id": "public-script-route"})
        assert snapshot["success"] is True
        context = snapshot["context"]
        assert context["accessibility_state_id"] is None
        provenance = context["capture_provenance"]
        assert provenance["backend"] == "dcc-cua"
        assert provenance["native_session_id"] == "native-runtime-session"
        assert provenance["task_context"]["task_id"] == "public-task"
        assert provenance["native_capture_provenance"]["window_dpi"] == 144
        acted = call(
            "act",
            {
                "session_id": "public-script-route",
                "snapshot_id": context["snapshot_id"],
                "action": "keypress",
                "keys": ["CTRL+F"],
            },
        )
        assert acted["success"] is True
        assert acted["context"]["result"]["metadata"]["effect"] == "unverifiable"
        assert call("stop_computer_use", {"session_id": "public-script-route"})["success"] is True
    finally:
        bridge.shutdown_script_execution()
    assert process.returncode == 0


def test_foreign_action_result_identity_closes_without_confirmation(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    process.mutate_action = lambda raw: raw["result"]["target"].update(process_id=43)
    with pytest.raises(CuaCliError, match="PID/HWND"):
        owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    assert process.returncode == 0


@pytest.mark.parametrize("enabled", [False, True])
def test_empty_owner_actions_and_operator_raw_ceiling_are_never_widened(runtime, monkeypatch, enabled) -> None:
    config, process, _ = runtime
    if enabled:
        config = UiControlRuntimeOptions(binary=config.binary, sha256=config.sha256, runtime_version=VERSION)
    owned = PixelsMcpHostClient(
        session_id="read-only",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        allow_raw_input=enabled,
        options=config,
    )
    try:
        start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
        assert start["params"]["arguments"]["allowed_actions"] == []
        assert "execute_action" not in start["params"]["arguments"]["allowed_methods"]
        owned.snapshot(max_depth=1, max_nodes=1)
        with pytest.raises(CuaCliError, match="does not authorize"):
            owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    finally:
        owned.stop()
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)


def test_owned_runtime_honors_raw_input_environment_in_actual_skill(runtime, monkeypatch) -> None:
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "false")
    bridge = HostExecutionBridge()
    try:
        result = bridge.execute_script(
            str(SCRIPTS / "snapshot.py"),
            {"session_id": "disabled-input"},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )
        assert result["success"] is True
        start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
        assert start["params"]["arguments"]["allowed_actions"] == []
    finally:
        bridge.shutdown_script_execution()
    assert process.returncode == 0


def test_response_correlation_failure_closes_without_replay(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    process.bad_id = True
    with pytest.raises(CuaCliError, match="response id"):
        owned.snapshot(max_depth=1, max_nodes=1)
    assert process.returncode == 0
    assert process.snapshots == 1


def test_exact_target_title_constraint_is_enforced(runtime) -> None:
    config, process, _ = runtime
    with pytest.raises(CuaCliError, match="title constraint"):
        PixelsMcpHostClient(
            session_id="narrowed",
            dcc_type="unreal",
            process_id=42,
            window_handle=500,
            allow_raw_input=True,
            options=config,
            window_title="Another window",
        )
    assert process.returncode == 0


def test_forced_child_cleanup_is_an_error_on_repeated_stop(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)

    def wait(timeout):
        if process.returncode is None:
            raise subprocess.TimeoutExpired("owned fixture", timeout)
        return process.returncode

    process.wait = wait
    process.stdin.close = lambda: None
    with pytest.raises(CuaCliError, match="forced termination"):
        owned.stop()
    with pytest.raises(CuaCliError, match="forced termination"):
        owned.stop()
    assert process.returncode == 0


def test_semantic_fields_are_rejected_without_any_action_request(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    try:
        with pytest.raises(CuaCliError, match="Semantic and secret"):
            owned.execute(
                {
                    "action": "click",
                    "input_kind": "raw_input",
                    "x": 1,
                    "y": 2,
                    "element_token": "forged-semantic-control",
                }
            )
        assert not any(
            r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests
        )
    finally:
        owned.stop()


@pytest.mark.parametrize("operation", ["activate", "restore", "minimize", "show"])
def test_default_window_ceiling_forbids_mutation_without_request(runtime, operation) -> None:
    config, process, _ = runtime
    owned = client(config)
    try:
        start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
        methods = start["params"]["arguments"]["allowed_methods"]
        assert "change_window_state" not in methods and "minimize_window" not in methods
        with pytest.raises(CuaCliError, match="owner did not authorize"):
            owned.change_window_state(operation)
        assert not any(
            r.get("params", {}).get("arguments", {}).get("method") in {"change_window_state", "minimize_window"}
            for r in process.requests
        )
    finally:
        owned.stop()


@pytest.mark.parametrize("operations", [("show",), ("activate", "activate"), ["minimize"]])
def test_window_options_are_closed_and_owner_typed(tmp_path, operations) -> None:
    with pytest.raises(ValueError, match="window_operations"):
        options(tmp_path, window_operations=operations)


@pytest.mark.parametrize("fault", ["capabilities", "protocol", "missing_schema", "wrong_schema"])
def test_handshake_schema_and_capability_mismatch_fail_before_task(runtime, fault) -> None:
    config, process, _ = runtime
    if fault == "capabilities":
        process.mutate_initialize = lambda raw: raw.pop("capabilities")
    elif fault == "protocol":
        process.mutate_initialize = lambda raw: raw.update(protocolVersion="2024-11-05")
    elif fault == "missing_schema":
        process.mutate_catalog = lambda raw: raw["tools"][0].pop("inputSchema")
    else:
        process.mutate_catalog = lambda raw: raw["tools"][1]["inputSchema"]["properties"]["params"].update(
            type="string"
        )
    with pytest.raises(CuaCliError):
        client(config)
    assert process.returncode == 0
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


@pytest.mark.parametrize("operation,wire_operation", [("activate", "activate"), ("restore", "restore_activate")])
def test_explicit_activation_retains_native_state_and_requires_new_pixels(runtime, operation, wire_operation) -> None:
    config, process, _ = runtime
    owned = client(replace(config, window_operations=(wire_operation,)))
    try:
        owned.snapshot(max_depth=1, max_nodes=1)
        changed = owned.change_window_state(operation)
        request = process.requests[-1]["params"]["arguments"]
        assert request["method"] == "change_window_state"
        assert request["params"] == {"operation": wire_operation}
        assert changed["state"]["foreground"] is True
        assert changed["result"]["automatic_input"] is False
        assert changed["result"]["target"]["window_id"] == TARGET["window_handle"]
        assert process.snapshots == 1  # Explicit window operations never take pixels.
        with pytest.raises(CuaCliError, match="fresh pixels"):
            owned.execute({"action": "click", "input_kind": "raw_input"})
    finally:
        owned.stop()


def test_minimize_scope_is_native_window_state_and_consumes_own_observation(runtime) -> None:
    config, process, _ = runtime
    owned = client(replace(config, allowed_actions=(), window_operations=("minimize",)))
    try:
        start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
        proposal = start["params"]["arguments"]
        assert proposal["allowed_methods"] == ["snapshot", "get_window_state", "minimize_window"]
        assert proposal["allowed_actions"] == [
            {
                "action": "minimize_window",
                "input_kind": "window_state",
                "secret_input": False,
                "authorization_category": "window_state",
            }
        ]
        with pytest.raises(CuaCliError, match="fresh pixels"):
            owned.change_window_state("minimize")
        owned.snapshot(max_depth=1, max_nodes=1)
        changed = owned.change_window_state("minimize")
        request = process.requests[-1]["params"]["arguments"]
        assert request["params"] == {"observation_id": "native-obs-1"}
        assert changed["state"]["minimized"] is True
        assert changed["state"]["foreground"] is False
        assert changed["result"]["effect"] == "confirmed"
        assert changed["task_context"]["task_id"] == "public-task"
        with pytest.raises(CuaCliError, match="fresh pixels"):
            owned.change_window_state("minimize")
        assert process.snapshots == 1
    finally:
        owned.stop()


@pytest.mark.parametrize("fault", ["target", "native_instance", "foreground", "observation"])
def test_wrong_window_completion_never_returns_success_and_closes(runtime, fault) -> None:
    config, process, _ = runtime
    operation = "minimize" if fault == "observation" else "activate"
    owned = client(replace(config, window_operations=(operation,)))
    owned.snapshot(max_depth=1, max_nodes=1)

    def corrupt(raw):
        if fault == "target":
            raw["result"]["target"]["window_id"] = 501
        elif fault == "native_instance":
            raw["state"]["native_instance"]["window_thread_id"] = 99
        elif fault == "foreground":
            raw["state"]["foreground"] = False
        else:
            raw["result"]["observation_id"] = "foreign-observation"

    process.mutate_window = corrupt
    with pytest.raises(CuaCliError):
        owned.change_window_state(operation)
    assert process.returncode == 0
    assert process.snapshots == 1


@pytest.mark.parametrize("field", ["delivery_completed", "post_dispatch_validated", "missing", "wrong_type"])
def test_native_physical_ack_requires_actual_delivery_and_post_dispatch_validation(runtime, field) -> None:
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)

    def corrupt(raw):
        if field == "missing":
            raw["result"].pop("delivery")
        elif field == "wrong_type":
            raw["result"]["delivery"]["delivery_completed"] = 1
        else:
            raw["result"]["delivery"][field] = False

    process.mutate_action = corrupt
    with pytest.raises(CuaCliError, match="native fence"):
        owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    assert process.returncode == 0


def test_formal_window_route_requires_matching_minimize_token_and_retains_native_outcome(runtime, monkeypatch) -> None:
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    config = replace(config, allowed_actions=(), window_operations=("activate", "minimize"))
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "false")
    bridge = HostExecutionBridge()

    def call(name, args):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "window-test", **args},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    try:
        activated = call("act", {"action": "activate_window"})
        assert activated["success"] is True
        assert activated["context"]["window_state"]["foreground"] is True
        assert process.snapshots == 0
        snapshot = call("snapshot", {})
        assert snapshot["success"] is True
        stale = call("act", {"action": "minimize_window", "snapshot_id": "foreign-id"})
        assert stale["success"] is False and stale["error"] == "stale_observation"
        minimized = call("act", {"action": "minimize_window", "snapshot_id": snapshot["context"]["snapshot_id"]})
        assert minimized["success"] is True
        assert minimized["context"]["native_outcome"]["observation_id"] == "native-obs-1"
        assert minimized["context"]["task_context"]["task_id"] == "public-task"
        assert minimized["context"]["fresh_observation_required"] is True
        assert process.snapshots == 1
        assert call("act", {"action": "minimize_window", "snapshot_id": "native-obs-1"})["success"] is False
        start = next(r for r in process.requests if r.get("params", {}).get("name") == "start_task")
        assert "execute_action" not in start["params"]["arguments"]["allowed_methods"]
        assert call("stop_computer_use", {})["success"] is True
    finally:
        bridge.shutdown_script_execution()


@pytest.mark.parametrize("input_sent,attempted", [("sent", True), ("unknown", True), ("not_sent", False)])
def test_actual_native_error_wire_retains_partial_unknown_evidence_and_consumes_snapshot(
    runtime, input_sent, attempted
) -> None:
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    bridge = HostExecutionBridge()

    def call(name, args):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "partial-test", **args},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    def reject(raw):
        context = raw["task_context"]
        raw.clear()
        raw.update(
            type="error",
            code="input_failed" if input_sent != "unknown" else "completion_unknown",
            message="Guarded input did not complete.",
            task_context=context,
            details={
                "phase": "local_mutation_dispatch" if attempted else "pre_dispatch",
                "action_attempted": attempted,
                "input_sent": input_sent,
                "completion": "unknown" if attempted else "known",
                "effect_unknown": attempted,
                "automatic_input": False,
                "blind_retry": False,
                "fresh_observation_required": True,
                "text": "PRIVATE_INPUT_MUST_NOT_ESCAPE",
                "path": "PRIVATE_PATH_MUST_NOT_ESCAPE",
            },
            native_outcome={
                "delivery_completed": False,
                "post_dispatch_validated": False,
                "mutation_attempted": attempted,
                "inserted_events": int(input_sent == "sent"),
                "text": "PRIVATE_OUTCOME_MUST_NOT_ESCAPE",
            },
        )

    try:
        snapshot = call("snapshot", {})
        assert snapshot["success"] is True
        process.mutate_action = reject
        args = {"snapshot_id": snapshot["context"]["snapshot_id"], "action": "click", "x": 1, "y": 2}
        rejected = call("act", args)
        assert rejected["success"] is False
        context = rejected["context"]
        assert context["details"]["input_sent"] == input_sent
        assert context["details"]["action_attempted"] is attempted
        assert context["task_context"]["task_id"] == "public-task"
        assert context["native_outcome"]["mutation_attempted"] is attempted
        assert context["fresh_observation_required"] is True and context["blind_retry"] is False
        assert "PRIVATE_" not in json.dumps(rejected)
        assert call("act", args)["error"] == "stale_observation"
        attempts = [
            r for r in process.requests if r.get("params", {}).get("arguments", {}).get("method") == "execute_action"
        ]
        assert len(attempts) == 1 and process.snapshots == 1
    finally:
        bridge.shutdown_script_execution()


def test_foreign_error_task_identity_is_rejected_and_closed(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)

    def reject(raw):
        raw.update(type="error", code="input_failed", message="Native rejection.")
        raw["task_context"]["target"]["window_handle"] = 501

    process.mutate_action = reject
    with pytest.raises(CuaCliError, match="PID/HWND"):
        owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
    assert process.returncode == 0


def test_pixels_recovery_never_suggests_ungranted_show_or_activation(runtime) -> None:
    config, _process, _ = runtime
    spec = importlib.util.spec_from_file_location("_pixels_recovery_backend", SCRIPTS / "_cua_backend.py")
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    try:
        for configured, expected in [
            (config, ["get_window_state"]),
            (replace(config, window_operations=("restore_activate",)), ["get_window_state", "restore_window"]),
        ]:
            result = backend._host_error(
                CuaCliError("invalid_target", "Exact target is minimized."), {"trusted_ui_control_runtime": configured}
            )
            assert result["context"]["recovery_actions"] == expected
            assert "show_window" not in result["prompt"]
    finally:
        backend.cleanup()


def test_minimize_retains_actual_foreground_status_without_inventing_blur(runtime) -> None:
    config, process, _ = runtime
    owned = client(replace(config, window_operations=("minimize",)))
    try:
        owned.snapshot(max_depth=1, max_nodes=1)
        process.mutate_window = lambda raw: raw["state"].update(foreground=True)
        assert owned.change_window_state("minimize")["state"]["foreground"] is True
    finally:
        owned.stop()


def test_native_rejected_delivery_is_failure_with_retained_outcome(runtime) -> None:
    config, process, _ = runtime
    owned = client(config)
    try:
        owned.snapshot(max_depth=1, max_nodes=1)

        def reject(raw):
            raw["success"] = raw["result"]["success"] = False
            raw["result"]["delivery"]["post_dispatch_validated"] = False

        process.mutate_action = reject
        result = owned.execute({"action": "click", "input_kind": "raw_input", "x": 1, "y": 2})
        assert result["success"] is False
        assert result["result"]["delivery"]["delivery_completed"] is True
        assert result["fresh_observation_required"] is True
        with pytest.raises(CuaCliError, match="fresh pixels"):
            owned.execute({"action": "click", "input_kind": "raw_input"})
    finally:
        owned.stop()
