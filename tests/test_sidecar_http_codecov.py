"""Coverage tests for py37-lite sidecar HTTP refactor (PIP-2535).

Targets Codecov patch gaps in ``server_base.py``, ``skill_paths.py``, and
``sidecar_skill_server.py`` introduced by PR #1806.
"""

from __future__ import annotations

import os
from pathlib import Path
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

try:
    from importlib import metadata as importlib_metadata
except ImportError:  # Python 3.7
    import importlib_metadata  # type: ignore[import-not-found]

from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarServerHandle
from dcc_mcp_core._runtime.sidecar_skill_server import _port_from_url
from dcc_mcp_core._runtime.sidecar_skill_server import _resolve_mcp_url


def _force_core_unavailable(monkeypatch: pytest.MonkeyPatch) -> None:
    """Pin the cached core-availability probe to the py37-lite profile."""
    import dcc_mcp_core._runtime.core_availability as core_availability

    monkeypatch.setattr(core_availability, "_CORE_AVAILABLE", False)
    monkeypatch.setattr(
        "dcc_mcp_core._runtime.core_availability.is_core_extension_available",
        lambda: False,
    )
    monkeypatch.setattr(
        "dcc_mcp_core._runtime.skill_paths.is_core_extension_available",
        lambda: False,
    )
    monkeypatch.setattr(
        "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
        lambda: False,
    )
    monkeypatch.setattr(
        "dcc_mcp_core.server_base.is_core_extension_available",
        lambda: False,
    )


