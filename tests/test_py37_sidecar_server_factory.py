"""Tests for py37-lite sidecar server factory and pure-Python McpHttpConfig."""

from __future__ import annotations

from dataclasses import dataclass
import os
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class
from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig
from dcc_mcp_core._runtime.server_factory import create_adapter_server
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer
from dcc_mcp_core._server.skill_query import SkillQueryClient
from dcc_mcp_core.constants import ENV_DISABLE_ACCUMULATED_SKILLS
from dcc_mcp_core.constants import ENV_DISABLE_DEFAULT_SKILL_PATHS
from dcc_mcp_core.constants import ENV_SKILL_PATHS


@dataclass(frozen=True)
class _SidecarStub:
    host_rpc: str
    adapter_version: str | None = None
    display_name: str | None = None
    wait_ready_timeout_secs: float = 15.0
    server_bin: str | None = None
    extra_args: tuple[str, ...] = ()


@dataclass(frozen=True)
class _DiagnosticsStub:
    dcc_pid: int | None = None


@dataclass(frozen=True)
class _OptionsStub:
    dcc_name: str
    sidecar: _SidecarStub
    diagnostics: _DiagnosticsStub = _DiagnosticsStub()


def _write_skill(root: Path, name: str, *, dcc: str = "maya", tags: str = "[test]") -> Path:
    skill_dir = root / name
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(
        f"---\nname: {name}\ndescription: {name} metadata skill\n"
        f"metadata:\n  dcc-mcp:\n    dcc: {dcc}\n    tags: {tags}\n---\n",
        encoding="utf-8",
    )
    return skill_dir


_REPO_ROOT = Path(__file__).resolve().parents[1]


class TestPureMcpHttpConfig:
    def test_defaults_match_rust_surface(self):
        cfg = PureMcpHttpConfig()
        assert cfg.port == 0
        assert cfg.server_name == "dcc-mcp"
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0
        assert cfg.gateway_port == 9765
        assert cfg.session_ttl_secs == 3600

    def test_repr_contains_name_and_port(self):
        cfg = PureMcpHttpConfig(port=1234, server_name="maya-mcp")
        text = repr(cfg)
        assert "McpHttpConfig" in text
        assert "1234" in text
        assert "maya-mcp" in text


class TestServerFactoryRouting:
    def test_uses_sidecar_backend_when_core_unavailable(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
            lambda: False,
        )
        options = _OptionsStub(
            dcc_name="maya",
            sidecar=_SidecarStub(
                host_rpc="commandport://127.0.0.1:6000",
                server_bin="dcc-mcp-server-test",
                extra_args=("--ppid-poll-ms", "50"),
            ),
        )
        config = PureMcpHttpConfig(port=8765, server_name="maya-mcp")
        server = create_adapter_server("maya", config, options)
        assert isinstance(server, SidecarBackedSkillServer)
        assert server.backend == "sidecar"
        assert server._server_bin == "dcc-mcp-server-test"
        assert server._extra_args == ("--ppid-poll-ms", "50")

    def test_uses_create_skill_server_when_core_available(self, monkeypatch: pytest.MonkeyPatch):
        import sys
        import types

        fake_server = MagicMock(name="embedded-server")
        fake_core = types.ModuleType("dcc_mcp_core._core")

        def _fake_create_skill_server(app_name, config):
            assert app_name == "maya"
            assert config is not None
            return fake_server

        fake_core.create_skill_server = _fake_create_skill_server
        monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
            lambda: True,
        )
        options = SimpleNamespace(sidecar=_SidecarStub(host_rpc="commandport://127.0.0.1:6000"))
        config = PureMcpHttpConfig()
        server = create_adapter_server("maya", config, options)
        assert server is fake_server


