from __future__ import annotations

import importlib
import os
from pathlib import Path
import sys

import pytest

_FALLBACK_PUBLIC_NAMES = (
    "DccCapabilities",
    "GuiExecutableHint",
    "PyPumpedDispatcher",
    "ReadinessProbe",
    "correct_python_executable",
    "is_gui_executable",
    "parse_skill_md",
    "scan_and_load_strict",
)


@pytest.fixture(autouse=True)
def _reset_py37_fallback_module_after_stub_core():
    """Drop cached ``_py37_fallback`` so xdist workers do not keep py37 bindings."""
    yield
    sys.modules.pop("dcc_mcp_core._py37_fallback", None)
    package = sys.modules.get("dcc_mcp_core")
    if package is not None:
        for name in _FALLBACK_PUBLIC_NAMES:
            package.__dict__.pop(name, None)


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

    assert host.BlockingDispatcher is fallback.BlockingDispatcher
    assert host.QueueDispatcher is fallback.QueueDispatcher
    assert host.PostHandle is fallback.PostHandle
    assert host.TickOutcome is fallback.TickOutcome
    assert host.DispatchError is fallback.DispatchError

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


@pytest.mark.parametrize("allowed_tools_key", ["allowed-tools", "allowed_tools"])
def test_parse_skill_md_fallback(monkeypatch, tmp_path, allowed_tools_key: str) -> None:
    """The public parser and lite catalog share canonical metadata semantics."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
        "dcc_mcp_core._runtime.pure_skill_catalog",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]
    catalog_module = modules["dcc_mcp_core._runtime.pure_skill_catalog"]

    skill_dir = tmp_path / "test-skill"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text(
        "---\n"
        "name: test-skill\n"
        "description: >-\n"
        "  A test skill with #quoted content\n"
        "  for shared discovery.\n"
        f"{allowed_tools_key}: [Read]\n"
        "metadata:\n"
        "  dcc-mcp:\n"
        "    dcc: maya\n"
        '    version: "1.0.0"  # release version\n'
        "    tags: [modeling, test]\n"
        "    depends: [base-skill]\n"
        "    allow-implicit-invocation: false\n"
        "---\n"
        "# Test Skill\n"
        "This is a test skill body.\n",
        encoding="utf-8",
    )

    meta = fallback.parse_skill_md(str(skill_dir))
    assert meta is not None
    assert meta.name == "test-skill"
    assert meta.description == "A test skill with #quoted content for shared discovery."
    assert meta.dcc == "maya"
    assert meta.version == "1.0.0"
    assert meta.tags == ["modeling", "test"]
    assert meta.depends == ["base-skill"]
    assert meta.implicit_invocation is False

    catalog = catalog_module.PurePythonSkillCatalog("maya")
    assert catalog.discover([(str(skill_dir), "repo")]) == 1
    summary = catalog.get_skill("test-skill")
    for field in ("name", "description", "dcc", "version", "tags", "depends", "implicit_invocation"):
        assert getattr(summary, field) == getattr(meta, field)

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


def test_shared_lite_parser_rejects_legacy_top_level_extensions(monkeypatch, tmp_path) -> None:
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
        "dcc_mcp_core._runtime.pure_skill_catalog",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]
    catalog_module = modules["dcc_mcp_core._runtime.pure_skill_catalog"]
    skill_dir = tmp_path / "legacy-skill"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text(
        "---\nname: legacy-skill\ndescription: Legacy metadata\ndcc: maya\n---\n",
        encoding="utf-8",
    )

    assert fallback.parse_skill_md(str(skill_dir)) is None
    catalog = catalog_module.PurePythonSkillCatalog("maya")
    assert catalog.discover([(str(skill_dir), "repo")]) == 0

    missing_description = tmp_path / "missing-description"
    missing_description.mkdir()
    (missing_description / "SKILL.md").write_text(
        "---\nname: missing-description\ndescription: # missing\n---\n",
        encoding="utf-8",
    )
    assert fallback.parse_skill_md(str(missing_description)) is None
    assert catalog.discover([(str(missing_description), "repo")]) == 0

    indented_opening = tmp_path / "indented-opening"
    indented_opening.mkdir()
    (indented_opening / "SKILL.md").write_text(
        "  ---\nname: indented-opening\ndescription: Invalid delimiter\n---\n",
        encoding="utf-8",
    )
    assert fallback.parse_skill_md(str(indented_opening)) is None
    assert catalog.discover([(str(indented_opening), "repo")]) == 0


def test_shared_lite_parser_keeps_indented_markdown_separator(monkeypatch, tmp_path) -> None:
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
        "dcc_mcp_core._runtime.pure_skill_catalog",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]
    catalog_module = modules["dcc_mcp_core._runtime.pure_skill_catalog"]
    skill_dir = tmp_path / "markdown-separator"
    skill_dir.mkdir()
    (skill_dir / "SKILL.md").write_text(
        "---\n"
        "name: markdown-separator\n"
        "description: |-\n"
        "  First paragraph\n"
        "  ---\n"
        "  Final paragraph\n"
        "metadata:\n"
        "  dcc-mcp:\n"
        "    dcc: maya\n"
        "    tags: [rule]\n"
        "---\n",
        encoding="utf-8",
    )

    parsed = fallback.parse_skill_md(str(skill_dir))
    catalog = catalog_module.PurePythonSkillCatalog("maya")
    assert catalog.discover([(str(skill_dir), "repo")]) == 1
    discovered = catalog.get_skill("markdown-separator")
    assert parsed.description == "First paragraph\n---\nFinal paragraph"
    assert discovered.description == parsed.description
    assert discovered.dcc == parsed.dcc == "maya"
    assert discovered.tags == parsed.tags == ["rule"]


def test_shared_lite_parser_matches_real_repo_skill(monkeypatch) -> None:
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
        "dcc_mcp_core._runtime.pure_skill_catalog",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]
    catalog_module = modules["dcc_mcp_core._runtime.pure_skill_catalog"]
    skill_dir = Path(__file__).resolve().parents[1] / "skills" / "dcc-mcp"

    parsed = fallback.parse_skill_md(str(skill_dir))
    catalog = catalog_module.PurePythonSkillCatalog("python")
    assert catalog.discover([(str(skill_dir), "repo")]) == 1
    discovered = catalog.get_skill("dcc-mcp")
    assert parsed is not None
    for field in ("name", "description", "dcc", "version", "tags", "depends"):
        assert getattr(discovered, field) == getattr(parsed, field)


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

    # scan_and_load_strict should come from _py37_fallback
    strict = dcc_mcp_core.scan_and_load_strict
    assert callable(strict)

    # GUI executable helpers should come from _py37_fallback
    assert callable(dcc_mcp_core.is_gui_executable)
    assert callable(dcc_mcp_core.correct_python_executable)

    # ReadinessProbe should come from _py37_fallback
    probe = dcc_mcp_core.ReadinessProbe()
    assert probe.report()["process"] is True
    assert probe.is_ready() is False


def test_readiness_probe_fallback_without_core(monkeypatch) -> None:
    """ReadinessProbe and AdapterReadinessBinder work without _core (py37-lite)."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
        "dcc_mcp_core.readiness",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]
    readiness = modules["dcc_mcp_core.readiness"]

    probe = fallback.ReadinessProbe()
    report = probe.report()
    assert report["process"] is True
    assert report["dcc"] is False
    assert report["skill_catalog"] is True
    assert report["dispatcher"] is False
    assert report["host_execution_bridge"] is False
    assert report["main_thread_executor"] is False
    assert probe.is_ready() is False

    probe.set_dispatcher_ready(True)
    probe.set_dcc_ready(True)
    assert probe.is_ready() is True

    probe.set_host_execution_bridge_ready(True)
    probe.set_main_thread_executor_ready(True)
    report = probe.report()
    assert report["host_execution_bridge"] is True
    assert report["main_thread_executor"] is True

    full = fallback.ReadinessProbe.fully_ready()
    assert all(full.report().values())

    class _FakeServer:
        def __init__(self) -> None:
            self.probe = None

        def set_readiness_probe(self, probe_obj: object) -> None:
            self.probe = probe_obj

    server = _FakeServer()
    binder = readiness.AdapterReadinessBinder.bind_inline(server)
    assert binder.probe.is_ready() is True
    assert server.probe is binder.probe


