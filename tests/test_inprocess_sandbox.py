"""Regression tests for sandbox-wrapped in-process skill execution (issue #1001)."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from unittest.mock import MagicMock
from unittest.mock import patch

from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.options import DccServerOptions
from dcc_mcp_core.server_base import DccServerBase


def _write_script(tmp_path: Path, body: str) -> Path:
    path = tmp_path / "execute_python.py"
    path.write_text(body, encoding="utf-8")
    return path


def test_denied_action_records_audit_without_importing_script(tmp_path: Path) -> None:
    policy = SandboxPolicy()
    policy.deny_actions(["execute_python"])
    ctx = SandboxContext(policy)
    bridge = HostExecutionBridge(sandbox_context=ctx)

    script = _write_script(
        tmp_path,
        "IMPORT_RAN = True\ndef main():\n    return {'success': True, 'message': 'should not run'}\n",
    )

    with patch(
        "dcc_mcp_core._server.inprocess_executor.run_skill_script",
        side_effect=AssertionError("skill script must not be imported"),
    ) as mocked:
        result = bridge.execute_script(
            str(script),
            {},
            action_name="execute_python",
        )

    mocked.assert_not_called()
    assert result["success"] is False
    assert result["error"]["type"] == "SandboxDenied"
    denials = ctx.audit_log.denials()
    assert len(denials) == 1
    assert denials[0].action == "execute_python"
    assert denials[0].outcome == "denied"


def test_allowed_action_records_success_audit(tmp_path: Path) -> None:
    policy = SandboxPolicy()
    policy.allow_actions(["echo_action"])
    ctx = SandboxContext(policy)
    bridge = HostExecutionBridge(sandbox_context=ctx)

    script = _write_script(
        tmp_path,
        "def main(value=0):\n    return {'success': True, 'value': value}\n",
    )
    result = bridge.execute_script(
        str(script),
        {"value": 3},
        action_name="echo_action",
    )

    assert result == {"success": True, "value": 3}
    successes = ctx.audit_log.successes()
    assert len(successes) == 1
    assert successes[0].action == "echo_action"
    assert successes[0].outcome == "success"


def test_sandbox_uses_script_stem_when_action_name_missing(tmp_path: Path) -> None:
    policy = SandboxPolicy()
    policy.deny_actions(["execute_python"])
    ctx = SandboxContext(policy)
    bridge = HostExecutionBridge(sandbox_context=ctx)

    script = _write_script(
        tmp_path,
        "def main():\n    return {'success': True, 'message': 'should not run'}\n",
    )

    with patch(
        "dcc_mcp_core._server.inprocess_executor.run_skill_script",
        side_effect=AssertionError("skill script must not be imported"),
    ) as mocked:
        result = bridge.execute_script(str(script), {})

    mocked.assert_not_called()
    assert result["success"] is False
    assert result["error"]["type"] == "SandboxDenied"
    assert result["error"]["action"] == "execute_python"
    denials = ctx.audit_log.denials()
    assert len(denials) == 1
    assert denials[0].action == "execute_python"


def _make_base_with_captured_executor(tmp_path: Path) -> tuple[DccServerBase, list[Any]]:
    opts = DccServerOptions.from_env(
        "test_inproc_sandbox",
        tmp_path,
        port=0,
        enable_file_logging=False,
        enable_job_persistence=False,
        enable_telemetry=False,
    )
    with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=MagicMock()):
        base = DccServerBase(opts)

    captured: list[Any] = []

    class _Sink:
        def __init__(self, real: Any) -> None:
            self._real = real

        def set_in_process_executor(self, executor: Any) -> None:
            captured.append(executor)

        def __getattr__(self, item: str) -> Any:
            return getattr(self._real, item)

    base._server = _Sink(base._server)
    return base, captured


def test_register_inprocess_executor_attaches_configured_sandbox(tmp_path: Path) -> None:
    base, captured = _make_base_with_captured_executor(tmp_path)

    policy = SandboxPolicy()
    policy.deny_actions(["execute_python"])
    base._config.sandbox_policy = policy

    base.register_inprocess_executor()
    assert len(captured) == 1

    script = _write_script(
        tmp_path,
        "def main():\n    return {'success': True, 'message': 'should not run'}\n",
    )
    result = captured[0](str(script), {})
    assert result["success"] is False
    assert result["error"]["type"] == "SandboxDenied"
    assert result["error"]["action"] == "execute_python"
    assert base._execution_bridge is not None
    assert base._execution_bridge.sandbox_context is not None


def test_register_host_execution_bridge_attaches_configured_sandbox(tmp_path: Path) -> None:
    base, captured = _make_base_with_captured_executor(tmp_path)

    trusted_root = tmp_path / "trusted"
    trusted_root.mkdir()
    materialization_root = tmp_path / "materialized"
    policy = SandboxPolicy()
    policy.deny_actions(["execute_python"])
    policy.allow_paths([str(trusted_root)])
    base._config.sandbox_policy = policy

    bridge = HostExecutionBridge(script_materialization_root=materialization_root)
    base.register_host_execution_bridge(bridge)
    assert len(captured) == 1
    assert bridge.sandbox_context is not None
    assert bridge.script_materialization_root == materialization_root.resolve()
    assert bridge.sandbox_context.is_path_allowed(str(materialization_root / "custom" / "script.py")) is True
    assert bridge.sandbox_context.is_path_allowed(str(tmp_path / "outside.py")) is False

    script = _write_script(
        tmp_path,
        "def main():\n    return {'success': True, 'message': 'should not run'}\n",
    )
    result = captured[0](str(script), {})
    assert result["success"] is False
    assert result["error"]["type"] == "SandboxDenied"
    assert result["error"]["action"] == "execute_python"