class TestSidecarBackedSkillServer:
    def test_discover_returns_real_skill_count_without_core(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        _write_skill(tmp_path, "maya-modeling", tags="[modeling, geometry]")
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        count = server.discover(extra_paths=[str(tmp_path)], accumulated=False)
        assert count == 1
        assert [skill.name for skill in server.list_skills()] == ["maya-modeling"]
        assert server.list_skills()[0].status == "discovered"
        assert server.list_skills()[0].loaded is False
        assert server.list_skills()[0].implicit_invocation is True
        assert [skill.name for skill in server.search_skills(query="geometry")] == ["maya-modeling"]
        assert server.get_skill("maya-modeling").name == "maya-modeling"
        with pytest.raises(RuntimeError, match="dispatch-only"):
            server.load_skill("maya-modeling")
        assert SkillQueryClient(server, "maya").load_skill("maya-modeling") is False

    def test_discover_builds_isolated_launch_environment(self, monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
        inherited = str(tmp_path / "inherited")
        explicit = str(tmp_path / "explicit")
        _write_skill(Path(explicit), "explicit-skill")
        monkeypatch.setenv(ENV_SKILL_PATHS, inherited)
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )

        count = server.discover(extra_paths=[explicit, explicit], accumulated=False)

        assert count == 1
        assert os.environ[ENV_SKILL_PATHS] == inherited
        launch_env = server._launch_environment()
        assert launch_env[ENV_SKILL_PATHS].split(os.pathsep) == [explicit, inherited]
        assert launch_env[ENV_DISABLE_ACCUMULATED_SKILLS] == "1"

    def test_discover_honors_app_global_and_accumulated_paths(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        app_root = tmp_path / "app"
        global_root = tmp_path / "global"
        accumulated_root = tmp_path / "accumulated"
        team_root = tmp_path / "team"
        _write_skill(app_root, "app-skill")
        _write_skill(global_root, "global-skill")
        _write_skill(accumulated_root, "accumulated-skill")
        _write_skill(team_root, "team-skill")
        monkeypatch.setenv("DCC_MCP_MAYA_SKILL_PATHS", str(app_root))
        monkeypatch.setenv(ENV_SKILL_PATHS, str(global_root))
        monkeypatch.setenv("DCC_MCP_USER_MAYA_SKILL_PATHS", str(accumulated_root))
        monkeypatch.setenv("DCC_MCP_TEAM_MAYA_SKILL_PATHS", str(team_root))
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )

        assert server.discover(accumulated=True) == 4
        assert {skill.name for skill in server.list_skills()} == {
            "accumulated-skill",
            "app-skill",
            "global-skill",
            "team-skill",
        }
        assert [skill.name for skill in server.search_skills(scope="user")] == ["accumulated-skill"]
        assert [skill.name for skill in server.search_skills(scope="team")] == ["team-skill"]
        assert server.discover(accumulated=False) == 2
        assert {skill.name for skill in server.list_skills()} == {"app-skill", "global-skill"}

    def test_discover_skips_invalid_frontmatter_and_folds_description(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        invalid = {
            "empty": "---\n---\n",
            "unclosed": "---\nname: unclosed\ndescription: missing close\n",
            "missing-name": "---\ndescription: no name\n---\n",
            "invalid-name": "---\nname: [oops\ndescription: invalid YAML scalar\n---\n",
            "legacy-top-level": "---\nname: legacy-flat\ndescription: invalid extension\ndcc: maya\n---\n",
            "no-frontmatter": "# Not a skill contract\n",
        }
        for directory, content in invalid.items():
            skill_dir = tmp_path / directory
            skill_dir.mkdir()
            (skill_dir / "SKILL.md").write_text(content, encoding="utf-8")
        valid = tmp_path / "valid-skill"
        valid.mkdir()
        (valid / "SKILL.md").write_text(
            "---\n"
            "name: valid-skill\n"
            "description: >-\n"
            "  Folded geometry creation\n"
            "  metadata for discovery.\n"
            "metadata:\n"
            "  dcc-mcp:\n"
            "    dcc: maya\n"
            '    version: "0.19.20"  # release version\n'
            '    tags: "geometry, discovery"\n'
            "    skill-reference-docs:\n"
            '      - "references/*.md"\n'
            "---\n",
            encoding="utf-8",
        )
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )

        assert server.discover([str(tmp_path)], accumulated=False) == 1
        skill = server.get_skill("valid-skill")
        assert skill.description == "Folded geometry creation metadata for discovery."
        assert skill.version == "0.19.20"
        assert skill.tags == ["geometry", "discovery"]
        assert [item.name for item in server.search_skills(query="geometry creation")] == ["valid-skill"]

    def test_discover_uses_canonical_defaults_when_dcc_metadata_is_absent(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        skill = tmp_path / "generic-skill"
        skill.mkdir()
        (skill / "SKILL.md").write_text(
            "---\nname: generic-skill\ndescription: Generic metadata skill\n---\n",
            encoding="utf-8",
        )
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="stub://localhost",
        )

        assert server.discover([str(tmp_path)], accumulated=False) == 1
        metadata = server.get_skill("generic-skill")
        assert metadata.dcc == "python"
        assert metadata.version == "1.0.0"
        assert [item.name for item in server.search_skills(dcc="python")] == ["generic-skill"]

    def test_custom_dcc_env_paths_use_canonical_underscore_slug(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        app_root = tmp_path / "custom-app"
        accumulated_root = tmp_path / "custom-user"
        _write_skill(app_root, "custom-app-skill", dcc="my-dcc")
        _write_skill(accumulated_root, "custom-user-skill", dcc="my-dcc")
        monkeypatch.setenv("DCC_MCP_MY_DCC_SKILL_PATHS", str(app_root))
        monkeypatch.setenv("DCC_MCP_USER_MY_DCC_SKILL_PATHS", str(accumulated_root))
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "my-dcc",
            PureMcpHttpConfig(),
            host_rpc="stub://localhost",
        )

        assert server.discover(accumulated=True) == 2
        assert {skill.name for skill in server.list_skills()} == {
            "custom-app-skill",
            "custom-user-skill",
        }

    def test_discovery_keeps_cross_dcc_infrastructure_metadata(
        self,
        monkeypatch: pytest.MonkeyPatch,
        tmp_path: Path,
    ):
        _write_skill(tmp_path, "python-infrastructure", dcc="python", tags="[infrastructure]")
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="stub://localhost",
        )

        assert server.discover([str(tmp_path)], accumulated=False) == 1
        assert [skill.name for skill in server.list_skills()] == ["python-infrastructure"]
        assert [skill.name for skill in server.search_skills(dcc="python")] == ["python-infrastructure"]
        assert server.search_skills(dcc="maya") == []

    def test_real_repo_skill_preserves_quoted_version_with_comment(
        self,
        monkeypatch: pytest.MonkeyPatch,
    ):
        monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="stub://localhost",
        )

        assert server.discover([str(_REPO_ROOT / "skills")], accumulated=False) >= 1
        skill_file = (_REPO_ROOT / "skills" / "dcc-cli-gateway" / "SKILL.md").read_text(encoding="utf-8")
        version_line = next(line for line in skill_file.splitlines() if line.strip().startswith("version:"))
        assert server.get_skill("dcc-cli-gateway").version == version_line.split('"')[1]

    def test_start_launches_sidecar_and_returns_handle(self, monkeypatch: pytest.MonkeyPatch):
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(port=8765),
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

        handle = server.start()
        assert handle.port == 9876
        assert handle.mcp_url() == "http://127.0.0.1:9876/mcp"

    def test_start_requires_host_rpc(self):
        server = SidecarBackedSkillServer("maya", PureMcpHttpConfig(), host_rpc="")
        with pytest.raises(RuntimeError, match="host_rpc"):
            server.start()

    def test_start_forwards_server_bin_and_extra_args(self, monkeypatch: pytest.MonkeyPatch):
        captured: dict = {"command": None, "launch": None}

        def _fake_build(**kwargs):
            captured["command"] = kwargs
            return {"success": True}

        def _fake_launch(**kwargs):
            captured["launch"] = kwargs
            return {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": MagicMock(),
            }

        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.build_sidecar_command",
            _fake_build,
        )
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            _fake_launch,
        )
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(port=8765),
            host_rpc="commandport://127.0.0.1:6000",
            server_bin="dcc-mcp-server-test",
            extra_args=("--ppid-poll-ms", "50"),
        )
        server.discover(extra_paths=["/tmp/skills"], accumulated=False)
        server.start()
        command = captured["command"]
        launch = captured["launch"]
        assert command["env"] == launch["env"]
        assert launch["env"][ENV_DISABLE_ACCUMULATED_SKILLS] == "1"
        assert "/tmp/skills" in launch["env"][ENV_SKILL_PATHS]
        assert launch["server_bin"] == "dcc-mcp-server-test"
        assert launch["extra_args"] == ("--ppid-poll-ms", "50")


