"""Tests for reusable sidecar action dispatch helpers."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Mapping

import pytest

import dcc_mcp_core
from dcc_mcp_core.sidecar import ERROR_DISPATCH_FAILED
from dcc_mcp_core.sidecar import ERROR_NO_SOURCE_FILE
from dcc_mcp_core.sidecar import ERROR_PAYLOAD_MALFORMED
from dcc_mcp_core.sidecar import ERROR_SERVER_NOT_RUNNING
from dcc_mcp_core.sidecar import ERROR_UNKNOWN_ACTION
from dcc_mcp_core.sidecar import SidecarActionDispatcher
from dcc_mcp_core.sidecar import SidecarDispatchRequest


def test_sidecar_dispatcher_exported_from_top_level() -> None:
    assert dcc_mcp_core.SidecarActionDispatcher is SidecarActionDispatcher
    assert dcc_mcp_core.SidecarDispatchRequest is SidecarDispatchRequest
    assert "SidecarActionDispatcher" in dcc_mcp_core.__all__
    assert "SidecarDispatchRequest" in dcc_mcp_core.__all__


def test_maya_style_executor_resolves_registered_action_source(tmp_path: Path) -> None:
    skill_root = tmp_path / "maya_skill"
    script = skill_root / "scripts" / "create_cube.py"
    script.parent.mkdir(parents=True)
    script.write_text("def main(**_): return {'success': True}\n", encoding="utf-8")
    server = object()
    calls: list[dict[str, Any]] = []

    def resolve_action(action: str, **_: Any) -> Mapping[str, Any]:
        assert action == "maya__create_cube"
        return {
            "source_file": "scripts/create_cube.py",
            "skill_name": "maya_primitives",
            "thread_affinity": "main",
            "execution": "sync",
            "timeout_hint_secs": 30,
        }

    def execute_in_process(
        active_server: Any,
        script_path: str,
        args: Mapping[str, Any],
        action_name: str,
    ) -> Mapping[str, Any]:
        calls.append(
            {
                "server": active_server,
                "script_path": script_path,
                "args": dict(args),
                "action_name": action_name,
            },
        )
        return {
            "success": True,
            "message": "created cube",
            "context": {"object_name": args["name"]},
        }

    dispatcher = SidecarActionDispatcher(
        "maya",
        server_provider=lambda: server,
        action_resolver=resolve_action,
        executor=SidecarActionDispatcher.maya_executor(execute_in_process),
        bundled_skill_roots=[skill_root],
    )

    result = dispatcher.dispatch_payload(
        {
            "action": "maya__create_cube",
            "args": {"name": "pCube1"},
            "request_id": "req-1",
        },
    )

    assert result == {
        "success": True,
        "message": "created cube",
        "context": {"object_name": "pCube1"},
    }
    assert calls == [
        {
            "server": server,
            "script_path": str(script),
            "args": {"name": "pCube1"},
            "action_name": "maya__create_cube",
        },
    ]


def test_3dsmax_style_executor_wraps_plain_script_result(tmp_path: Path) -> None:
    script = tmp_path / "scripts" / "box.py"
    script.parent.mkdir()
    script.write_text("def main(**_): return {'ok': True}\n", encoding="utf-8")
    calls: list[tuple[str, Mapping[str, Any]]] = []

    def run_skill_script(script_path: str, args: Mapping[str, Any]) -> Mapping[str, Any]:
        calls.append((script_path, dict(args)))
        return {"node": "Box001", "script": Path(script_path)}

    dispatcher = SidecarActionDispatcher(
        "3dsmax",
        server_provider=lambda: {"running": True},
        executor=SidecarActionDispatcher.script_executor(run_skill_script),
    )

    result = dispatcher.dispatch_payload(
        {
            "action": "max__create_box",
            "args": {"length": 10},
            "source_file": str(script),
            "request_id": "req-2",
        },
    )

    assert result == {
        "success": True,
        "message": "Sidecar action dispatched",
        "context": {
            "dcc_name": "3dsmax",
            "action": "max__create_box",
            "request_id": "req-2",
            "script_path": str(script),
            "result": {"node": "Box001", "script": str(script)},
        },
    }
    assert calls == [(str(script), {"length": 10})]


@pytest.mark.parametrize(
    ("payload", "reason"),
    [
        (None, None),
        ([], None),
        ({}, "missing-action"),
        ({"action": ""}, "missing-action"),
        ({"action": "x", "args": []}, "invalid-args"),
        ({"action": "x", "request_id": 1}, "invalid-request-id"),
        ({"action": "x", "script_path": ""}, "invalid-script-path"),
        ({"action": "x", "source_file": 7}, "invalid-source-file"),
    ],
)
def test_malformed_payloads_return_payload_malformed(payload: Any, reason: str | None) -> None:
    dispatcher = SidecarActionDispatcher("maya", server_provider=lambda: object())

    result = dispatcher.dispatch_payload(payload)  # type: ignore[arg-type]

    assert result["success"] is False
    assert result["error"] == ERROR_PAYLOAD_MALFORMED
    if reason is not None:
        assert result["context"]["reason"] == reason


def test_server_provider_none_returns_server_not_running(tmp_path: Path) -> None:
    script = tmp_path / "tool.py"
    script.write_text("def main(): return None\n", encoding="utf-8")
    dispatcher = SidecarActionDispatcher("maya", server_provider=lambda: None)

    result = dispatcher.dispatch_payload({"action": "maya__noop", "script_path": str(script)})

    assert result["success"] is False
    assert result["error"] == ERROR_SERVER_NOT_RUNNING
    assert result["context"]["dcc_name"] == "maya"
    assert result["context"]["action"] == "maya__noop"


def test_missing_server_provider_returns_server_not_running(tmp_path: Path) -> None:
    script = tmp_path / "tool.py"
    script.write_text("def main(): return None\n", encoding="utf-8")
    dispatcher = SidecarActionDispatcher("maya")

    result = dispatcher.dispatch_payload({"action": "maya__noop", "script_path": str(script)})

    assert result["success"] is False
    assert result["error"] == ERROR_SERVER_NOT_RUNNING
    assert result["context"]["reason"] == "missing-server-provider"


def test_unknown_action_from_resolver_returns_unknown_action() -> None:
    dispatcher = SidecarActionDispatcher(
        "maya",
        server_provider=lambda: object(),
        action_resolver=lambda _action: None,
    )

    result = dispatcher.dispatch_payload({"action": "maya__missing", "args": {}})

    assert result["success"] is False
    assert result["error"] == ERROR_UNKNOWN_ACTION
    assert result["context"]["action"] == "maya__missing"


def test_resolved_action_without_source_returns_no_source_file() -> None:
    dispatcher = SidecarActionDispatcher(
        "maya",
        server_provider=lambda: object(),
        action_resolver=lambda _action: {"skill_name": "broken_skill"},
    )

    result = dispatcher.dispatch_payload({"action": "maya__broken", "args": {}})

    assert result["success"] is False
    assert result["error"] == ERROR_NO_SOURCE_FILE
    assert result["context"]["action"] == "maya__broken"


def test_executor_failure_returns_dispatch_failed(tmp_path: Path) -> None:
    script = tmp_path / "boom.py"
    script.write_text("def main(): return None\n", encoding="utf-8")

    def fail(_request: SidecarDispatchRequest) -> Any:
        raise RuntimeError("host executor stopped")

    dispatcher = SidecarActionDispatcher(
        "maya",
        server_provider=lambda: object(),
        executor=fail,
    )

    result = dispatcher.dispatch_payload({"action": "maya__boom", "source_file": str(script)})

    assert result["success"] is False
    assert result["error"] == ERROR_DISPATCH_FAILED
    assert result["context"]["error_type"] == "RuntimeError"
    assert result["context"]["error_message"] == "host executor stopped"
    assert "Traceback" in result["context"]["traceback"]


def test_executor_receives_resolved_request_metadata(tmp_path: Path) -> None:
    skill_root = tmp_path / "skill"
    script = skill_root / "scripts" / "report.py"
    script.parent.mkdir(parents=True)
    script.write_text("def main(): return None\n", encoding="utf-8")
    seen: list[SidecarDispatchRequest] = []

    def execute(request: SidecarDispatchRequest) -> Mapping[str, Any]:
        seen.append(request)
        return {
            "success": True,
            "message": "ok",
            "context": {
                "skill_name": request.skill_name,
                "affinity": request.thread_affinity,
                "timeout": request.timeout_hint_secs,
            },
        }

    dispatcher = SidecarActionDispatcher(
        "houdini",
        server_provider=lambda: object(),
        action_resolver=lambda _action: {
            "path": "report.py",
            "skill": "houdini_report",
            "affinity": "any",
            "execution": "async",
            "timeout_hint_secs": "12",
        },
        executor=execute,
        bundled_skill_roots=[skill_root],
    )

    result = dispatcher.dispatch_payload({"action": "houdini__report", "args": {"detail": True}})

    assert result["success"] is True
    assert result["context"] == {"skill_name": "houdini_report", "affinity": "any", "timeout": 12}
    assert seen[0].script_path == str(script)
    assert seen[0].source_file == "report.py"
    assert seen[0].args == {"detail": True}
