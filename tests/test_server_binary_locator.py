from __future__ import annotations

import importlib.util
from pathlib import Path
import sys

_LOCATOR_SOURCE = (
    Path(__file__).resolve().parents[1] / "pkg" / "dcc-mcp-server-bin" / "python" / "dcc_mcp_server" / "__init__.py"
)


def _load_locator_module():
    spec = importlib.util.spec_from_file_location("_dcc_mcp_server_locator_under_test", _LOCATOR_SOURCE)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_binary_path_prefers_current_interpreter_environment_over_path(monkeypatch, tmp_path):
    locator = _load_locator_module()
    binary_name = locator._BINARY_NAME
    scripts_name = "Scripts" if locator.os.name == "nt" else "bin"
    interpreter_prefix = tmp_path / "current-environment"
    interpreter_binary = interpreter_prefix / scripts_name / binary_name
    interpreter_binary.parent.mkdir(parents=True)
    interpreter_binary.write_bytes(b"current")
    stale_path_binary = tmp_path / "stale-path" / binary_name
    stale_path_binary.parent.mkdir(parents=True)
    stale_path_binary.write_bytes(b"stale")

    monkeypatch.delenv("DCC_MCP_SERVER_BIN", raising=False)
    monkeypatch.setattr(locator.sys, "prefix", str(interpreter_prefix))
    monkeypatch.setattr(locator.shutil, "which", lambda _name: str(stale_path_binary))

    assert locator.binary_path() == interpreter_binary


def test_binary_path_keeps_explicit_operator_override_first(monkeypatch, tmp_path):
    locator = _load_locator_module()
    override = tmp_path / locator._BINARY_NAME
    override.write_bytes(b"operator")
    monkeypatch.setenv("DCC_MCP_SERVER_BIN", str(override))
    monkeypatch.setattr(locator.shutil, "which", lambda _name: sys.executable)

    assert locator.binary_path() == override