def test_public_lite_factory_preserves_discovery_contract(monkeypatch: pytest.MonkeyPatch, tmp_path: Path):
    import dcc_mcp_core.server_base as server_base

    explicit_root = tmp_path / "skills"
    _write_skill(explicit_root, "public-factory-skill", tags="[factory, smoke]")
    explicit = str(explicit_root)
    monkeypatch.delenv(ENV_SKILL_PATHS, raising=False)
    monkeypatch.delenv("DCC_MCP_MAYA_SKILL_PATHS", raising=False)
    monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
    monkeypatch.setattr(server_base, "_core", None)
    monkeypatch.setattr(server_base, "is_core_extension_available", lambda: False)
    monkeypatch.setattr(
        "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
        lambda: False,
    )

    server = server_base.create_skill_server("maya", extra_paths=[explicit], accumulated=False)

    assert isinstance(server, SidecarBackedSkillServer)
    assert server._pending_skill_paths == [explicit]
    assert server._accumulated is False
    assert ENV_SKILL_PATHS not in os.environ
    assert [skill.name for skill in server.list_skills()] == ["public-factory-skill"]
    assert [skill.name for skill in server.search_skills(query="factory")] == ["public-factory-skill"]


def test_public_factory_rejects_unknown_keywords() -> None:
    import dcc_mcp_core.server_base as server_base

    with pytest.raises(TypeError, match="unexpected keyword argument"):
        server_base.create_skill_server("maya", unsupported=True)


