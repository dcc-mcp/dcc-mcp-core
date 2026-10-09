"""Owned cleanup evidence reaches real skill unload hooks without changing JSONL."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace

import pytest

from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.cua_mcp_recording import PixelsMcpRecording
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_recording import recording


@pytest.fixture(params=[False, True], ids=["ambient-clean", "ambient-conflict"])
def _ambient_target_scope(request, monkeypatch):
    if request.param:
        for name, value in {
            "DCC_MCP_UI_CONTROL_PROCESS_ID": "43",
            "DCC_MCP_UI_CONTROL_WINDOW_HANDLE": "84",
            "DCC_MCP_UI_CONTROL_WINDOW_TITLE": "Different Test Target",
            "DCC_MCP_UI_CONTROL_PROCESS_NAME": "powershell.exe",
            "DCC_MCP_UI_CONTROL_DCC_TYPE": "different-test-host",
        }.items():
            monkeypatch.setenv(name, value)


@pytest.fixture
def lifecycle_recording(recording, monkeypatch, _ambient_target_scope):
    # Operator scope intentionally overrides adapter scope in production. Bind
    # this fake runtime explicitly so another test or CI environment cannot
    # replace its PID/HWND/title or introduce a denied process-name constraint.
    for name, value in {
        "DCC_MCP_UI_CONTROL_PROCESS_ID": str(TARGET["process_id"]),
        "DCC_MCP_UI_CONTROL_WINDOW_HANDLE": str(TARGET["window_handle"]),
        "DCC_MCP_UI_CONTROL_WINDOW_TITLE": TARGET["window_title"],
        "DCC_MCP_UI_CONTROL_DCC_TYPE": "unreal",
    }.items():
        monkeypatch.setenv(name, value)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_PROCESS_NAME", raising=False)
    return recording


@pytest.fixture
def lifecycle_bridge():
    bridge = HostExecutionBridge()
    try:
        yield bridge
    finally:
        bridge.shutdown_script_execution()


def setup_diagnostic(result):
    # Assertion diagnostics must not echo arbitrary native messages, command
    # tokens, or response payloads into public CI logs.
    known_codes = {
        "invalid_target",
        "invalid_request",
        "policy_disabled",
        "backend_unavailable",
        "capture_failed",
        "permission_denied",
    }
    code = result.get("error")
    return {
        "success": result.get("success") is True,
        "error": code if isinstance(code, str) and code in known_codes else "unrecognized",
    }


def load_backend():
    spec = importlib.util.spec_from_file_location("_test_lifecycle_backend", SCRIPTS / "_cua_backend.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize("status", ["cleanup_failed", "cleanup_unknown"])
def test_real_skill_unload_reports_owned_partial_failure_and_attempts_legacy_peers(
    lifecycle_recording, lifecycle_bridge, caplog, monkeypatch, status
):
    # Other scripts may register synthetic origins which are not filesystem
    # paths. Windows Python 3.8 rejects resolving these even with strict=False.
    monkeypatch.setitem(
        sys.modules, "_test_synthetic_sidecar_origin", SimpleNamespace(__file__="<dcc-mcp-sidecar-bootstrap>")
    )
    config, process, _ = lifecycle_recording
    bridge = lifecycle_bridge
    session = "owned-lifecycle"
    result = bridge.execute_script(
        str(SCRIPTS / "recording_start.py"),
        {"session_id": session},
        skill_name="ui-control",
        trusted_ui_control_runtime=config,
        trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
    )
    success = result.get("success")
    assert success is True, setup_diagnostic(result)
    backend = next(
        module
        for module in list(sys.modules.values())
        if session in getattr(module, "_CLIENTS", {})
        and getattr(module, "__file__", None)
        and Path(module.__file__).resolve() == (SCRIPTS / "_cua_backend.py").resolve()
    )
    owned = backend._CLIENTS[session]["client"]
    attempts = []

    def legacy_stop():
        attempts.append("legacy-failed")
        raise RuntimeError("private-legacy-message-must-not-be-logged")

    backend._CLIENTS["legacy-failed"] = {"client": SimpleNamespace(stop=legacy_stop)}
    backend._CLIENTS["legacy-success"] = {"client": SimpleNamespace(stop=lambda: attempts.append("legacy-success"))}
    partial = str(Path(process.output) / "video.partial.mp4")

    def fail(raw):
        raw.update(ok=False, status=status)
        raw["cleanup"].update(
            success=False,
            cleanup_issues=[{"phase": "recording_stop", "code": "capture_failed", "message": "private-native-text"}],
            recording_video={"active": False, "finalized": False, "current_partial": partial, "segment_paths": []},
        )

    process.mutate_stop = fail
    try:
        # Catalog/module unload can succeed, but the native failure must be
        # visible through the existing stop-request and cleanup warnings.
        assert bridge.shutdown_script_execution() > 0
        warnings = [record for record in caplog.records if record.exc_info and "Skill package" in record.message]
        assert {record.message.split(" failed ")[0] for record in warnings} == {
            "Skill package stop request",
            "Skill package cleanup",
        }
        cached = owned._cleanup._stop_error
        assert isinstance(cached, OwnedCuaMcpError)
        for warning in warnings:
            aggregate = warning.exc_info[1]
            assert aggregate.failures == (cached,)
            evidence = aggregate.report["failures"][0]["native_evidence"]
            assert evidence["cleanup_status"] == status
            assert evidence["cleanup"]["recording_video"]["current_partial"] == partial
            assert evidence["task_context"]["task_id"] == "public-task"
        assert session in backend._CLIENTS
        assert attempts == ["legacy-failed", "legacy-success"] * 2
        assert "private-native-text" not in caplog.text and "private-legacy-message" not in caplog.text
        assert status in caplog.text and "video.partial.mp4" in caplog.text
        assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
        with pytest.raises(backend._LIFECYCLE.OwnedPixelsCleanupError) as repeated:
            backend.cleanup()
        assert repeated.value.failures == (cached,) and session in backend._CLIENTS
        assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    finally:
        bridge.shutdown_script_execution()


def test_setup_diagnostic_keeps_known_code_and_omits_private_payload():
    result = {"success": False, "error": "invalid_target", "message": "private-command-token", "details": "private"}
    assert setup_diagnostic(result) == {"success": False, "error": "invalid_target"}
    result["error"] = "private-command-token"
    assert setup_diagnostic(result) == {"success": False, "error": "unrecognized"}
    result["error"] = {"private-command-token": "private"}
    assert setup_diagnostic(result) == {"success": False, "error": "unrecognized"}
    assert "private" not in str(setup_diagnostic(result))


def test_legacy_only_cleanup_stays_best_effort_and_clears_entries():
    backend = load_backend()
    attempts = []

    def fail():
        attempts.append("failure")
        raise RuntimeError("legacy best effort")

    backend._CLIENTS.update(
        failed={"client": SimpleNamespace(stop=fail)},
        successful={"client": SimpleNamespace(stop=lambda: attempts.append("success"))},
    )
    backend.cleanup()
    assert attempts == ["failure", "success"] and backend._CLIENTS == {}


def test_owned_warning_is_bounded_and_keeps_original_large_partial_evidence():
    lifecycle = load_backend()._LIFECYCLE
    failure = OwnedCuaMcpError("cleanup_unknown", "private-command-token-must-not-be-logged", {})
    failure.native_evidence = {
        "cleanup_pending": True,
        "cleanup": {"recording_video": {"current_partial": "partial", "segment_paths": ["long-path" * 1000] * 100}},
    }
    aggregate = lifecycle.OwnedPixelsCleanupError([failure] * 20)
    assert len(str(aggregate)) < 32768
    assert aggregate.failures == (failure,) * 20
    assert aggregate.report["failure_count"] == 20 and aggregate.report["omitted_failure_count"] == 16
    for report in aggregate.report["failures"]:
        assert report["evidence_omitted"] is True and report["recording_partial_retained"] is True
        assert report["segment_count"] == 100 and report["cleanup_pending"] is True
    assert "private-command-token" not in str(aggregate)


def test_all_owned_failures_are_attempted_and_arbitrary_exceptions_stay_private():
    lifecycle = load_backend()._LIFECYCLE
    failures = [OwnedCuaMcpError("cleanup_failed", "private-native-message", {}), RuntimeError("private-token")]
    attempts = []
    entries = []
    for index, failure in enumerate(failures):
        owned = PixelsMcpHostClient.__new__(PixelsMcpHostClient)
        owned._recording = PixelsMcpRecording(owned)

        def fail(error=failure, attempt=index):
            attempts.append(attempt)
            raise error

        owned._cleanup = SimpleNamespace(stop=fail)
        entries.append({"client": owned})
    with pytest.raises(lifecycle.OwnedPixelsCleanupError) as caught:
        lifecycle.stop_clients(entries)
    assert attempts == [0, 1] and caught.value.failures == tuple(failures)
    assert [report["code"] for report in caught.value.report["failures"]] == ["cleanup_failed", "cleanup_unknown"]
    assert "private" not in str(caught.value)
