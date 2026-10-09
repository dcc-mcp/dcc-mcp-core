"""Source-frozen passive MCP wire contracts; no executable or UI is started."""

from __future__ import annotations

import base64
from copy import deepcopy
from dataclasses import replace
import json
from pathlib import Path
import threading

import jsonschema
import pytest

from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core.cancellation import CancelToken
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_errors import safe_failure_evidence
from dcc_mcp_core.host.ui_control_options import UiControlCapturePreparationOptions
from test_cua_mcp_pixels import PNG
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import FakeMcpProcess
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import options
from test_cua_mcp_pixels import pixel_response

FIXTURES = Path(__file__).parent / "fixtures/cua_capture_preparation"
INSTANCE = pixel_response()["observation"]["capture_provenance"]["native_instance"]


def status(root, *, phase="active"):
    window = {
        "identity": {
            "process_id": 42,
            "window_handle": 500,
            "native_instance": dict(INSTANCE),
            "executable": {
                "canonical_image_path": "C:/fixture/editor.exe",
                "volume_serial_number": 1,
                "file_id": [1] * 16,
            },
        },
        "topmost": False,
        "bounds": [-640, 0, 640, 480],
        "visible_bounds": [-640, 0, 640, 480],
        "dpi": 144,
        "visible": True,
        "minimized": False,
        "foreground": False,
        "anchors": {"above": None, "below": None},
    }
    return {
        "preparation_id": [7] * 16,
        "phase": phase,
        "deadline_ms": 10000,
        "pending_sequence": None,
        "capture_revoked": phase != "active",
        "cleanup_verified": phase == "restored",
        "original": [deepcopy(window)],
        "last_mutation": None,
        "failure": None,
        "journal_path": str(root / "epoch" / "journal.json"),
        "affected_readback": [deepcopy(window)],
        "last_completed_sequence": 0,
    }


class PreparationProcess(FakeMcpProcess):
    def __init__(self, root):
        super().__init__()
        self.root = root
        self.prepared = False
        self.state = None
        self.metadata_index = 0
        self.timestamp = 100
        self.mutate_preparation = lambda raw: None
        self.after_preparation = lambda operation: None
        self.mutate_catalog = self.catalog
        self.mutate_window = self.metadata
        self.mutate_stop = self.cleanup

    def catalog(self, value):
        for tool in value["tools"]:
            if tool["name"] in {"start_task", "dcc_cua_task_call"}:
                tool["inputSchema"] = json.loads((FIXTURES / (tool["name"] + ".input.schema.json")).read_text())

    def metadata(self, raw):
        if raw["type"] == "window_state":
            self.metadata_index += 1
            raw["session_id"] = "mcp-public-task"
            raw["state"]["visible_bounds"] = [-640, 0, 640, 480]
            raw["state"]["window_state_id"] = "metadata-" + str(self.metadata_index)

    def cleanup(self, raw):
        if self.prepared:
            raw["cleanup"]["capture_preparation"] = status(self.root, phase="restored")

    def write(self, data):
        request = json.loads(data)
        public = request.get("params", {})
        name = public.get("name")
        if name in {"start_task", "dcc_cua_task_call"}:
            schema = json.loads((FIXTURES / (name + ".input.schema.json")).read_text())
            jsonschema.Draft202012Validator(schema).validate(public["arguments"])
        operation = public.get("arguments", {}).get("method", "")
        if not operation.startswith("capture_preparation_"):
            return super().write(data)
        self.requests.append(request)
        if self.drop_response:
            return
        self.prepared = True
        if operation == "capture_preparation_begin":
            self.state = status(self.root)
        elif operation == "capture_preparation_stop":
            self.state = status(self.root, phase="restored")
        raw = {"type": operation, "session_id": "mcp-public-task", "task_context": pixel_response()["task_context"]}
        content = []
        if operation == "capture_preparation_snapshot":
            self.timestamp += 1
            raw.update(
                passive=True,
                input_authorized=False,
                metadata={
                    "schema": "dcc-cua-passive-prepared-evidence-v1",
                    "passive": True,
                    "input_authorized": False,
                    "preparation_id": [7] * 16,
                    "process_id": 42,
                    "window_handle": 500,
                    "native_instance": dict(INSTANCE),
                    "foreground_at_capture": False,
                    "foreground_at_publication": False,
                    "captured_at_ms": self.timestamp,
                    "width": 640,
                    "height": 480,
                    "bounds": [-640, 0, 640, 480],
                    "whole_desktop_capture": False,
                },
                image={"length": len(PNG), "mime_type": "image/png"},
                images=[],
                use_shared_memory=False,
            )
            content = [{"type": "image", "mimeType": "image/png", "data": base64.b64encode(PNG).decode()}]
        else:
            raw["result"] = deepcopy(self.state)
        self.mutate_preparation(raw)
        self.after_preparation(operation)
        self.lines.put(
            (
                json.dumps(
                    {
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {"structuredContent": raw, "content": content, "isError": False},
                    }
                )
                + "\n"
            ).encode()
        )


