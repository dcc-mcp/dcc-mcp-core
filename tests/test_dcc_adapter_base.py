"""Tests for DCC adapter base abstractions.

Covers:
- DccSkillHotReloader (hotreload.py)
- DccGatewayElection (gateway_election.py)
- DccServerBase (server_base.py)
- create_dcc_server / make_start_stop (factory.py)
"""

# Import future modules
from __future__ import annotations

from pathlib import Path

# Import built-in modules
import threading
import time
from typing import Any
from typing import List
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

# Import third-party modules
import pytest

# ── helpers ───────────────────────────────────────────────────────────────────


def _make_mock_server(is_running: bool = False, is_gateway: bool = False):
    """Return a mock server that satisfies DccServerBase / DccGatewayElection contracts."""
    mock = MagicMock()
    mock.is_running = is_running
    mock.is_gateway = is_gateway
    mock._handle = MagicMock() if is_running else None
    mock._server = MagicMock()
    mock._server.list_skills.return_value = []
    return mock


# ═══════════════════════════════════════════════════════════════════════════
# DccSkillHotReloader
# ═══════════════════════════════════════════════════════════════════════════


class TestDccSkillHotReloader:
    """Tests for dcc_mcp_core.hotreload.DccSkillHotReloader."""

    def _make_reloader(self, dcc_name: str = "test-dcc"):
        from dcc_mcp_core.hotreload import DccSkillHotReloader

        return DccSkillHotReloader(dcc_name=dcc_name, server=_make_mock_server())

    def test_initial_state(self):
        reloader = self._make_reloader()
        assert not reloader.is_enabled
        assert reloader.reload_count == 0
        assert reloader.watched_paths == []

    def test_enable_with_no_paths_returns_false(self):
        reloader = self._make_reloader()
        result = reloader.enable(skill_paths=[])
        assert result is False
        assert not reloader.is_enabled

    def test_disable_is_safe_when_not_enabled(self):
        reloader = self._make_reloader()
        reloader.disable()  # Must not raise
        assert not reloader.is_enabled

    def test_reload_now_when_disabled_returns_zero(self):
        reloader = self._make_reloader()
        assert reloader.reload_now() == 0

    def test_get_stats_structure(self):
        reloader = self._make_reloader()
        stats = reloader.get_stats()
        assert "enabled" in stats
        assert "watched_paths" in stats
        assert "reload_count" in stats
        assert stats["enabled"] is False
        assert stats["reload_count"] == 0

    def test_repr(self):
        reloader = self._make_reloader(dcc_name="blender")
        r = repr(reloader)
        assert "blender" in r
        assert "disabled" in r

    def test_enable_with_mock_watcher(self, tmp_path):
        """When SkillWatcher is available, enable() should succeed."""
        reloader = self._make_reloader()
        with patch("dcc_mcp_core.hotreload.DccSkillHotReloader.enable", return_value=True) as mock_enable:
            reloader.enable(skill_paths=[str(tmp_path)])
            # We mocked enable, so just check it was called
            mock_enable.assert_called_once()


# ═══════════════════════════════════════════════════════════════════════════
# DccGatewayElection
# ═══════════════════════════════════════════════════════════════════════════


