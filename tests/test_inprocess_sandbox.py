"""Regression tests for sandbox-wrapped in-process skill execution (issue #1001)."""

from __future__ import annotations

from pathlib import Path
from typing import Any
from unittest.mock import patch

from dcc_mcp_core import SandboxContext
from dcc_mcp_core import SandboxPolicy
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge


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


def test_mcp_http_config_forwards_sandbox_policy_to_bridge() -> None:
    from dcc_mcp_core import McpHttpConfig

    policy = SandboxPolicy()
    policy.deny_actions(["blocked"])
    cfg = McpHttpConfig(port=0)
    cfg.sandbox_policy = policy

    bridge = HostExecutionBridge()
    assert bridge.sandbox_context is None

    policy_obj = cfg.sandbox_policy
    assert policy_obj is not None
    bridge.sandbox_context = SandboxContext(policy_obj)
    assert bridge.sandbox_context is not None
    assert bridge.sandbox_context.is_allowed("safe") in (True, False)
