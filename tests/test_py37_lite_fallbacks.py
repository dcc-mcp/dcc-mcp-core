from __future__ import annotations

import importlib
import os
from pathlib import Path
import sys


def _import_without_core(monkeypatch, *module_names: str):
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", None)
    import dcc_mcp_core

    dcc_mcp_core.__dict__.pop("_core", None)
    for name in module_names:
        sys.modules.pop(name, None)
    return {name: importlib.import_module(name) for name in module_names}


def test_host_namespace_falls_back_without_core(monkeypatch) -> None:
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core.host._protocols",
        "dcc_mcp_core.host._wire",
        "dcc_mcp_core.host._adapter",
        "dcc_mcp_core.host._standalone",
        "dcc_mcp_core.host",
    )
    host = modules["dcc_mcp_core.host"]

    dispatcher = host.QueueDispatcher()
    assert dispatcher.tick().jobs_executed == 0

    with host.StandaloneHost(dispatcher):
        assert dispatcher.post(lambda: 42).wait(timeout=2.0) == 42

    blocking = host.BlockingDispatcher()
    with host.StandaloneHost(blocking):
        assert blocking.post(lambda: "ok").wait(timeout=2.0) == "ok"

    assert host.normalize_tool_arguments('{"radius": 2}') == {"radius": 2}
    assert host.normalize_tool_meta(None) is None


def test_server_and_skill_helpers_fallback_without_core(monkeypatch, tmp_path) -> None:
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._server.config",
        "dcc_mcp_core._server.skill_discovery",
        "dcc_mcp_core.server_base",
    )
    config = modules["dcc_mcp_core._server.config"]
    skill_discovery = modules["dcc_mcp_core._server.skill_discovery"]
    server_base = modules["dcc_mcp_core.server_base"]

    cfg = config.McpHttpConfig(port=8765, server_name="fallback", enable_cors=True, request_timeout_ms=1234)
    assert cfg.host == "127.0.0.1"
    assert cfg.endpoint_path == "/mcp"
    assert cfg.enable_cors is True
    assert cfg.request_timeout_ms == 1234
    cfg.backend_timeout_ms = 4567
    assert cfg.backend_timeout_ms == 4567
    cfg.job_recovery = "Requeue"
    assert cfg.job_recovery == "requeue"

    monkeypatch.setenv("DCC_MCP_SKILL_PATHS", os.pathsep.join([str(tmp_path / "a"), str(tmp_path / "b")]))
    assert skill_discovery.get_skill_paths_from_env() == [str(tmp_path / "a"), str(tmp_path / "b")]
    assert skill_discovery.get_local_skills_dir("maya").endswith(str(Path(".dcc-mcp") / "maya" / "skills"))
    assert skill_discovery.get_skills_dir("maya").endswith(str(Path(".dcc-mcp") / "skills" / "maya"))

    assert server_base._PKG_VERSION == "0.0.0-dev"