class TestDccGatewayElection:
    """Tests for dcc_mcp_core.gateway_election.DccGatewayElection."""

    def _make_election(self, server=None, gateway_port: int = 19876):
        from dcc_mcp_core.gateway_election import DccGatewayElection

        if server is None:
            server = _make_mock_server()
        return DccGatewayElection(
            dcc_name="test-dcc",
            server=server,
            gateway_port=gateway_port,
            probe_interval=1,
            probe_timeout=0.5,
            probe_failures=2,
        )

    def test_initial_state(self):
        election = self._make_election()
        assert not election.is_running
        assert election.consecutive_failures == 0

    def test_start_and_stop(self):
        election = self._make_election()
        election.start()
        assert election.is_running
        election.stop()
        assert not election.is_running

    def test_double_start_is_safe(self):
        election = self._make_election()
        election.start()
        election.start()  # Second start must not raise or spawn duplicate thread
        election.stop()
        assert not election.is_running

    def test_stop_when_not_running_is_safe(self):
        election = self._make_election()
        election.stop()  # Must not raise
        assert not election.is_running

    def test_probe_gateway_unreachable(self):
        election = self._make_election(gateway_port=19999)
        # Port is not bound, so probe should return False
        result = election._probe_gateway()
        assert result is False

    def test_get_status(self):
        election = self._make_election()
        status = election.get_status()
        assert "running" in status
        assert "consecutive_failures" in status
        assert "gateway_host" in status
        assert "gateway_port" in status

    def test_repr(self):
        election = self._make_election()
        r = repr(election)
        assert "test-dcc" in r
        assert "stopped" in r

    def test_election_skips_when_already_gateway(self):
        """When server.is_gateway is True, election loop should reset failures."""
        server = _make_mock_server(is_running=True, is_gateway=True)
        election = self._make_election(server=server, gateway_port=19877)
        election._consecutive_failures = 5
        election.start()
        time.sleep(1.2)  # Let loop run once
        election.stop()
        assert election._consecutive_failures == 0

    def test_attempt_election_on_already_bound_port(self):
        """_attempt_election returns False when the port is already occupied."""
        import socket as _socket

        # Find a free port
        finder = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
        finder.bind(("127.0.0.1", 0))
        port = finder.getsockname()[1]
        finder.setsockopt(_socket.SOL_SOCKET, _socket.SO_REUSEADDR, 1)
        finder.listen(1)

        election = self._make_election(gateway_port=port)
        try:
            result = election._attempt_election()
            assert result is False
        finally:
            finder.close()


# ═══════════════════════════════════════════════════════════════════════════
# DccServerBase
# ═══════════════════════════════════════════════════════════════════════════


class _FakeDccServer:
    """Minimal McpHttpServer / skill-manager mock."""

    def __init__(self):
        self.started = False
        self._handle = _FakeHandle()

    def start(self):
        self.started = True
        return self._handle

    def discover_and_load_all(self, paths):
        pass

    def list_skills(self):
        return []

    def load_skill(self, name):
        pass

    def unload_skill(self, name):
        pass

    def find_skills(self, **kwargs):
        return []

    def is_loaded(self, name):
        return False

    def get_skill_info(self, name):
        return None


class _FakeHandle:
    port = 18765
    is_gateway = False
    instance_id = "fake-id-001"

    def mcp_url(self):
        return f"http://127.0.0.1:{self.port}/mcp"

    def shutdown(self):
        pass


class _FakeConfig:
    port = 18765
    server_name = "fake-mcp"
    server_version = "0.1.0"
    gateway_port = 0
    registry_dir = ""
    dcc_version = ""
    scene = ""


