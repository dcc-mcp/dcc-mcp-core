"""Tests for standard script execution capture/envelopes (#603)."""

from __future__ import annotations

import sys

import dcc_mcp_core
from dcc_mcp_core.script_execution import ScriptExecutionCapture
from dcc_mcp_core.script_execution import ScriptExecutionResult


def test_script_execution_helpers_are_exported() -> None:
    assert dcc_mcp_core.ScriptExecutionCapture is ScriptExecutionCapture
    assert dcc_mcp_core.ScriptExecutionResult is ScriptExecutionResult
    assert "ScriptExecutionCapture" in dcc_mcp_core.__all__
    assert "ScriptExecutionResult" in dcc_mcp_core.__all__


def test_capture_collects_stdout_and_stderr() -> None:
    with ScriptExecutionCapture() as cap:
        print("hello stdout")
        print("hello stderr", file=sys.stderr)

    assert cap.stdout == "hello stdout\n"
    assert cap.stderr == "hello stderr\n"


def test_capture_tee_forwards_to_original_streams(capsys) -> None:
    with ScriptExecutionCapture(tee=True) as cap:
        print("visible stdout")
        print("visible stderr", file=sys.stderr)

    captured = capsys.readouterr()
    assert cap.stdout == "visible stdout\n"
    assert cap.stderr == "visible stderr\n"
    assert captured.out == "visible stdout\n"
    assert captured.err == "visible stderr\n"


def test_result_from_value_returns_standard_success_envelope() -> None:
    result = ScriptExecutionResult.from_value(
        {"created": "pCube1"},
        stdout="out",
        stderr="err",
    )

    assert result == {
        "success": True,
        "message": "Script executed successfully",
        "context": {
            "result": {"created": "pCube1"},
            "stdout": "out",
            "stderr": "err",
        },
    }


def test_strict_json_reports_non_serializable_values() -> None:
    result = ScriptExecutionResult.from_value({"bad": object()}, strict_json=True)

    assert result["success"] is False
    assert result["error"] == "non_serializable_result"
    assert "not JSON serializable" in result["message"]
    assert result["context"]["stdout"] == ""
    assert result["context"]["stderr"] == ""


def test_non_strict_json_uses_repr_fallback() -> None:
    class HostObject:
        def __repr__(self) -> str:
            return "<HostObject pCube1>"

    result = ScriptExecutionResult.from_value(
        {"node": HostObject(), "items": {1, 2}},
        strict_json=False,
    )

    assert result["success"] is True
    assert result["context"]["result"]["node"] == "<HostObject pCube1>"
    assert sorted(result["context"]["result"]["items"]) == [1, 2]


def test_from_exception_includes_traceback_and_captured_output() -> None:
    try:
        raise RuntimeError("boom")
    except RuntimeError as exc:
        result = ScriptExecutionResult.from_exception(exc, stdout="out", stderr="err")

    assert result["success"] is False
    assert result["error"] == "script_execution_error"
    assert result["context"]["stdout"] == "out"
    assert result["context"]["stderr"] == "err"
    assert result["context"]["exception_type"] == "RuntimeError"
    assert result["context"]["exception_message"] == "boom"
    assert "Traceback" in result["context"]["traceback"]