@pytest.fixture
def runtime(tmp_path, monkeypatch):
    root = tmp_path / "journal"
    root.mkdir()
    config = options(tmp_path, allowed_actions=(), capture_preparation=UiControlCapturePreparationOptions(str(root)))
    process = PreparationProcess(root)
    launches = []

    def popen(command, **kwargs):
        launches.append((command, kwargs))
        return process

    monkeypatch.setattr("dcc_mcp_core.host.cua_mcp_transport.subprocess.Popen", popen)
    for key in ("PROCESS_ID", "WINDOW_HANDLE", "WINDOW_TITLE", "PROCESS_NAME", "DCC_TYPE"):
        monkeypatch.delenv("DCC_MCP_UI_CONTROL_" + key, raising=False)
    return config, process, launches


@pytest.fixture
def route(runtime):
    config, process, launches = runtime
    bridge = HostExecutionBridge()

    def call(operation, extra=None, *, token=None, owner=config, action="capture_preparation"):
        return bridge.execute_script(
            str(SCRIPTS / (action + ".py")),
            {
                "session_id": "passive-test",
                **TARGET,
                **({"operation": operation} if operation else {}),
                **(extra or {}),
            },
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "python", **TARGET},
            trusted_ui_control_runtime=owner,
            cancel_token=token,
        )

    yield call, process, launches, config
    call(None, action="stop_computer_use")
    bridge.shutdown_script_execution()


def begin(call):
    state = call(None, {"action": "get_window_state"}, action="act")
    assert state["success"], state
    token = state["context"]["window_state"]["window_state_id"]
    return call("begin", {"window_state_id": token, "lifetime_ms": 1000})


def test_exact_wire_and_passive_image_have_no_action_token(runtime):
    config, process, launches = runtime
    owned = client(config)
    state = owned.window_state()["state"]
    owned.preparation.call("begin", window_state_id=state["window_state_id"], lifetime_ms=1000)
    snap = owned.preparation.call("snapshot")
    assert snap["image_bytes"] == PNG and snap["metadata"]["foreground_at_capture"] is False
    assert owned._observation_id is None and owned._window_state_id is None
    assert "observation_id" not in snap and "window_state_id" not in snap
    proposal = next(
        r["params"]["arguments"] for r in process.requests if r.get("params", {}).get("name") == "start_task"
    )
    assert proposal["allow_capture_preparation"] is True
    assert "execute_action" not in proposal["allowed_methods"]
    assert proposal["allowed_actions"] == [
        {
            "action": "capture_preparation_begin",
            "input_kind": "window_state",
            "secret_input": False,
            "authorization_category": "window_state",
        }
    ]
    assert launches[0][1]["env"]["DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT"] == config.capture_preparation.journal_root
    assert owned.preparation.call("stop")["result"]["cleanup_verified"]
    assert owned.stop()["capture_preparation"]["cleanup_verified"]


def test_formal_route_returns_passive_pixels_and_preserves_restoration(route):
    call, _, launches, _ = route
    assert begin(call)["success"]
    result = call("snapshot")
    assert result["success"], result
    assert result["context"]["__rich__"]["kind"] == "image"
    assert result["context"]["input_authorized"] is False
    assert "snapshot_id" not in result["context"]
    assert call("stop")["context"]["capture_preparation"]["phase"] == "restored"
    assert len(launches) == 1


@pytest.mark.parametrize(
    "field,value",
    [
        ("observation_id", "forged"),
        ("window_state_id", None),
        ("process_id", 99),
        ("native_instance", {}),
        ("preparation_id", [8] * 16),
        ("captured_at_ms", 10000),
        ("width", 641),
        ("input_authorized", True),
    ],
)
def test_rejected_passive_pixels_revoke_and_never_publish(route, field, value):
    call, process, _, _ = route
    assert begin(call)["success"]

    def mutate(raw):
        if raw["type"].endswith("snapshot"):
            raw["metadata"][field] = value

    process.mutate_preparation = mutate
    result = call("snapshot")
    assert not result["success"] and "__rich__" not in result["context"]
    assert result["context"]["capture_preparation"]["cleanup_verified"]


