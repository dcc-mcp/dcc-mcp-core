"""Tests for import-light environment parsing helpers."""

import json
import os
from pathlib import Path
import subprocess
import sys

from dcc_mcp_core.env import env_flag
from dcc_mcp_core.env import env_float
from dcc_mcp_core.env import env_int
from dcc_mcp_core.env import env_path

REPO_ROOT = Path(__file__).resolve().parent.parent


def test_env_module_does_not_load_core_in_fresh_process() -> None:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import json, sys; import dcc_mcp_core.env; "
            'print(json.dumps({"core_loaded": "dcc_mcp_core._core" in sys.modules}))',
        ],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    assert json.loads(result.stdout) == {"core_loaded": False}


def test_env_flag_uses_explicit_truthy_contract(monkeypatch) -> None:
    monkeypatch.delenv("DCC_MCP_TEST_FLAG", raising=False)
    assert env_flag("DCC_MCP_TEST_FLAG") is False
    assert env_flag("DCC_MCP_TEST_FLAG", default=True) is True

    monkeypatch.setenv("DCC_MCP_TEST_FLAG", "TRUE")
    assert env_flag("DCC_MCP_TEST_FLAG") is True
    assert env_flag("DCC_MCP_TEST_FLAG", truthy=("1",)) is False


def test_env_number_helpers_fall_back_and_apply_minimum(monkeypatch) -> None:
    monkeypatch.setenv("DCC_MCP_TEST_INT", "invalid")
    monkeypatch.setenv("DCC_MCP_TEST_FLOAT", "0.01")
    assert env_int("DCC_MCP_TEST_INT", 7) == 7
    assert env_float("DCC_MCP_TEST_FLOAT", 3.0, minimum=0.1) == 0.1


def test_env_path_expands_user_and_supports_default(monkeypatch) -> None:
    monkeypatch.delenv("DCC_MCP_TEST_PATH", raising=False)
    assert env_path("DCC_MCP_TEST_PATH") is None
    assert env_path("DCC_MCP_TEST_PATH", "relative") == Path("relative")

    monkeypatch.setenv("DCC_MCP_TEST_PATH", "~/dcc-mcp")
    assert env_path("DCC_MCP_TEST_PATH") == Path("~/dcc-mcp").expanduser()
