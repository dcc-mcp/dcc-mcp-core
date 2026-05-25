"""Tests for standard script execution capture/envelopes (#603)."""

from __future__ import annotations

from pathlib import Path
import sys

import pytest

import dcc_mcp_core
from dcc_mcp_core.script_execution import FileBackedScriptExecutionParams
from dcc_mcp_core.script_execution import ScriptExecutionCapture
from dcc_mcp_core.script_execution import ScriptExecutionParams
from dcc_mcp_core.script_execution import ScriptExecutionResult
from dcc_mcp_core.script_execution import allow_script_materialization_root
from dcc_mcp_core.script_execution import execute_with_context
from dcc_mcp_core.script_execution import normalize_file_backed_script_execution_params
from dcc_mcp_core.script_execution import normalize_script_execution_params
from dcc_mcp_core.script_execution import validate_script_file_path


def test_script_execution_helpers_are_exported() -> None:
    assert dcc_mcp_core.ScriptExecutionCapture is ScriptExecutionCapture
    assert dcc_mcp_core.FileBackedScriptExecutionParams is FileBackedScriptExecutionParams
    assert dcc_mcp_core.ScriptExecutionParams is ScriptExecutionParams
    assert dcc_mcp_core.ScriptExecutionResult is ScriptExecutionResult
    assert dcc_mcp_core.allow_script_materialization_root is allow_script_materialization_root
    assert dcc_mcp_core.normalize_file_backed_script_execution_params is normalize_file_backed_script_execution_params
    assert dcc_mcp_core.normalize_script_execution_params is normalize_script_execution_params
    assert dcc_mcp_core.validate_script_file_path is validate_script_file_path
    assert "ScriptExecutionCapture" in dcc_mcp_core.__all__
    assert "FileBackedScriptExecutionParams" in dcc_mcp_core.__all__
    assert "ScriptExecutionParams" in dcc_mcp_core.__all__
    assert "ScriptExecutionResult" in dcc_mcp_core.__all__
    assert "allow_script_materialization_root" in dcc_mcp_core.__all__
    assert "normalize_file_backed_script_execution_params" in dcc_mcp_core.__all__
    assert "normalize_script_execution_params" in dcc_mcp_core.__all__
    assert "validate_script_file_path" in dcc_mcp_core.__all__


def test_normalize_script_execution_params_accepts_code() -> None:
    assert normalize_script_execution_params({"code": "print(1)"}).code == "print(1)"


def test_normalize_script_execution_params_timeout_secs() -> None:
    p = normalize_script_execution_params({"code": "pass", "timeout_secs": 7})
    assert p.timeout_secs == 7

    default = normalize_script_execution_params({"code": "pass"}, default_timeout_secs=30)
    assert default.timeout_secs == 30


def test_normalize_script_execution_params_validates_input() -> None:
    with pytest.raises(ValueError, match="Missing required 'code'"):
        normalize_script_execution_params({})
    with pytest.raises(TypeError, match="code must be a string"):
        normalize_script_execution_params({"code": 123})
    with pytest.raises(ValueError, match="greater than zero"):
        normalize_script_execution_params({"code": "pass", "timeout_secs": 0})


def test_file_backed_normalizer_auto_materializes_inline_code(tmp_path: Path) -> None:
    params = normalize_file_backed_script_execution_params(
        {"code": "result = 21 * 2", "timeout_secs": 5},
        dcc_type="maya",
        instance_id="inst-1",
        session_id="sess-1",
        materialization_root=tmp_path,
        tool_call_id="call-1",
        correlation_id="corr-1",
    )

    assert params.is_file_backed is True
    assert params.file_path is not None
    assert Path(params.file_path).is_file()
    assert params.timeout_secs == 5
    assert params.source == "materialized"
    assert params.materialized_script is not None
    assert params.materialized_context()["sha256"] == params.materialized_script.sha256
    assert execute_with_context(params.code, filename=params.file_path) == 42


def test_file_backed_normalizer_require_rejects_inline_code(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="policy=require"):
        normalize_file_backed_script_execution_params(
            {"code": "pass"},
            dcc_type="maya",
            instance_id="inst-1",
            session_id="sess-1",
            materialization_root=tmp_path,
            policy="require",
        )