def test_gui_executable_fallback(monkeypatch, tmp_path) -> None:
    """is_gui_executable / correct_python_executable work without _core."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]

    maya = tmp_path / "maya.exe"
    mayapy = tmp_path / "mayapy.exe"
    maya.write_text("", encoding="utf-8")
    mayapy.write_text("", encoding="utf-8")

    hint = fallback.is_gui_executable(str(maya))
    assert hint is not None
    assert hint.dcc_kind == "maya"
    assert hint.recommended_replacement == mayapy
    assert fallback.correct_python_executable(str(maya)) == mayapy

    assert fallback.is_gui_executable(str(mayapy)) is None
    assert fallback.is_gui_executable("python.exe") is None
    assert fallback.correct_python_executable("/usr/bin/python3") == Path("/usr/bin/python3")


def test_scan_and_load_strict_fallback(monkeypatch, tmp_path) -> None:
    """scan_and_load_strict works without _core (py37-lite)."""
    modules = _import_without_core(
        monkeypatch,
        "dcc_mcp_core._py37_fallback",
    )
    fallback = modules["dcc_mcp_core._py37_fallback"]

    good = tmp_path / "good-skill"
    good.mkdir()
    (good / "SKILL.md").write_text(
        "---\nname: good-skill\ndescription: ok\nmetadata:\n  dcc-mcp:\n    dcc: maya\n---\n# Good\n",
        encoding="utf-8",
    )

    bad = tmp_path / "bad-skill"
    bad.mkdir()
    (bad / "SKILL.md").write_text("# missing frontmatter\n", encoding="utf-8")

    skills, skipped = fallback.scan_and_load_strict(extra_paths=[str(good)])
    assert len(skills) == 1
    assert skills[0].name == "good-skill"
    assert skipped == []

    generic = tmp_path / "generic-skill"
    generic.mkdir()
    (generic / "SKILL.md").write_text(
        "---\nname: generic-skill\ndescription: Cross-DCC infrastructure\n---\n",
        encoding="utf-8",
    )
    skills, skipped = fallback.scan_and_load_strict(extra_paths=[str(generic)], dcc_name="maya")
    assert [skill.name for skill in skills] == ["generic-skill"]
    assert skills[0].dcc == "python"
    assert skipped == []

    try:
        fallback.scan_and_load_strict(extra_paths=[str(tmp_path)])
    except ValueError as exc:
        message = str(exc)
        assert "Strict scan rejected 1 directory" in message
        assert "bad-skill" in message
        assert "scan_and_load_lenient" in message
    else:
        raise AssertionError("Expected ValueError for bad-skill directory")
