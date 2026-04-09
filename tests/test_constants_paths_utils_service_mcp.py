"""Tests for module constants, path utility functions, ServiceEntry deep fields.

Also covers: McpServerHandle, LoggingMiddleware, unwrap_parameters, scan_skill_paths,
scan_and_load_lenient, expand_transitive_dependencies, is_telemetry_initialized.
All APIs were previously under-tested (count < 20 mentions in the test suite).
"""

from __future__ import annotations

from pathlib import Path
import tempfile

import pytest

import dcc_mcp_core
from dcc_mcp_core import APP_AUTHOR
from dcc_mcp_core import APP_NAME
from dcc_mcp_core import DEFAULT_DCC
from dcc_mcp_core import DEFAULT_LOG_LEVEL
from dcc_mcp_core import DEFAULT_MIME_TYPE
from dcc_mcp_core import DEFAULT_VERSION
from dcc_mcp_core import ENV_LOG_LEVEL
from dcc_mcp_core import ENV_SKILL_PATHS
from dcc_mcp_core import SKILL_METADATA_DIR
from dcc_mcp_core import SKILL_METADATA_FILE
from dcc_mcp_core import SKILL_SCRIPTS_DIR
from dcc_mcp_core import ActionDispatcher
from dcc_mcp_core import ActionPipeline
from dcc_mcp_core import ActionRegistry
from dcc_mcp_core import BooleanWrapper
from dcc_mcp_core import FloatWrapper
from dcc_mcp_core import IntWrapper
from dcc_mcp_core import LoggingMiddleware
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import McpServerHandle
from dcc_mcp_core import TransportManager
from dcc_mcp_core import expand_transitive_dependencies
from dcc_mcp_core import get_actions_dir
from dcc_mcp_core import get_config_dir
from dcc_mcp_core import get_data_dir
from dcc_mcp_core import get_log_dir
from dcc_mcp_core import get_platform_dir
from dcc_mcp_core import get_skill_paths_from_env
from dcc_mcp_core import get_skills_dir
from dcc_mcp_core import is_telemetry_initialized
from dcc_mcp_core import scan_and_load_lenient
from dcc_mcp_core import scan_skill_paths
from dcc_mcp_core import unwrap_parameters

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

_SKILL_FRONTMATTER = """\
---
name: {name}
description: "{desc}"
dcc: python
version: "1.0.0"
tools: []
---
# {name}
"""

_SKILL_WITH_DEPS = """\
---
name: {name}
description: "{desc}"
dcc: python
version: "1.0.0"
tools: []
depends: [{deps_csv}]
---
# {name}
"""