class TestSkillPathsPy37Lite:
    def test_get_skill_paths_from_env_empty(self, monkeypatch: pytest.MonkeyPatch) -> None:
        _force_core_unavailable(monkeypatch)
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        from dcc_mcp_core._runtime.skill_paths import get_skill_paths_from_env

        assert get_skill_paths_from_env() == []

    def test_get_skill_paths_from_env_splits_os_pathsep(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        _force_core_unavailable(monkeypatch)
        first = str(tmp_path / "alpha")
        second = str(tmp_path / "beta")
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", os.pathsep.join([first, second]))
        from dcc_mcp_core._runtime.skill_paths import get_skill_paths_from_env

        assert get_skill_paths_from_env() == [first, second]

    def test_get_app_skill_paths_from_env_empty(self, monkeypatch: pytest.MonkeyPatch) -> None:
        _force_core_unavailable(monkeypatch)
        monkeypatch.delenv("DCC_MCP_MAYA_SKILL_PATHS", raising=False)
        from dcc_mcp_core._runtime.skill_paths import get_app_skill_paths_from_env

        assert get_app_skill_paths_from_env("maya") == []

    def test_get_app_skill_paths_from_env_splits(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        _force_core_unavailable(monkeypatch)
        path = str(tmp_path / "show-skills")
        monkeypatch.setenv("DCC_MCP_MAYA_SKILL_PATHS", path)
        from dcc_mcp_core._runtime.skill_paths import get_app_skill_paths_from_env

        assert get_app_skill_paths_from_env("maya") == [path]

    def test_get_local_skills_dir_uses_home_slug(self, monkeypatch: pytest.MonkeyPatch) -> None:
        _force_core_unavailable(monkeypatch)
        from dcc_mcp_core._runtime.skill_paths import get_local_skills_dir

        result = get_local_skills_dir("Maya")
        assert ".dcc-mcp" in result
        assert result.endswith("maya/skills") or result.endswith("maya\\skills")

    def test_get_skills_dir_returns_default_home_path(self, monkeypatch: pytest.MonkeyPatch) -> None:
        _force_core_unavailable(monkeypatch)
        from dcc_mcp_core._runtime.skill_paths import get_skills_dir

        result = get_skills_dir()
        assert result is not None
        assert ".dcc-mcp" in result
        assert result.endswith("skills")

    def test_delegates_to_core_when_available(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import sys
        import types

        fake_core = types.ModuleType("dcc_mcp_core._core")

        def _paths():
            return ["/from-core"]

        fake_core.get_skill_paths_from_env = _paths
        monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.skill_paths.is_core_extension_available",
            lambda: True,
        )
        from dcc_mcp_core._runtime.skill_paths import get_skill_paths_from_env

        assert get_skill_paths_from_env() == ["/from-core"]

    def test_delegates_app_paths_to_core_when_available(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import sys
        import types

        fake_core = types.ModuleType("dcc_mcp_core._core")
        fake_core.get_app_skill_paths_from_env = lambda dcc: [f"/app-{dcc}"]
        fake_core.get_local_skills_dir = lambda dcc: f"/local/{dcc}"
        fake_core.get_skills_dir = lambda: "/global/skills"
        monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.skill_paths.is_core_extension_available",
            lambda: True,
        )
        from dcc_mcp_core._runtime.skill_paths import get_app_skill_paths_from_env
        from dcc_mcp_core._runtime.skill_paths import get_local_skills_dir
        from dcc_mcp_core._runtime.skill_paths import get_skills_dir

        assert get_app_skill_paths_from_env("blender") == ["/app-blender"]
        assert get_local_skills_dir("blender") == "/local/blender"
        assert get_skills_dir() == "/global/skills"


class TestSidecarSkillServerCoverage:
    def test_registry_property_returns_python_registry(self) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        assert server.registry is server._registry

    def test_shutdown_noop_when_process_missing(self, monkeypatch: pytest.MonkeyPatch) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            lambda **kwargs: {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": None,
            },
        )
        server.start().shutdown()

    def test_port_from_url_returns_zero_when_urlsplit_raises(self, monkeypatch: pytest.MonkeyPatch) -> None:
        def _boom(_url: str):
            raise ValueError("broken parser")

        monkeypatch.setattr("urllib.parse.urlsplit", _boom)
        assert _port_from_url("http://127.0.0.1:1/mcp") == 0

    def test_sidecar_server_handle_surface(self) -> None:
        shutdown = MagicMock()
        handle = SidecarServerHandle(port=8765, mcp_url="http://127.0.0.1:8765/mcp", shutdown=shutdown)
        assert handle.port == 8765
        assert handle.mcp_url() == "http://127.0.0.1:8765/mcp"
        assert handle.update_gateway_metadata(scene="test") is False
        handle.shutdown()
        shutdown.assert_called_once()

    def test_discover_keeps_host_env_isolated(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
        existing = str(tmp_path / "existing")
        extra = str(tmp_path / "extra")
        monkeypatch.setenv("DCC_MCP_SKILL_PATHS", existing)
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        assert server.discover(extra_paths=[extra]) == 0
        assert os.environ["DCC_MCP_SKILL_PATHS"] == existing
        assert server._launch_environment()["DCC_MCP_SKILL_PATHS"].split(os.pathsep) == [extra, existing]

    def test_discover_empty_paths_is_noop(self) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        assert server.discover(extra_paths=[]) == 0

    def test_start_returns_cached_handle(self, monkeypatch: pytest.MonkeyPatch) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            lambda **kwargs: {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": MagicMock(),
            },
        )
        first = server.start()
        second = server.start()
        assert first is second

    def test_start_raises_when_build_command_fails(self, monkeypatch: pytest.MonkeyPatch) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.build_sidecar_command",
            lambda **kwargs: {"success": False, "message": "bad command"},
        )
        with pytest.raises(RuntimeError, match="bad command"):
            server.start()

    def test_start_raises_when_launch_fails(self, monkeypatch: pytest.MonkeyPatch) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            lambda **kwargs: {"success": False, "message": "launch boom"},
        )
        with pytest.raises(RuntimeError, match="launch boom"):
            server.start()

    def test_shutdown_swallows_terminate_errors(self, monkeypatch: pytest.MonkeyPatch) -> None:
        proc = MagicMock()
        proc.terminate.side_effect = OSError("already dead")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            lambda **kwargs: {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": proc,
            },
        )
        handle = server.start()
        handle.shutdown()

    def test_skill_activation_fails_in_dispatch_only_mode(self) -> None:
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        assert server.list_skills() == []
        assert server.search_skills(query="x") == []
        with pytest.raises(RuntimeError, match="dispatch-only"):
            server.load_skill("demo")
        assert server.get_skill("demo") is None
        with pytest.raises(RuntimeError, match="dispatch-only"):
            server.load_skill_object(object())
        with pytest.raises(RuntimeError, match="dispatch-only"):
            server.unload_skill("demo")
        assert server.is_loaded("demo") is False
        assert server.get_skill_info("demo") is None
        server.set_in_process_executor(object())
        server.attach_dispatcher(object())
        assert server.resources() is None
        assert server.set_readiness_probe(lambda: True) is False

    @pytest.mark.parametrize(
        ("launch", "expected"),
        [
            ({"readiness": {"discovery_mcp_url": "http://127.0.0.1:1111/mcp"}}, "http://127.0.0.1:1111/mcp"),
            ({"endpoint": "http://127.0.0.1:2222/mcp"}, "http://127.0.0.1:2222/mcp"),
            (
                {"readiness": {"metadata": {"mcp_url": "http://127.0.0.1:3333/mcp"}}},
                "http://127.0.0.1:3333/mcp",
            ),
            ({}, "http://127.0.0.1:0/mcp"),
        ],
    )
    def test_resolve_mcp_url_prefers_known_keys(self, launch: dict, expected: str) -> None:
        assert _resolve_mcp_url(launch) == expected

    def test_port_from_url_parses_explicit_port(self) -> None:
        assert _port_from_url("http://127.0.0.1:4242/mcp") == 4242

    def test_port_from_url_returns_zero_on_invalid_url(self) -> None:
        assert _port_from_url("not-a-url") == 0


