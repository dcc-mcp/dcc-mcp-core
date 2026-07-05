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
        "dcc_mcp_core.host._fallback",
        "dcc_mcp_core.host",
    )
    host = modules["dcc_mcp_core.host"]
    fallback = modules["dcc_mcp_core.host._fallback"]

    assert host.BlockingDispatcher is None
    assert host.QueueDispatcher is None
    assert host.PostHandle is None
    assert host.TickOutcome is None
    assert host.DispatchError is None

    dispatcher = fallback.QueueDispatcher()
    assert dispatcher.tick().jobs_executed == 0

    with host.StandaloneHost(dispatcher):
        assert dispatcher.post(lambda: 42).wait(timeout=2.0) == 42

    blocking = fallback.BlockingDispatcher()
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


def test_parse_skill_md_fallback(monkeypatch, tmp_path) -> None:
    """parse_skill_md works without _core (py37-lite)."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]

    # Create a minimal SKILL.md
    skill_dir = tmp_path / "test-skill"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text(
        "---\n"
        "name: test-skill\n"
        "description: A test skill\n"
        "dcc: maya\n"
        "version: '1.0.0'\n"
        "tags: [modeling, test]\n"
        "---\n"
        "# Test Skill\n"
        "This is a test skill body.\n",
        encoding="utf-8",
    )

    meta = fallback.parse_skill_md(str(skill_dir))
    assert meta is not None
    assert meta.name == "test-skill"
    assert meta.description == "A test skill"
    assert meta.dcc == "maya"
    assert meta.version == "1.0.0"
    assert "modeling" in meta.tags

    # Non-existent path raises FileNotFoundError
    try:
        fallback.parse_skill_md(str(tmp_path / "nonexistent"))
    except FileNotFoundError:
        pass
    else:
        raise AssertionError("Expected FileNotFoundError")

    # Directory without SKILL.md returns None
    empty_dir = tmp_path / "empty"
    empty_dir.mkdir()
    assert fallback.parse_skill_md(str(empty_dir)) is None

    # Direct file path works
    result = fallback.parse_skill_md(str(skill_dir / "SKILL.md"))
    assert result is not None
    assert result.name == "test-skill"


def test_dcc_capabilities_fallback(monkeypatch) -> None:
    """DccCapabilities works without _core (py37-lite)."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]

    caps = fallback.DccCapabilities(
        scene_info=True,
        snapshot=True,
        file_operations=True,
    )
    assert caps.scene_info is True
    assert caps.snapshot is True
    assert caps.file_operations is True
    assert caps.selection is False
    assert caps.has_embedded_python is True
    assert caps.uses_bridge() is False

    # Static factories
    http_caps = fallback.DccCapabilities.http_bridge("http://localhost:8765")
    assert http_caps.bridge_kind == "http"
    assert http_caps.bridge_endpoint == "http://localhost:8765"
    assert http_caps.uses_bridge() is True

    ws_caps = fallback.DccCapabilities.websocket_bridge("ws://localhost:9001")
    assert ws_caps.bridge_kind == "websocket"
    assert ws_caps.uses_bridge() is True

    # Default constructor
    default = fallback.DccCapabilities()
    assert default.scene_info is False
    assert default.has_embedded_python is True
    assert default.extensions == {}
    assert default.bridge_kind is None


def test_py_pumped_dispatcher_fallback(monkeypatch) -> None:
    """PyPumpedDispatcher works without _core (py37-lite)."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]

    disp = fallback.PyPumpedDispatcher(budget_ms=100)
    assert disp.budget_ms == 100
    assert disp.total_dispatched == 0
    assert disp.total_processed == 0
    assert disp.pending() == 0
    assert "any" in disp.supported()
    assert "main" in disp.supported()
    assert disp.capabilities()["pumped"] is True

    # Submit "any" affinity — executed immediately
    result = disp.submit("test-action", payload="hello", affinity="any")
    assert result["success"] is True
    assert result["action_name"] == "test-action"
    assert result["output"] == "hello"
    assert disp.total_dispatched == 1
    assert disp.total_processed == 1

    # Submit "main" affinity — queued for pump
    result = disp.submit("main-action", payload="world", affinity="main")
    assert result["success"] is True
    assert result["output"] == "world"
    assert disp.total_dispatched == 2
    assert disp.total_processed == 2

    # pump() on empty queue
    stats = disp.pump()
    assert stats["processed"] == 0
    assert stats["remaining"] == 0

    # pump_with_budget
    stats2 = disp.pump_with_budget(50)
    assert stats2["processed"] == 0

    # repr
    r = repr(disp)
    assert "PyPumpedDispatcher" in r
    assert "budget_ms=100" in r


def test_fallback_imports_from_top_level(monkeypatch) -> None:
    """Top-level imports resolve via _py37_fallback when _core is absent."""
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", None)
    import dcc_mcp_core
    dcc_mcp_core.__dict__.pop("_core", None)

    # Clear the fallback module cache so it re-imports without _core
    sys.modules.pop("dcc_mcp_core._py37_fallback", None)

    # parse_skill_md should come from _py37_fallback
    parse = dcc_mcp_core.parse_skill_md
    assert callable(parse)

    # DccCapabilities should come from _py37_fallback
    caps = dcc_mcp_core.DccCapabilities(scene_info=True)
    assert caps.scene_info is True

    # PyPumpedDispatcher should come from _py37_fallback
    disp = dcc_mcp_core.PyPumpedDispatcher()
    assert disp.budget_ms == 8