@pytest.mark.parametrize("operation", ["capture_preparation_begin", "capture_preparation_snapshot"])
def test_cancellation_after_native_receipt_revokes(route, operation):
    call, process, _, _ = route
    token = CancelToken()
    if operation.endswith("snapshot"):
        assert begin(call)["success"]
    process.after_preparation = lambda actual: token.cancel() if actual == operation else None
    if operation.endswith("begin"):
        state = call(None, {"action": "get_window_state"}, action="act")["context"]["window_state"]
        result = call("begin", {"window_state_id": state["window_state_id"], "lifetime_ms": 1000}, token=token)
    else:
        result = call("snapshot", token=token)
    assert result["error"] == "cancelled", result
    assert result["context"]["capture_preparation"]["cleanup_verified"]
    assert "__rich__" not in result["context"]


def test_unknown_restoration_is_retained(route):
    call, process, _, _ = route
    assert begin(call)["success"]

    def mutate(raw):
        if raw["type"].endswith("stop"):
            raw["result"].update(
                phase="cleanup_unknown", cleanup_verified=False, failure={"reason": "worker_lost", "os_error": 5}
            )

    process.mutate_preparation = mutate
    result = call("stop")
    assert result["error"] == "cleanup_unknown"
    assert result["context"]["capture_preparation"]["failure"] == {"reason": "worker_lost", "os_error": 5}


def test_disabled_owner_denies_before_launch(route):
    call, _, launches, config = route
    result = call(
        "begin", {"window_state_id": "invented", "lifetime_ms": 1000}, owner=replace(config, capture_preparation=None)
    )
    assert result["error"] == "permission_denied" and not launches


def test_typed_failure_projection_preserves_reason_without_arbitrary_text():
    result = safe_failure_evidence(
        {"details": {"capture_preparation": {"reason": "expired", "os_error": 5, "secret": "omit"}}}
    )
    assert result == {"details": {"capture_preparation": {"reason": "expired", "os_error": 5}}}


@pytest.mark.parametrize("lifetime", [0, 30001, True, "1000"])
def test_invalid_lifetime_consumes_metadata_without_native_mutation(runtime, lifetime):
    config, process, _ = runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]
    with pytest.raises(CuaCliError):
        owned.preparation.call("begin", window_state_id=token, lifetime_ms=lifetime)
    assert not process.prepared and owned._window_state_id is None
    owned.stop()


def test_metadata_one_use_and_input_evidence_invalidated(runtime):
    config, _, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    token = owned.window_state()["state"]["window_state_id"]
    owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    assert owned._observation_id is None
    with pytest.raises(CuaCliError, match="fresh same-session"):
        owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    owned.stop()


def test_transport_timeout_does_not_claim_restoration_or_retry(runtime):
    config, process, _ = runtime
    owned = client(replace(config, timeout_seconds=0.02))
    token = owned.window_state()["state"]["window_state_id"]
    owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    process.drop_response = True
    with pytest.raises(CuaCliError) as failed:
        owned.preparation.call("snapshot")
    assert failed.value.code == "timeout"
    assert owned._observation_id is None and owned._window_state_id is None
    assert not owned.preparation.status["cleanup_verified"]
    methods = [r.get("params", {}).get("arguments", {}).get("method") for r in process.requests]
    assert methods.count("capture_preparation_snapshot") == 1
    with pytest.raises(CuaCliError) as cleanup:
        owned.stop()
    assert cleanup.value.code == "cleanup_unknown"


@pytest.mark.parametrize("corruption", ["target", "instance", "preparation", "deadline", "affected_scope", "journal"])
def test_changed_status_binding_fails_closed(route, corruption):
    call, process, _, _ = route
    assert begin(call)["success"]

    def mutate(raw):
        if not raw["type"].endswith("state"):
            return
        value = raw["result"]
        if corruption == "target":
            value["original"][0]["identity"]["window_handle"] += 1
        elif corruption == "instance":
            value["original"][0]["identity"]["native_instance"]["process_creation_time_100ns"] += 1
        elif corruption == "preparation":
            value["preparation_id"] = [9] * 16
        elif corruption == "deadline":
            value["deadline_ms"] += 1
        elif corruption == "affected_scope":
            value["affected_readback"][0]["identity"]["process_id"] += 1
        else:
            value["journal_path"] = str(process.root.parent / "foreign.json")

    process.mutate_preparation = mutate
    result = call("state")
    assert result["success"] is False
    assert result["context"]["capture_preparation"]["cleanup_verified"]


