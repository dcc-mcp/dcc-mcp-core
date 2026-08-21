"""Tests for the shared Python exception hierarchy."""

import json
import os
from pathlib import Path
import subprocess
import sys

from dcc_mcp_core import DccMcpError
from dcc_mcp_core.asset_import import AssetImportValidationError
from dcc_mcp_core.asset_sync import AssetSyncConflictError
from dcc_mcp_core.asset_sync import AssetSyncValidationError
from dcc_mcp_core.auth import TokenValidationError
from dcc_mcp_core.bridge import BridgeConnectionError
from dcc_mcp_core.bridge import BridgeError
from dcc_mcp_core.bridge import BridgeRpcError
from dcc_mcp_core.bridge import BridgeTimeoutError
from dcc_mcp_core.cancellation import DccMcpCancelledError
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.guardrails import DccGuardrailError
from dcc_mcp_core.host._fallback import DispatchError
from dcc_mcp_core.lifecycle_hooks import HookDeny
from dcc_mcp_core.script_execution import ScriptExecutionSerializationError
from dcc_mcp_core.skills_helper import HttpStatusError
from dcc_mcp_core.skills_helper import SkillCodecError
from dcc_mcp_core.skills_helper import SkillFileError
from dcc_mcp_core.skills_helper import SkillHelperError
from dcc_mcp_core.skills_helper import SkillHttpError
from dcc_mcp_core.vector_embedder import EmbedderError

REPO_ROOT = Path(__file__).resolve().parent.parent


def test_error_module_does_not_load_core_in_fresh_process() -> None:
    env = os.environ.copy()
    env["PYTHONPATH"] = str(REPO_ROOT / "python")
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import json, sys; from dcc_mcp_core import DccMcpError; "
            'print(json.dumps({"core_loaded": "dcc_mcp_core._core" in sys.modules, '
            '"name": DccMcpError.__name__}))',
        ],
        cwd=str(REPO_ROOT),
        env=env,
        check=True,
        capture_output=True,
        text=True,
    )
    assert json.loads(result.stdout) == {"core_loaded": False, "name": "DccMcpError"}


def test_public_errors_share_one_root() -> None:
    error_types = (
        AssetImportValidationError,
        AssetSyncConflictError,
        AssetSyncValidationError,
        BridgeConnectionError,
        BridgeError,
        BridgeRpcError,
        BridgeTimeoutError,
        DccMcpCancelledError,
        CuaCliError,
        DccGuardrailError,
        DispatchError,
        EmbedderError,
        HookDeny,
        HttpStatusError,
        ScriptExecutionSerializationError,
        SkillCodecError,
        SkillFileError,
        SkillHelperError,
        SkillHttpError,
        TokenValidationError,
    )
    assert all(issubclass(error_type, DccMcpError) for error_type in error_types)


def test_builtin_exception_contracts_are_preserved() -> None:
    assert issubclass(AssetImportValidationError, ValueError)
    assert issubclass(AssetSyncValidationError, ValueError)
    assert issubclass(AssetSyncConflictError, RuntimeError)
    assert issubclass(CuaCliError, RuntimeError)
    assert issubclass(DccGuardrailError, RuntimeError)
    assert issubclass(DispatchError, RuntimeError)
    assert issubclass(EmbedderError, RuntimeError)
    assert issubclass(ScriptExecutionSerializationError, TypeError)