class TestServerBasePackageVersion:
    def test_uses_core_version_when_extension_available(self, monkeypatch: pytest.MonkeyPatch) -> None:
        import types

        import dcc_mcp_core

        fake_core = types.ModuleType("dcc_mcp_core._core")
        fake_core.__version__ = "9.9.9-core"
        monkeypatch.setattr(dcc_mcp_core, "_core", fake_core)
        monkeypatch.setattr(
            "dcc_mcp_core.server_base.is_core_extension_available",
            lambda: True,
        )
        from dcc_mcp_core.server_base import _package_version

        assert _package_version() == "9.9.9-core"

    def test_uses_distribution_metadata_when_core_unavailable(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(
            "dcc_mcp_core.server_base.is_core_extension_available",
            lambda: False,
        )
        monkeypatch.setattr(importlib_metadata, "version", lambda _name: "0.19.7")
        from dcc_mcp_core.server_base import _package_version

        assert _package_version() == "0.19.7"

    def test_falls_back_when_metadata_lookup_fails(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setattr(
            "dcc_mcp_core.server_base.is_core_extension_available",
            lambda: False,
        )

        def _boom(_name: str) -> str:
            raise RuntimeError("no dist")

        monkeypatch.setattr(importlib_metadata, "version", _boom)
        from dcc_mcp_core.server_base import _PKG_VERSION
        from dcc_mcp_core.server_base import _package_version

        assert _package_version() == _PKG_VERSION


class TestServerBaseSidecarWiring:
    def test_init_wires_sidecar_server_when_core_unavailable(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _force_core_unavailable(monkeypatch)
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core._server.options import SidecarOptions
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()
        options = DccServerOptions(
            dcc_name="maya",
            builtin_skills_dir=skills_dir,
            port=0,
            sidecar=SidecarOptions(host_rpc="commandport://127.0.0.1:6000"),
        )

        class _Stub(DccServerBase):
            pass

        with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_job_persistence"):
                with patch("dcc_mcp_core.server_base.DccServerBase._register_builtin_skills"):
                    server = _Stub(options)

        assert isinstance(server._server, SidecarBackedSkillServer)
        assert server._server._host_rpc == "commandport://127.0.0.1:6000"

    def test_init_registers_inprocess_executor_when_inline_mode(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _force_core_unavailable(monkeypatch)
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core._server.options import ExecutionOptions
        from dcc_mcp_core._server.options import SidecarOptions
        from dcc_mcp_core._server.options import StandaloneMainThreadExecution
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()
        options = DccServerOptions(
            dcc_name="maya",
            builtin_skills_dir=skills_dir,
            port=0,
            execution=ExecutionOptions(mode=StandaloneMainThreadExecution),
            sidecar=SidecarOptions(host_rpc="commandport://127.0.0.1:6000"),
        )

        class _Stub(DccServerBase):
            pass

        mock_server = MagicMock()
        with patch("dcc_mcp_core.server_base.create_adapter_server", return_value=mock_server):
            with patch("dcc_mcp_core.server_base.DccServerBase._init_file_logging", return_value=""):
                with patch("dcc_mcp_core.server_base.DccServerBase._init_job_persistence"):
                    with patch("dcc_mcp_core.server_base.DccServerBase._register_builtin_skills"):
                        with patch.object(
                            DccServerBase,
                            "_get_execution",
                        ) as get_execution:
                            execution = MagicMock()
                            get_execution.return_value = execution
                            _Stub(options)
                            execution.register_inprocess_executor.assert_called_once_with(None)

    def test_lazy_controller_getters_recreate_missing_seams(self, tmp_path: Path) -> None:
        from dcc_mcp_core._server.execution_bridge import ExecutionBridgeBinder
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core._server.skill_discovery import SkillDiscoveryController
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()
        server = DccServerBase.__new__(DccServerBase)
        server.__dict__.clear()
        server._options = DccServerOptions(dcc_name="maya", builtin_skills_dir=skills_dir)

        discovery = server._get_skill_discovery()
        execution = server._get_execution()
        lifecycle = server._get_lifecycle_ctrl()
        observability = server._get_observability()

        assert isinstance(discovery, SkillDiscoveryController)
        assert isinstance(execution, ExecutionBridgeBinder)
        assert lifecycle is not None
        assert observability is not None