@pytest.mark.parametrize("kind", ["omitted", "unknown", "nested"])
def test_stop_ack_requires_actual_restoration_component(runtime, kind):
    config, process, _ = runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]
    owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    original = process.mutate_stop

    def mutate(raw):
        original(raw)
        ack = raw["cleanup"]
        if kind == "omitted":
            ack.pop("capture_preparation")
        elif kind == "unknown":
            ack["capture_preparation"].update(phase="restore_pending", cleanup_verified=False, pending_sequence=2)
        else:
            ack["host_response"] = {**ack}
            ack.pop("capture_preparation")

    process.mutate_stop = mutate
    if kind == "nested":
        assert owned.stop()["capture_preparation"]["cleanup_verified"]
    else:
        with pytest.raises(CuaCliError) as failed:
            owned.stop()
        assert failed.value.code == "cleanup_unknown"
        if kind == "unknown":
            assert failed.value.native_evidence["capture_preparation"]["phase"] == "restore_pending"


def test_inherited_journal_does_not_enable_preparation(runtime, monkeypatch):
    config, process, launches = runtime
    monkeypatch.setenv("DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT", "C:/untrusted")
    owned = client(replace(config, capture_preparation=None))
    assert "DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT" not in launches[0][1]["env"]
    proposal = next(
        r["params"]["arguments"] for r in process.requests if r.get("params", {}).get("name") == "start_task"
    )
    assert "allow_capture_preparation" not in proposal
    owned.stop()


def test_optional_snapshot_ceiling_and_narrow_policy_allow_stop(route):
    call, _, _, config = route
    restricted = replace(config, capture_preparation=replace(config.capture_preparation, allow_snapshot=False))

    def bounded(operation, extra=None):
        return call(operation, extra, owner=restricted)

    state = call(None, {"action": "get_window_state"}, owner=restricted, action="act")["context"]["window_state"]
    assert bounded("begin", {"window_state_id": state["window_state_id"], "lifetime_ms": 1000})["success"]
    assert bounded("snapshot")["error"] == "permission_denied"
    result = bounded("stop", {"policy": {"allow_snapshot": False, "allow_mutating_actions": False}})
    assert result["success"], result


def test_queued_cancel_serializes_and_revokes_same_session(route):
    call, process, _, _ = route
    assert begin(call)["success"]
    entered, release = threading.Event(), threading.Event()

    def pause(operation):
        if operation == "capture_preparation_state":
            entered.set()
            assert release.wait(5)

    process.after_preparation = pause
    results = []
    first = threading.Thread(target=lambda: results.append(call("state")))
    first.start()
    assert entered.wait(5)
    queued = threading.Event()

    class QueuedToken(CancelToken):
        @property
        def cancelled(self):
            value = super().cancelled
            queued.set()
            return value

    token = QueuedToken()
    second = threading.Thread(target=lambda: results.append(call("snapshot", token=token)))
    second.start()
    assert queued.wait(5)
    token.cancel()
    release.set()
    first.join(5)
    second.join(5)
    assert not first.is_alive() and not second.is_alive()
    assert sorted(result.get("error") or "ok" for result in results) == ["cancelled", "ok"]
    methods = [r.get("params", {}).get("arguments", {}).get("method") for r in process.requests]
    assert "capture_preparation_snapshot" not in methods
    assert methods.count("capture_preparation_stop") == 1


def test_canonical_discovery_without_gateway_or_host_start(runtime, tmp_path):
    from dcc_mcp_core.server import DccServerOptions
    from dcc_mcp_core.server_base import DccServerBase

    config, _, launches = runtime
    skills = SCRIPTS.parent.parent
    bridge = HostExecutionBridge()
    settings = DccServerOptions.from_env(
        "unreal",
        skills,
        port=0,
        gateway_port=0,
        execution_bridge=bridge,
        registry_dir=str(tmp_path / "registry"),
        enable_gateway_failover=False,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_checkpoint_persistence=False,
        enable_checkpoint_tools=False,
        enable_telemetry=False,
        ui_control=config,
    )
    server = DccServerBase(settings)
    try:
        server.skill_discovery.register_builtin_actions(extra_skill_paths=[str(skills)], include_bundled=False)
        assert server.load_skill("ui-control")
        actions = [a for a in server.list_actions(dcc_name="python") if a["name"] == "ui_control__capture_preparation"]
        assert len(actions) == 1
        action = actions[0]
        assert Path(action["source_file"]).resolve() == (SCRIPTS / "capture_preparation.py").resolve()
        schema = action["input_schema"]
        if isinstance(schema, str):
            schema = json.loads(schema)
        args = {
            "session_id": "test",
            "process_id": 42,
            "window_handle": 500,
            "operation": "begin",
            "window_state_id": "metadata",
            "lifetime_ms": 1000,
        }
        jsonschema.validate(args, schema)
        with pytest.raises(jsonschema.ValidationError):
            jsonschema.validate({**args, "journal_root": "C:/injected"}, schema)
        assert not launches
    finally:
        server.stop()
        bridge.shutdown_script_execution()