def test_public_factory_preserves_app_env_paths_with_dcc_override(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    import dcc_mcp_core.server_base as server_base

    app_root = tmp_path / "app-server"
    _write_skill(app_root, "app-owned-maya-skill", dcc="maya")
    monkeypatch.setenv("DCC_MCP_APP_SERVER_SKILL_PATHS", str(app_root))
    monkeypatch.setenv(ENV_DISABLE_DEFAULT_SKILL_PATHS, "1")
    monkeypatch.setattr(server_base, "_core", None)
    monkeypatch.setattr(server_base, "is_core_extension_available", lambda: False)
    monkeypatch.setattr(
        "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
        lambda: False,
    )

    server = server_base.create_skill_server("app-server", dcc_name="maya", accumulated=False)

    assert [skill.name for skill in server.list_skills()] == ["app-owned-maya-skill"]


def test_default_skill_path_flag_is_available_without_core() -> None:
    import dcc_mcp_core

    assert dcc_mcp_core.ENV_DISABLE_ACCUMULATED_SKILLS == "DCC_MCP_DISABLE_ACCUMULATED_SKILLS"
    assert dcc_mcp_core.ENV_DISABLE_DEFAULT_SKILL_PATHS == "DCC_MCP_DISABLE_DEFAULT_SKILL_PATHS"
    assert dcc_mcp_core.ENV_SKILL_PATHS == "DCC_MCP_SKILL_PATHS"
    assert dcc_mcp_core.ENV_TEAM_SKILL_PATHS == "DCC_MCP_TEAM_SKILL_PATHS"
    assert dcc_mcp_core.ENV_USER_SKILL_PATHS == "DCC_MCP_USER_SKILL_PATHS"


class TestExecutionBridgeLazyCore:
    def test_sandbox_context_skips_core_when_unavailable(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setattr(
            "dcc_mcp_core._server.execution_bridge.is_core_extension_available",
            lambda: False,
        )
        from dcc_mcp_core._server.execution_bridge import _sandbox_context

        assert _sandbox_context(MagicMock()) is None


@pytest.mark.skipif(is_core_extension_available(), reason="only meaningful on py37-lite profile")
class TestServerBaseImportLight:
    def test_server_base_imports_fallbacks_without_core(self, monkeypatch: pytest.MonkeyPatch):
        import importlib

        module = importlib.import_module("dcc_mcp_core.server_base")

        assert "create_skill_server" in module.__dict__
        assert "create_adapter_server" in module.__dict__
        assert module._PKG_VERSION == "0.0.0-dev"


@pytest.mark.skipif(is_core_extension_available(), reason="only meaningful on py37-lite profile")
def test_resolve_mcp_http_config_class_uses_pure_python():
    cls = resolve_mcp_http_config_class()
    assert cls is PureMcpHttpConfig