def test_file_backed_normalizer_accepts_trusted_file_path(tmp_path: Path) -> None:
    script = tmp_path / "tool.py"
    script.write_text("result = 'from-file'", encoding="utf-8")

    params = normalize_file_backed_script_execution_params(
        {"file_path": str(script)},
        dcc_type="photoshop",
        instance_id="ps-1",
        session_id="sess-1",
        materialization_root=tmp_path / "materialized",
        trusted_roots=(tmp_path,),
        policy="require",
    )

    assert params.file_path == str(script.resolve())
    assert params.code == "result = 'from-file'"
    metadata = params.materialized_context()
    assert metadata["path"] == str(script.resolve())
    assert metadata["file_ref"]["digest"] == f"sha256:{params.sha256}"
    assert execute_with_context(params.code, filename=params.file_path) == "from-file"


def test_file_backed_normalizer_rejects_untrusted_file_path(tmp_path: Path) -> None:
    script = tmp_path / "outside.py"
    script.write_text("result = 'nope'", encoding="utf-8")

    with pytest.raises(ValueError, match="outside trusted roots"):
        normalize_file_backed_script_execution_params(
            {"file_path": str(script)},
            dcc_type="custom",
            instance_id="inst",
            session_id="sess",
            materialization_root=tmp_path / "materialized",
        )


def test_allow_script_materialization_root_extends_sandbox_allowlist(tmp_path: Path) -> None:
    policy = dcc_mcp_core.SandboxPolicy()
    trusted = tmp_path / "trusted"
    trusted.mkdir()
    root = tmp_path / "materialized"
    policy.allow_paths([str(trusted)])

    allowed_root = allow_script_materialization_root(policy, root=root)
    ctx = dcc_mcp_core.SandboxContext(policy)

    assert allowed_root == root.resolve()
    assert ctx.is_path_allowed(str(root / "custom" / "script.py")) is True
    assert ctx.is_path_allowed(str(tmp_path / "outside.py")) is False


def test_allow_script_materialization_root_preserves_unrestricted_sandbox(tmp_path: Path) -> None:
    policy = dcc_mcp_core.SandboxPolicy()
    root = tmp_path / "materialized"

    allowed_root = allow_script_materialization_root(policy, root=root)
    ctx = dcc_mcp_core.SandboxContext(policy)

    assert allowed_root == root.resolve()
    assert ctx.is_path_allowed(str(root / "custom" / "script.py")) is True
    assert ctx.is_path_allowed(str(tmp_path / "outside.py")) is True


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


def test_result_from_value_attaches_materialized_script_context(tmp_path: Path) -> None:
    params = normalize_file_backed_script_execution_params(
        {"code": "result = 7"},
        dcc_type="blender",
        instance_id="blend-1",
        session_id="sess-1",
        materialization_root=tmp_path,
    )

    result = ScriptExecutionResult.from_value(
        7,
        materialized_script=params,
    )

    metadata = result["context"]["materialized_script"]
    assert metadata["path"] == params.file_path
    assert metadata["file_ref"]["digest"] == f"sha256:{params.sha256}"
    assert metadata["bytes"] == params.bytes
    assert metadata["reused"] is False


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


# ── issue #856 regression: output_capture pause/resume ────────────────────────


class _FakeOutputCapture:
    """Minimal test double for OutputCapture (the Rust PyO3 object)."""

    def __init__(self) -> None:
        self.paused = False
        self.pause_calls: list[bool] = []

    def set_paused(self, value: bool) -> None:
        self.paused = value
        self.pause_calls.append(value)


def test_capture_pauses_output_capture_on_enter_resumes_on_exit() -> None:
    """ScriptExecutionCapture must pause output_capture on enter and resume on exit."""
    oc = _FakeOutputCapture()
    with ScriptExecutionCapture(output_capture=oc):
        assert oc.paused is True, "must be paused during script body"

    assert oc.paused is False, "must be resumed after exit"
    assert oc.pause_calls == [True, False]


def test_capture_resumes_output_capture_on_exception() -> None:
    """output_capture must be resumed even when the script body raises."""
    oc = _FakeOutputCapture()
    try:
        with ScriptExecutionCapture(output_capture=oc):
            raise RuntimeError("script error")
    except RuntimeError:
        pass

    assert oc.paused is False, "must resume on exception exit"
    assert oc.pause_calls == [True, False]


def test_capture_without_output_capture_works_normally() -> None:
    """Passing no output_capture must leave existing behaviour unchanged."""
    with ScriptExecutionCapture() as cap:
        print("hello")

    assert cap.stdout == "hello\n"


def test_capture_tolerates_set_paused_raising() -> None:
    """A broken output_capture must not abort the script body."""

    class _BrokenCapture:
        def set_paused(self, _value: bool) -> None:
            raise RuntimeError("broken")

    with ScriptExecutionCapture(output_capture=_BrokenCapture()) as cap:
        print("still works")

    assert cap.stdout == "still works\n"