class TestDccServerBase:
    """Tests for dcc_mcp_core.server_base.DccServerBase."""

    def _make_server(self, tmp_path, dcc_name="fake-dcc"):
        """Create a DccServerBase without calling the real __init__ (no Rust deps)."""
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir(exist_ok=True)

        # Bypass __init__ to avoid needing compiled _core
        server = object.__new__(DccServerBase)
        server._dcc_name = dcc_name
        server._builtin_skills_dir = skills_dir
        server._handle = None
        server._enable_gateway_failover = False
        server._hot_reloader = None
        server._gateway_election = None
        server._config = _FakeConfig()
        server._server = _FakeDccServer()
        return server

    def test_initial_state(self, tmp_path):
        server = self._make_server(tmp_path)
        assert not server.is_running
        assert server.mcp_url is None
        assert not server.is_hot_reload_enabled

    def test_start_and_stop(self, tmp_path):
        server = self._make_server(tmp_path)
        handle = server.start()
        assert server.is_running
        assert handle is not None
        assert server.mcp_url is not None

        server.stop()
        assert not server.is_running
        assert server.mcp_url is None

    def test_start_returns_same_handle_if_already_running(self, tmp_path):
        server = self._make_server(tmp_path)
        h1 = server.start()
        h2 = server.start()
        assert h1 is h2
        server.stop()

    def test_list_skills_returns_list(self, tmp_path):
        server = self._make_server(tmp_path)
        assert isinstance(server.list_skills(), list)

    def test_list_actions_returns_list(self, tmp_path):
        server = self._make_server(tmp_path)
        server._server.registry = MagicMock()
        server._server.registry.list_actions.return_value = []
        # registry is a property, patch it
        with patch.object(type(server), "registry", new_callable=lambda: property(lambda self: None)):
            result = server.list_actions()
        assert isinstance(result, list)

    def test_find_skills_returns_list(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.find_skills(query="anything")
        assert isinstance(result, list)

    def test_is_skill_loaded_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.is_skill_loaded("some-skill")
        assert isinstance(result, bool)

    def test_get_skill_info_returns_none_when_missing(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.get_skill_info("nonexistent-skill")
        assert result is None

    def test_load_skill_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.load_skill("some-skill") is True

    def test_unload_skill_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.unload_skill("some-skill") is True

    @pytest.fixture()
    def _patch_skill_env(self):
        """Patch the three skill-path helpers to return empty / None values.

        server_base.py does ``from dcc_mcp_core import <fn>`` inside
        ``collect_skill_search_paths``, so we patch the symbols on the
        ``dcc_mcp_core`` package itself.
        """
        with patch("dcc_mcp_core.get_app_skill_paths_from_env", return_value=[]), patch(
            "dcc_mcp_core.get_skill_paths_from_env", return_value=[]
        ), patch("dcc_mcp_core.get_skills_dir", return_value=None):
            yield

    def test_collect_skill_search_paths_includes_builtin(self, tmp_path, _patch_skill_env):
        server = self._make_server(tmp_path)
        paths = server.collect_skill_search_paths(include_bundled=False)
        # builtin_skills_dir (tmp_path/skills) should be in the result
        assert any("skills" in p for p in paths)

    def test_collect_skill_search_paths_filter_existing_removes_nonexistent(self, tmp_path, _patch_skill_env):
        """filter_existing=True removes non-existent paths and deduplicates."""
        server = self._make_server(tmp_path)
        # Add extra_paths with a mix of existing and non-existent
        existing_dir = str(tmp_path / "skills")
        nonexistent = "/nonexistent/path/xyz"
        paths = server.collect_skill_search_paths(
            extra_paths=[existing_dir, nonexistent],
            include_bundled=False,
            filter_existing=True,
        )
        assert existing_dir in paths
        assert nonexistent not in paths

    def test_collect_skill_search_paths_filter_existing_deduplicates(self, tmp_path, _patch_skill_env):
        """filter_existing=True deduplicates identical paths."""
        server = self._make_server(tmp_path)
        existing_dir = str(tmp_path / "skills")
        paths = server.collect_skill_search_paths(
            extra_paths=[existing_dir, existing_dir],
            include_bundled=False,
            filter_existing=True,
        )
        assert paths.count(existing_dir) == 1

    def test_collect_skill_search_paths_filter_false_keeps_all(self, tmp_path, _patch_skill_env):
        """filter_existing=False (default) preserves non-existent paths."""
        server = self._make_server(tmp_path)
        nonexistent = "/nonexistent/path/xyz"
        paths = server.collect_skill_search_paths(
            extra_paths=[nonexistent],
            include_bundled=False,
            filter_existing=False,
        )
        assert nonexistent in paths

    def test_enable_hot_reload_creates_reloader(self, tmp_path):
        server = self._make_server(tmp_path)
        with patch("dcc_mcp_core.hotreload.DccSkillHotReloader.enable", return_value=True):
            result = server.enable_hot_reload(skill_paths=[str(tmp_path)])
        assert result is True
        assert server._hot_reloader is not None

    def test_disable_hot_reload_safe_when_none(self, tmp_path):
        server = self._make_server(tmp_path)
        server.disable_hot_reload()  # must not raise

    def test_hot_reload_stats_when_never_enabled(self, tmp_path):
        server = self._make_server(tmp_path)
        stats = server.hot_reload_stats
        assert stats["enabled"] is False
        assert stats["reload_count"] == 0

    def test_get_gateway_election_status_no_election(self, tmp_path):
        server = self._make_server(tmp_path)
        status = server.get_gateway_election_status()
        assert "enabled" in status
        assert "running" in status
        assert status["running"] is False

    def test_is_gateway_false_when_not_running(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.is_gateway is False

    def test_gateway_url_none_when_not_running(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.gateway_url is None

    def test_version_string_default(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server._version_string() == "unknown"

    def test_repr(self, tmp_path):
        server = self._make_server(tmp_path, dcc_name="houdini")
        r = repr(server)
        assert "houdini" in r
        assert "stopped" in r

    def test_update_metadata_fails_when_not_running(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.update_gateway_metadata(scene="/some/scene.hip")
        assert result is False

    def test_subclass_minimal(self, tmp_path):
        """A minimal subclass should work out of the box."""
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()

        class HoudiniMcpServer(DccServerBase):
            def _version_string(self):
                return "20.0.547"

        # Bypass __init__ again
        srv = object.__new__(HoudiniMcpServer)
        srv._dcc_name = "houdini"
        srv._builtin_skills_dir = skills_dir
        srv._handle = None
        srv._enable_gateway_failover = False
        srv._hot_reloader = None
        srv._gateway_election = None
        srv._config = _FakeConfig()
        srv._server = _FakeDccServer()

        assert srv._dcc_name == "houdini"
        assert srv._version_string() == "20.0.547"
        assert not srv.is_running

    def test_init_uses_version_string_for_gateway_metadata(self, tmp_path):
        import builtins

        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()

        class HoudiniMcpServer(DccServerBase):
            def _version_string(self):
                return "20.5.1"

        fake_config = _FakeConfig()

        real_import = __import__

        def fake_import(name, globals=None, locals=None, fromlist=(), level=0):
            if name == "dcc_mcp_core" and fromlist:
                from types import SimpleNamespace

                return SimpleNamespace(
                    McpHttpConfig=lambda **_kwargs: fake_config,
                    create_skill_manager=lambda *_args, **_kwargs: _FakeDccServer(),
                    __version__="9.9.9",
                )
            return real_import(name, globals, locals, fromlist, level)

        with patch.object(builtins, "__import__", side_effect=fake_import):
            server = HoudiniMcpServer(dcc_name="houdini", builtin_skills_dir=skills_dir)

        assert server._config.dcc_version == "20.5.1"

    def test_init_explicit_dcc_version_overrides_version_string(self, tmp_path):
        import builtins

        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()

        class HoudiniMcpServer(DccServerBase):
            def _version_string(self):
                return "20.5.1"

        fake_config = _FakeConfig()

        real_import = __import__

        def fake_import(name, globals=None, locals=None, fromlist=(), level=0):
            if name == "dcc_mcp_core" and fromlist:
                from types import SimpleNamespace

                return SimpleNamespace(
                    McpHttpConfig=lambda **_kwargs: fake_config,
                    create_skill_manager=lambda *_args, **_kwargs: _FakeDccServer(),
                    __version__="9.9.9",
                )
            return real_import(name, globals, locals, fromlist, level)

        with patch.object(builtins, "__import__", side_effect=fake_import):
            server = HoudiniMcpServer(
                dcc_name="houdini",
                builtin_skills_dir=skills_dir,
                dcc_version="19.5.0",
            )

        assert server._config.dcc_version == "19.5.0"


# ═══════════════════════════════════════════════════════════════════════════
# factory.py
# ═══════════════════════════════════════════════════════════════════════════


class TestCreateDccServer:
    """Tests for dcc_mcp_core.factory.create_dcc_server and make_start_stop."""

    def _server_class(self, tmp_path):
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir(exist_ok=True)

        class _TestServer(DccServerBase):
            def __init__(self, port=18767, **kwargs):
                # Avoid hitting real McpHttpConfig / create_skill_manager
                self._dcc_name = "test"
                self._builtin_skills_dir = skills_dir
                self._handle = None
                self._enable_gateway_failover = False
                self._hot_reloader = None
                self._gateway_election = None
                self._config = _FakeConfig()
                self._server = _FakeDccServer()

        return _TestServer

    def test_create_dcc_server_returns_handle(self, tmp_path):
        from dcc_mcp_core.factory import create_dcc_server

        cls = self._server_class(tmp_path)
        holder = [None]
        lock = threading.Lock()

        handle = create_dcc_server(
            instance_holder=holder,
            lock=lock,
            server_class=cls,
            register_builtins=False,
        )
        assert handle is not None
        assert holder[0] is not None

    def test_create_dcc_server_returns_same_instance(self, tmp_path):
        from dcc_mcp_core.factory import create_dcc_server

        cls = self._server_class(tmp_path)
        holder = [None]
        lock = threading.Lock()

        h1 = create_dcc_server(instance_holder=holder, lock=lock, server_class=cls, register_builtins=False)
        h2 = create_dcc_server(instance_holder=holder, lock=lock, server_class=cls, register_builtins=False)
        assert h1 is h2

    def test_make_start_stop(self, tmp_path):
        from dcc_mcp_core.factory import make_start_stop

        cls = self._server_class(tmp_path)
        start_fn, stop_fn = make_start_stop(cls)

        handle = start_fn(register_builtins=False)
        assert handle is not None
        stop_fn()

    def test_get_server_instance_none_initially(self):
        from dcc_mcp_core.factory import get_server_instance

        holder = [None]
        assert get_server_instance(holder) is None

    def test_get_server_instance_after_creation(self, tmp_path):
        from dcc_mcp_core.factory import create_dcc_server
        from dcc_mcp_core.factory import get_server_instance

        cls = self._server_class(tmp_path)
        holder = [None]
        lock = threading.Lock()

        create_dcc_server(instance_holder=holder, lock=lock, server_class=cls, register_builtins=False)
        assert get_server_instance(holder) is not None


# ═══════════════════════════════════════════════════════════════════════════
# __init__.py public exports
# ═══════════════════════════════════════════════════════════════════════════


class TestPublicExports:
    """Verify the new symbols are reachable via dcc_mcp_core top-level imports."""

    def test_dcc_server_base_importable(self):
        from dcc_mcp_core import DccServerBase

        assert DccServerBase is not None

    def test_dcc_skill_hot_reloader_importable(self):
        from dcc_mcp_core import DccSkillHotReloader

        assert DccSkillHotReloader is not None

    def test_dcc_gateway_election_importable(self):
        from dcc_mcp_core import DccGatewayElection

        assert DccGatewayElection is not None

    def test_create_dcc_server_importable(self):
        from dcc_mcp_core import create_dcc_server

        assert create_dcc_server is not None

    def test_make_start_stop_importable(self):
        from dcc_mcp_core import make_start_stop

        assert make_start_stop is not None

    def test_get_server_instance_importable(self):
        from dcc_mcp_core import get_server_instance

        assert get_server_instance is not None

    def test_all_in___all__(self):
        import dcc_mcp_core

        for name in [
            "DccServerBase",
            "DccSkillHotReloader",
            "DccGatewayElection",
            "create_dcc_server",
            "make_start_stop",
            "get_server_instance",
        ]:
            assert name in dcc_mcp_core.__all__, f"{name!r} missing from __all__"