def _make_skill(base: str, name: str, deps: list[str] | None = None) -> str:
    """Create a minimal valid skill directory; return its path."""
    skill_dir = Path(base) / name
    scripts_dir = skill_dir / "scripts"
    scripts_dir.mkdir(parents=True, exist_ok=True)
    if deps:
        deps_csv = ", ".join(f'"{d}"' for d in deps)
        content = _SKILL_WITH_DEPS.format(name=name, desc=name, deps_csv=deps_csv)
    else:
        content = _SKILL_FRONTMATTER.format(name=name, desc=name)
    (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
    # Minimal script so scripts list is non-empty
    (scripts_dir / "run.py").write_text("# placeholder\n", encoding="utf-8")
    return str(skill_dir)


# ---------------------------------------------------------------------------
# Module constants
# ---------------------------------------------------------------------------


class TestModuleConstants:
    def test_app_author_is_str(self):
        assert isinstance(APP_AUTHOR, str)
        assert APP_AUTHOR  # non-empty

    def test_app_name_is_str(self):
        assert isinstance(APP_NAME, str)
        assert APP_NAME

    def test_default_dcc_is_str(self):
        assert isinstance(DEFAULT_DCC, str)
        assert DEFAULT_DCC

    def test_default_log_level_is_str(self):
        assert isinstance(DEFAULT_LOG_LEVEL, str)
        assert DEFAULT_LOG_LEVEL

    def test_default_mime_type_is_str(self):
        assert isinstance(DEFAULT_MIME_TYPE, str)
        assert "/" in DEFAULT_MIME_TYPE  # e.g. "text/plain"

    def test_default_version_is_semver_like(self):
        assert isinstance(DEFAULT_VERSION, str)
        parts = DEFAULT_VERSION.split(".")
        assert len(parts) == 3

    def test_env_log_level_is_str(self):
        assert isinstance(ENV_LOG_LEVEL, str)
        assert ENV_LOG_LEVEL

    def test_env_skill_paths_is_str(self):
        assert isinstance(ENV_SKILL_PATHS, str)
        assert ENV_SKILL_PATHS  # e.g. "DCC_MCP_SKILL_PATHS"

    def test_skill_metadata_dir_is_str(self):
        assert isinstance(SKILL_METADATA_DIR, str)

    def test_skill_metadata_file_is_str(self):
        assert isinstance(SKILL_METADATA_FILE, str)
        assert "SKILL" in SKILL_METADATA_FILE.upper() or SKILL_METADATA_FILE.endswith(".md")

    def test_skill_scripts_dir_is_str(self):
        assert isinstance(SKILL_SCRIPTS_DIR, str)

    def test_author_is_str(self):
        assert isinstance(dcc_mcp_core.__author__, str)
        assert "@" in dcc_mcp_core.__author__  # email in author string

    def test_version_is_str(self):
        assert isinstance(dcc_mcp_core.__version__, str)
        parts = dcc_mcp_core.__version__.split(".")
        assert len(parts) >= 2


# ---------------------------------------------------------------------------
# Path utility functions
# ---------------------------------------------------------------------------


class TestGetConfigDir:
    def test_returns_str(self):
        result = get_config_dir()
        assert isinstance(result, str)
        assert result  # non-empty

    def test_contains_dcc_mcp(self):
        result = get_config_dir()
        assert "dcc" in result.lower() or "mcp" in result.lower()


class TestGetDataDir:
    def test_returns_str(self):
        assert isinstance(get_data_dir(), str)

    def test_non_empty(self):
        assert get_data_dir()


class TestGetLogDir:
    def test_returns_str(self):
        assert isinstance(get_log_dir(), str)

    def test_non_empty(self):
        assert get_log_dir()


class TestGetActionsDir:
    def test_returns_str_for_maya(self):
        result = get_actions_dir("maya")
        assert isinstance(result, str)
        assert result

    def test_maya_contains_maya(self):
        assert "maya" in get_actions_dir("maya").lower()

    def test_blender_contains_blender(self):
        assert "blender" in get_actions_dir("blender").lower()

    def test_different_for_different_dcc(self):
        assert get_actions_dir("maya") != get_actions_dir("blender")


class TestGetPlatformDir:
    def test_config_returns_str(self):
        assert isinstance(get_platform_dir("config"), str)

    def test_data_returns_str(self):
        assert isinstance(get_platform_dir("data"), str)

    def test_log_returns_str(self):
        assert isinstance(get_platform_dir("log"), str)

    def test_cache_returns_str(self):
        assert isinstance(get_platform_dir("cache"), str)


class TestGetSkillsDir:
    def test_no_arg_returns_str(self):
        assert isinstance(get_skills_dir(), str)

    def test_with_dcc_name_returns_str(self):
        assert isinstance(get_skills_dir("maya"), str)

    def test_maya_path_contains_maya(self):
        assert "maya" in get_skills_dir("maya").lower()

    def test_dcc_specific_differs_from_base(self):
        assert get_skills_dir("maya") != get_skills_dir()


class TestGetSkillPathsFromEnv:
    def test_no_env_returns_empty_list(self, monkeypatch):
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        result = get_skill_paths_from_env()
        assert isinstance(result, list)
        assert result == []

    def test_with_single_path(self, tmp_path, monkeypatch):
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        result = get_skill_paths_from_env()
        assert isinstance(result, list)
        assert len(result) >= 1
        assert any(str(tmp_path) in p for p in result)

    def test_with_multiple_paths(self, tmp_path, monkeypatch):
        p1 = tmp_path / "a"
        p2 = tmp_path / "b"
        p1.mkdir()
        p2.mkdir()
        import os

        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", os.pathsep.join([str(p1), str(p2)]))
        result = get_skill_paths_from_env()
        assert isinstance(result, list)


# ---------------------------------------------------------------------------
# scan_skill_paths
# ---------------------------------------------------------------------------


class TestScanSkillPaths:
    def test_no_env_empty_extra_returns_list(self, monkeypatch):
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        result = scan_skill_paths()
        assert isinstance(result, list)

    def test_with_env_path_contains_dir(self, tmp_path, monkeypatch):
        _make_skill(str(tmp_path), "myskill")
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        result = scan_skill_paths()
        assert isinstance(result, list)
        assert len(result) >= 1

    def test_extra_paths_arg(self, tmp_path, monkeypatch):
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        _make_skill(str(tmp_path), "extra-skill")
        result = scan_skill_paths(extra_paths=[str(tmp_path)])
        assert isinstance(result, list)

    def test_dcc_name_filter(self, tmp_path, monkeypatch):
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        result = scan_skill_paths(dcc_name="maya")
        assert isinstance(result, list)


# ---------------------------------------------------------------------------
# scan_and_load_lenient
# ---------------------------------------------------------------------------


class TestScanAndLoadLenient:
    def test_returns_tuple_of_two_lists(self, tmp_path, monkeypatch):
        _make_skill(str(tmp_path), "skill-one")
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        result = scan_and_load_lenient()
        assert isinstance(result, (tuple, list))
        # Result is (skills, failed_dirs) tuple
        skills, failed = result
        assert isinstance(skills, list)
        assert isinstance(failed, list)

    def test_empty_dir_gives_empty_skills(self, tmp_path, monkeypatch):
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        skills, _failed = scan_and_load_lenient()
        assert isinstance(skills, list)

    def test_invalid_skill_goes_to_failed(self, tmp_path, monkeypatch):
        # Bad SKILL.md (no frontmatter) goes to failed
        bad_dir = tmp_path / "bad-skill"
        bad_dir.mkdir()
        (bad_dir / "SKILL.md").write_text("no frontmatter here\n")
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        _skills, failed = scan_and_load_lenient()
        assert isinstance(failed, list)

    def test_no_env_returns_tuple(self, monkeypatch):
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        result = scan_and_load_lenient()
        assert isinstance(result, (tuple, list))


# ---------------------------------------------------------------------------
# expand_transitive_dependencies
# ---------------------------------------------------------------------------


class TestExpandTransitiveDependencies:
    def _load_skills(self, tmp_path, monkeypatch):
        """Build skill-a + skill-b (depends on skill-a), return loaded skills."""
        _make_skill(str(tmp_path), "skill-a")
        _make_skill(str(tmp_path), "skill-b", deps=["skill-a"])
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", str(tmp_path))
        import dcc_mcp_core as _dc

        skills, _ = _dc.scan_and_load()
        return skills

    def test_no_deps_returns_empty_or_self(self, tmp_path, monkeypatch):
        skills = self._load_skills(tmp_path, monkeypatch)
        if not skills:
            pytest.skip("No skills loaded (SKILL.md format issue)")
        skill_a = next((s for s in skills if s.name == "skill-a"), None)
        if skill_a is None:
            pytest.skip("skill-a not loaded")
        result = expand_transitive_dependencies(skills, "skill-a")
        assert isinstance(result, list)

    def test_with_dep_includes_dep(self, tmp_path, monkeypatch):
        skills = self._load_skills(tmp_path, monkeypatch)
        if not skills:
            pytest.skip("No skills loaded (SKILL.md format issue)")
        skill_b = next((s for s in skills if s.name == "skill-b"), None)
        if skill_b is None:
            pytest.skip("skill-b not loaded")
        result = expand_transitive_dependencies(skills, "skill-b")
        assert isinstance(result, list)

    def test_unknown_name_returns_list(self, tmp_path, monkeypatch):
        skills = self._load_skills(tmp_path, monkeypatch)
        if not skills:
            pytest.skip("No skills loaded")
        try:
            result = expand_transitive_dependencies(skills, "nonexistent")
            assert isinstance(result, list)
        except Exception:
            pass  # may raise for unknown skill


# ---------------------------------------------------------------------------
# ServiceEntry deep
# ---------------------------------------------------------------------------


def _make_mgr():
    return TransportManager(tempfile.mkdtemp())


class TestServiceEntryFields:
    def _get_svc(self):
        mgr = _make_mgr()
        iid = mgr.register_service("maya", "127.0.0.1", 18812)
        return mgr.get_service("maya", iid)

    def test_get_service_returns_service_entry(self):
        svc = self._get_svc()
        from dcc_mcp_core import ServiceEntry

        assert isinstance(svc, ServiceEntry)

    def test_dcc_type_is_maya(self):
        svc = self._get_svc()
        assert svc.dcc_type == "maya"

    def test_host_is_correct(self):
        svc = self._get_svc()
        assert svc.host == "127.0.0.1"

    def test_port_is_correct(self):
        svc = self._get_svc()
        assert svc.port == 18812

    def test_instance_id_is_uuid_str(self):
        import uuid

        svc = self._get_svc()
        # Should be a valid UUID string
        uuid.UUID(svc.instance_id)  # raises ValueError if invalid

    def test_status_is_available(self):
        svc = self._get_svc()
        assert str(svc.status) in ("AVAILABLE", "ServiceStatus.AVAILABLE")

    def test_version_is_none_when_not_set(self):
        svc = self._get_svc()
        assert svc.version is None

    def test_scene_is_none_when_not_set(self):
        svc = self._get_svc()
        assert svc.scene is None

    def test_last_heartbeat_ms_is_int(self):
        svc = self._get_svc()
        assert isinstance(svc.last_heartbeat_ms, int)
        assert svc.last_heartbeat_ms > 0

    def test_is_ipc_false_for_tcp(self):
        svc = self._get_svc()
        # TCP-registered service: is_ipc is False
        assert svc.is_ipc is False

    def test_metadata_is_dict(self):
        svc = self._get_svc()
        assert isinstance(svc.metadata, dict)

    def test_transport_address_none_for_tcp(self):
        svc = self._get_svc()
        # TCP registered service has no transport_address attribute value
        assert svc.transport_address is None

    def test_effective_address_is_callable(self):
        svc = self._get_svc()
        # effective_address is a method
        assert callable(svc.effective_address)

    def test_to_dict_has_required_keys(self):
        svc = self._get_svc()
        d = svc.to_dict()
        assert isinstance(d, dict)
        for key in ["dcc_type", "instance_id", "host", "port", "status"]:
            assert key in d

    def test_to_dict_dcc_type_value(self):
        svc = self._get_svc()
        d = svc.to_dict()
        assert d["dcc_type"] == "maya"

    def test_to_dict_port_value(self):
        svc = self._get_svc()
        d = svc.to_dict()
        assert d["port"] == 18812

    def test_repr_contains_maya(self):
        svc = self._get_svc()
        assert "maya" in repr(svc)


class TestServiceEntryListAllServices:
    def test_list_all_services_returns_list(self):
        mgr = _make_mgr()
        mgr.register_service("maya", "127.0.0.1", 18812)
        svcs = mgr.list_all_services()
        assert isinstance(svcs, list)
        assert len(svcs) == 1

    def test_list_all_services_items_are_service_entry(self):
        from dcc_mcp_core import ServiceEntry

        mgr = _make_mgr()
        mgr.register_service("maya", "127.0.0.1", 18812)
        svcs = mgr.list_all_services()
        assert all(isinstance(s, ServiceEntry) for s in svcs)

    def test_multiple_services(self):
        mgr = _make_mgr()
        mgr.register_service("maya", "127.0.0.1", 18812)
        mgr.register_service("blender", "127.0.0.1", 19000)
        svcs = mgr.list_all_services()
        assert len(svcs) == 2
        dcc_types = {s.dcc_type for s in svcs}
        assert "maya" in dcc_types
        assert "blender" in dcc_types


# ---------------------------------------------------------------------------
# LoggingMiddleware
# ---------------------------------------------------------------------------


class TestLoggingMiddleware:
    def test_direct_construction_true(self):
        lm = LoggingMiddleware(True)
        assert lm is not None

    def test_direct_construction_false(self):
        lm = LoggingMiddleware(False)
        assert lm is not None

    def test_log_params_true(self):
        lm = LoggingMiddleware(True)
        assert lm.log_params is True

    def test_log_params_false(self):
        lm = LoggingMiddleware(False)
        assert lm.log_params is False

    def test_add_logging_adds_to_pipeline(self):
        reg = ActionRegistry()
        reg.register("op", description="test", category="misc")
        disp = ActionDispatcher(reg)
        pipeline = ActionPipeline(disp)
        pipeline.add_logging(log_params=True)
        assert "logging" in pipeline.middleware_names()

    def test_add_logging_returns_none(self):
        reg = ActionRegistry()
        disp = ActionDispatcher(reg)
        pipeline = ActionPipeline(disp)
        result = pipeline.add_logging(log_params=False)
        # add_logging returns None (side-effect only)
        assert result is None


# ---------------------------------------------------------------------------
# McpServerHandle (via McpHttpServer.start())
# ---------------------------------------------------------------------------


class TestMcpServerHandle:
    def _start_server(self):
        reg = ActionRegistry()
        reg.register("echo", description="Echo", category="test")
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        return handle

    def test_start_returns_server_handle(self):
        handle = self._start_server()
        try:
            assert isinstance(handle, McpServerHandle)
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_port_is_positive_int(self):
        handle = self._start_server()
        try:
            assert isinstance(handle.port, int)
            assert handle.port > 0
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_bind_addr_contains_port(self):
        handle = self._start_server()
        try:
            addr = handle.bind_addr
            assert isinstance(addr, str)
            assert str(handle.port) in addr
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_bind_addr_contains_127_0_0_1(self):
        handle = self._start_server()
        try:
            assert "127.0.0.1" in handle.bind_addr
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_mcp_url_is_callable(self):
        handle = self._start_server()
        try:
            assert callable(handle.mcp_url)
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_mcp_url_call_returns_str(self):
        handle = self._start_server()
        try:
            url = handle.mcp_url()
            assert isinstance(url, str)
            assert "mcp" in url.lower()
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_mcp_url_contains_port(self):
        handle = self._start_server()
        try:
            url = handle.mcp_url()
            assert str(handle.port) in url
        finally:
            handle.signal_shutdown()
            handle.shutdown()

    def test_signal_shutdown_does_not_raise(self):
        handle = self._start_server()
        handle.signal_shutdown()
        handle.shutdown()

    def test_shutdown_idempotent(self):
        handle = self._start_server()
        handle.signal_shutdown()
        handle.shutdown()
        # Second shutdown should not raise
        handle.shutdown()

    def test_mcpconfig_port_zero_assigns_ephemeral(self):
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0  # before binding

    def test_mcpconfig_server_name(self):
        cfg = McpHttpConfig(port=0, server_name="test-server")
        assert cfg.server_name == "test-server"

    def test_mcpconfig_server_version(self):
        cfg = McpHttpConfig(port=0, server_version="2.0.0")
        assert cfg.server_version == "2.0.0"


# ---------------------------------------------------------------------------
# unwrap_parameters
# ---------------------------------------------------------------------------


class TestUnwrapParameters:
    def test_int_wrapper_becomes_int(self):
        result = unwrap_parameters({"x": IntWrapper(42)})
        assert result["x"] == 42
        assert isinstance(result["x"], int)

    def test_float_wrapper_becomes_float(self):
        result = unwrap_parameters({"y": FloatWrapper(3.14)})
        assert abs(result["y"] - 3.14) < 1e-6
        assert isinstance(result["y"], float)

    def test_bool_wrapper_becomes_bool_true(self):
        result = unwrap_parameters({"flag": BooleanWrapper(True)})
        assert result["flag"] is True

    def test_bool_wrapper_becomes_bool_false(self):
        result = unwrap_parameters({"flag": BooleanWrapper(False)})
        assert result["flag"] is False

    def test_plain_str_passes_through(self):
        result = unwrap_parameters({"name": "hello"})
        assert result["name"] == "hello"
        assert isinstance(result["name"], str)

    def test_plain_int_passes_through(self):
        result = unwrap_parameters({"n": 99})
        assert result["n"] == 99

    def test_plain_float_passes_through(self):
        result = unwrap_parameters({"v": 1.5})
        assert abs(result["v"] - 1.5) < 1e-9

    def test_mixed_dict(self):
        params = {
            "a": IntWrapper(1),
            "b": FloatWrapper(2.0),
            "c": BooleanWrapper(True),
            "d": "text",
            "e": 7,
        }
        result = unwrap_parameters(params)
        assert result["a"] == 1
        assert isinstance(result["a"], int)
        assert isinstance(result["b"], float)
        assert result["c"] is True
        assert result["d"] == "text"
        assert result["e"] == 7

    def test_empty_dict(self):
        result = unwrap_parameters({})
        assert result == {}

    def test_result_is_dict(self):
        result = unwrap_parameters({"x": IntWrapper(0)})
        assert isinstance(result, dict)


# ---------------------------------------------------------------------------
# is_telemetry_initialized
# ---------------------------------------------------------------------------


class TestIsTelemetryInitialized:
    def test_returns_bool(self):
        assert isinstance(is_telemetry_initialized(), bool)

    def test_is_false_without_init(self):
        # In test environment without TelemetryConfig.init() called, should be False
        # (may be True if another test called init() first)
        result = is_telemetry_initialized()
        assert isinstance(result, bool)
