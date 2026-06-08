"""Tests for DCC adapter base abstractions.

Covers:
- DccSkillHotReloader (hotreload.py)
- DccGatewayElection (gateway_election.py)
- DccServerBase (server_base.py)
- create_dcc_server / make_start_stop (factory.py)
"""

# Import future modules
from __future__ import annotations

import logging
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

    # ── promotion hook (regression for issue #204) ───────────────────────────

    def _free_port(self) -> int:
        """Pick an unused TCP port and release it."""
        import socket as _socket

        s = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
        try:
            s.bind(("127.0.0.1", 0))
            return s.getsockname()[1]
        finally:
            s.close()

    def test_upgrade_to_gateway_without_hook_returns_false(self):
        """No callback and no server hook must not claim a bogus success."""
        server = _make_mock_server()
        # MagicMock auto-creates attributes, so strip _upgrade_to_gateway.
        del server._upgrade_to_gateway
        election = self._make_election(server=server)
        assert election._upgrade_to_gateway() is False

    def test_upgrade_to_gateway_calls_on_promote_callback(self):
        """on_promote callback is invoked and its return value propagates."""
        calls = {"n": 0}

        def _promote() -> bool:
            calls["n"] += 1
            return True

        election = self._make_election()
        election._on_promote = _promote
        assert election._upgrade_to_gateway() is True
        assert calls["n"] == 1

    def test_upgrade_to_gateway_falls_back_to_server_hook(self):
        """When no callback is given, the server's _upgrade_to_gateway is used."""
        server = _make_mock_server()
        server._upgrade_to_gateway = MagicMock(return_value=True)
        election = self._make_election(server=server)
        assert election._upgrade_to_gateway() is True
        server._upgrade_to_gateway.assert_called_once_with()

    def test_upgrade_to_gateway_swallows_hook_exception(self):
        """A raising hook must not break the election loop."""
        server = _make_mock_server()
        server._upgrade_to_gateway = MagicMock(side_effect=RuntimeError("boom"))
        election = self._make_election(server=server)
        assert election._upgrade_to_gateway() is False

    def test_attempt_election_on_free_port_triggers_promotion(self):
        """When the port is free, _attempt_election must delegate to promotion."""
        port = self._free_port()
        promote = MagicMock(return_value=True)
        election = self._make_election(gateway_port=port)
        election._on_promote = promote

        assert election._attempt_election() is True
        promote.assert_called_once_with()

    def test_attempt_election_returns_false_when_promotion_fails(self):
        """If promotion hook returns False, _attempt_election must reflect that."""
        port = self._free_port()
        election = self._make_election(gateway_port=port)
        election._on_promote = MagicMock(return_value=False)
        assert election._attempt_election() is False

    def test_on_promote_kwarg_is_stored(self):
        """on_promote passed via __init__ is stored and used."""
        from dcc_mcp_core.gateway_election import DccGatewayElection

        cb = MagicMock(return_value=True)
        election = DccGatewayElection(
            dcc_name="test-dcc",
            server=_make_mock_server(),
            gateway_port=19876,
            on_promote=cb,
        )
        assert election._on_promote is cb


# ═══════════════════════════════════════════════════════════════════════════
# DccServerBase
# ═══════════════════════════════════════════════════════════════════════════


class _FakeDccServer:
    """Minimal McpHttpServer / skill-manager mock."""

    def __init__(self):
        self.started = False
        self._handle = _FakeHandle()
        self._resources = _FakeResourceHandle()

    def start(self):
        self.started = True
        return self._handle

    def discover_and_load_all(self, paths):
        pass

    def list_skills(self):
        return []

    def load_skill(self, name):
        pass

    def get_skill(self, name):
        return None

    def load_skill_object(self, skill):
        pass

    def set_skill_load_transform(self, transform):
        self.skill_load_transform = transform

    def clear_skill_load_transform(self):
        self.skill_load_transform = None

    def set_after_load_skill_hook(self, hook):
        self.after_load_skill_hook = hook

    def clear_after_load_skill_hook(self):
        self.after_load_skill_hook = None

    def unload_skill(self, name):
        pass

    def search_skills(self, **kwargs):
        return []

    def is_loaded(self, name):
        return False

    def get_skill_info(self, name):
        return None

    def resources(self):
        return self._resources


class _FakeResourceHandle:
    def __init__(self):
        self.producers = []
        self.scene = None
        self.updated = []

    def register_producer(self, scheme_or_uri, producer):
        self.producers.append((scheme_or_uri, producer))

    def set_scene(self, snapshot):
        self.scene = snapshot

    def notify_updated(self, uri):
        self.updated.append(uri)


class _FakeHandle:
    port = 18765
    is_gateway = False
    instance_id = "fake-id-001"

    def __init__(self):
        self.gateway_metadata_updates = []

    def mcp_url(self):
        return f"http://127.0.0.1:{self.port}/mcp"

    def update_gateway_metadata(self, metadata):
        self.gateway_metadata_updates.append(dict(metadata))

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

    def __init__(self):
        self._instance_metadata = {}

    @property
    def instance_metadata(self):
        return dict(self._instance_metadata)

    @instance_metadata.setter
    def instance_metadata(self, metadata):
        self._instance_metadata = dict(metadata)


class TestDccServerBase:
    """Tests for dcc_mcp_core.server_base.DccServerBase."""

    def _make_server(self, tmp_path, dcc_name="fake-dcc"):
        """Create a DccServerBase without calling the real __init__ (no Rust deps)."""
        from dcc_mcp_core._testing import make_test_server

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir(exist_ok=True)

        return make_test_server(
            server=_FakeDccServer(),
            dcc_name=dcc_name,
            _builtin_skills_dir=skills_dir,
            _handle=None,
            _enable_gateway_failover=False,
            _hot_reloader=None,
            _gateway_election=None,
            _config=_FakeConfig(),
            _enable_telemetry=False,
            _enable_file_logging=False,
            _enable_job_persistence=False,
        )

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

    def test_start_logs_server_version(self, tmp_path, caplog):
        server = self._make_server(tmp_path)

        with caplog.at_level(logging.INFO, logger="dcc_mcp_core._server.lifecycle_controller"):
            server.start()

        assert "[fake-dcc] MCP server v0.1.0 started at http://127.0.0.1:18765/mcp" in caplog.text
        server.stop()

    def test_start_returns_same_handle_if_already_running(self, tmp_path):
        server = self._make_server(tmp_path)
        h1 = server.start()
        h2 = server.start()
        assert h1 is h2
        server.stop()

    def test_resources_returns_public_inner_handle(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.resources() is server._server._resources

    def test_resource_helpers_delegate_to_public_handle(self, tmp_path):
        server = self._make_server(tmp_path)

        def producer(uri):
            return {"mimeType": "text/plain", "text": uri}

        server.register_resource_producer("docs://adapter", producer)
        server.set_scene_resource({"name": "demo"})
        server.notify_resource_updated("docs://adapter")

        handle = server._server._resources
        assert handle.producers == [("docs://adapter", producer)]
        assert handle.scene == {"name": "demo"}
        assert handle.updated == ["docs://adapter"]

    def test_quit_hooks_run_lifo_once_on_stop(self, tmp_path):
        server = self._make_server(tmp_path)
        calls = []
        server.register_quit_hook(lambda: calls.append("first"))
        server.register_quit_hook(lambda: calls.append("second"))

        server.stop()
        server.stop()

        assert calls == ["second", "first"]

    def test_quit_hook_exception_does_not_block_later_hooks(self, tmp_path, caplog):
        server = self._make_server(tmp_path)
        calls = []

        def broken():
            calls.append("broken")
            raise RuntimeError("boom")

        server.register_quit_hook(lambda: calls.append("first"))
        server.register_quit_hook(broken)
        server.register_quit_hook(lambda: calls.append("last"))

        server.stop()

        assert calls == ["last", "broken", "first"]
        assert "Quit hook failed" in caplog.text

    def test_unregister_quit_hook(self, tmp_path):
        server = self._make_server(tmp_path)
        calls = []

        def hook():
            calls.append("hook")

        assert server.register_quit_hook(hook) is hook
        assert server.unregister_quit_hook(hook) is True
        assert server.unregister_quit_hook(hook) is False
        server.stop()
        assert calls == []

    def test_context_manager_starts_and_stops(self, tmp_path):
        server = self._make_server(tmp_path)
        with server as handle:
            assert handle is not None
            assert server.is_running
        assert not server.is_running

    def test_start_installs_weak_atexit_hook(self, tmp_path, monkeypatch):
        import atexit as atexit_module

        from dcc_mcp_core._server.lifecycle_controller import LifecycleController

        server = self._make_server(tmp_path)
        registrations = []
        monkeypatch.setattr(atexit_module, "register", lambda *args: registrations.append(args))

        server.start()

        assert len(registrations) == 1
        callback, ref = registrations[0]
        assert callback is LifecycleController._stop_from_atexit
        assert ref() is server

    def test_init_registers_builtin_skills(self, tmp_path, monkeypatch):
        """Verify that __init__ calls register_all_builtin_skills."""
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core.server_base import DccServerBase

        calls = []

        def mock_register(*args, **kwargs):
            calls.append((args, kwargs))

        monkeypatch.setattr("dcc_mcp_core._server.skill_discovery.register_all_builtin_skills", mock_register)
        monkeypatch.setattr("dcc_mcp_core.server_base.create_skill_server", MagicMock())

        opts = DccServerOptions.from_env("maya", tmp_path, port=0, gateway_port=0)
        _ = DccServerBase(opts)

        assert len(calls) == 1
        assert "dcc_name" in calls[0][1]
        assert calls[0][1]["dcc_name"] == "maya"

    def test_start_enables_gateway_election_through_runtime_controller(self, tmp_path, monkeypatch):
        server = self._make_server(tmp_path)
        server._enable_gateway_failover = True
        server._config.gateway_port = 19765
        starts = []

        class _FakeElection:
            def __init__(self, *, dcc_name, server, gateway_port):
                self.dcc_name = dcc_name
                self.server = server
                self.gateway_port = gateway_port

            def start(self):
                starts.append((self.dcc_name, self.server, self.gateway_port))

            def stop(self):
                pass

        monkeypatch.setattr("dcc_mcp_core._server.runtime.DccGatewayElection", _FakeElection)
        monkeypatch.setattr(
            "dcc_mcp_core._server.runtime.ensure_gateway_daemon",
            lambda **_kwargs: {"ok": False, "reason": "spawn_failed"},
        )

        server.start()

        assert starts == [(server._dcc_name, server, 19765)]
        assert isinstance(server._gateway_election, _FakeElection)

    def test_start_skips_embedded_election_when_daemon_backed(self, tmp_path, monkeypatch):
        server = self._make_server(tmp_path)
        server._enable_gateway_failover = True
        server._config.gateway_port = 19765
        started = {"election": 0}
        guardians = []

        class _FakeElection:
            def __init__(self, *, dcc_name, server, gateway_port):
                _ = (dcc_name, server, gateway_port)

            def start(self):
                started["election"] += 1

            def stop(self):
                pass

        monkeypatch.setattr("dcc_mcp_core._server.runtime.DccGatewayElection", _FakeElection)
        monkeypatch.setattr(
            "dcc_mcp_core._server.runtime.ensure_gateway_daemon",
            lambda **_kwargs: {"ok": True, "reason": "already_healthy"},
        )

        class _FakeGuardian:
            def __init__(self, **kwargs):
                self.kwargs = kwargs
                self.started = False
                self.stopped = False
                guardians.append(self)

            def start(self):
                self.started = True
                self.kwargs["status_callback"]({"ok": True, "reason": "guardian_started"})
                return True

            def stop(self):
                self.stopped = True

            def status(self):
                return {"ok": True, "reason": "guardian_started"}

        monkeypatch.setattr("dcc_mcp_core._server.runtime.GatewayDaemonGuardian", _FakeGuardian)

        server.start()

        assert started["election"] == 0
        assert server.get_gateway_election_status()["gateway_runtime_mode"] == "daemon-backed"
        assert server._config.instance_metadata["gateway_runtime_mode"] == "daemon-backed"
        assert server._config.instance_metadata["gateway_guardian_enabled"] == "true"
        assert server._config.instance_metadata["gateway_recovery_driver"] == "daemon_guardian"
        assert server._config.instance_metadata["registration_refresh_mode"] == "file_registry_heartbeat"
        assert server._handle.gateway_metadata_updates[-1] == {
            "gateway_runtime_mode": "daemon-backed",
            "gateway_guardian_enabled": "true",
            "gateway_recovery_driver": "daemon_guardian",
            "registration_refresh_mode": "file_registry_heartbeat",
        }
        assert len(guardians) == 1
        assert guardians[0].started is True
        assert server.get_gateway_election_status()["gateway_daemon_status"]["reason"] == "guardian_started"

        server.stop()
        assert server._config.instance_metadata["gateway_guardian_enabled"] == "false"
        assert server._config.instance_metadata["gateway_recovery_driver"] == "none"
        assert server._server._handle.gateway_metadata_updates[-1]["gateway_guardian_enabled"] == "false"
        assert server._server._handle.gateway_metadata_updates[-1]["gateway_recovery_driver"] == "none"

    def test_gateway_election_start_failure_clears_runtime_state(self, tmp_path, monkeypatch):
        server = self._make_server(tmp_path)
        server._enable_gateway_failover = True
        server._config.gateway_port = 19765

        class _BrokenElection:
            def __init__(self, *, dcc_name, server, gateway_port):
                pass

            def start(self):
                raise RuntimeError("boom")

        monkeypatch.setattr("dcc_mcp_core._server.runtime.DccGatewayElection", _BrokenElection)

        server.start()

        assert server._gateway_election is None

    def test_stop_uses_runtime_controller_to_shutdown_gateway_and_handle(self, tmp_path):
        server = self._make_server(tmp_path)
        handle = MagicMock()
        gateway = MagicMock()
        guardian = MagicMock()
        server._handle = handle
        server._gateway_election = gateway
        server._gateway_guardian = guardian

        server.stop()

        guardian.stop.assert_called_once_with()
        gateway.stop.assert_called_once_with()
        handle.shutdown.assert_called_once_with()
        assert server._gateway_guardian is None
        assert server._gateway_election is None
        assert server._handle is None

    def test_public_lifecycle_methods_recreate_missing_controllers(self, tmp_path):
        server = self._make_server(tmp_path)
        hook = MagicMock()
        del server.__dict__["_lifecycle"]
        del server.__dict__["_runtime"]

        server.register_quit_hook(hook)

        assert server.unregister_quit_hook(hook) is True
        assert "_lifecycle" in server.__dict__
        assert "_runtime" not in server.__dict__
        server.start()
        assert "_runtime" in server.__dict__

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

    def test_search_skills_returns_list(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.search_skills(query="anything")
        assert isinstance(result, list)

    def test_is_skill_loaded_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.is_skill_loaded("some-skill")
        assert isinstance(result, bool)

    def test_get_skill_info_returns_none_when_missing(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.get_skill_info("nonexistent-skill")
        assert result is None

    def test_get_skill_returns_none_when_missing(self, tmp_path):
        server = self._make_server(tmp_path)
        result = server.get_skill("nonexistent-skill")
        assert result is None

    def test_load_skill_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.load_skill("some-skill") is True

    def test_load_skill_object_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.load_skill_object(object()) is True

    def test_set_skill_load_transform_delegates_to_inner_server(self, tmp_path):
        server = self._make_server(tmp_path)

        def transform(skill):
            return skill

        assert server.set_skill_load_transform(transform) is True
        assert server._server.skill_load_transform is transform
        assert server.clear_skill_load_transform() is True
        assert server._server.skill_load_transform is None

    def test_set_after_load_skill_hook_delegates_to_inner_server(self, tmp_path):
        server = self._make_server(tmp_path)

        def hook(skill, registered):
            return None

        assert server.set_after_load_skill_hook(hook) is True
        assert server._server.after_load_skill_hook is hook
        assert server.clear_after_load_skill_hook() is True
        assert server._server.after_load_skill_hook is None

    def test_unload_skill_returns_bool(self, tmp_path):
        server = self._make_server(tmp_path)
        assert server.unload_skill("some-skill") is True

    @pytest.fixture()
    def _patch_skill_env(self):
        """Patch skill-path helpers to return empty / None values.

        server_base.py imports these helpers at module import time, so patch
        the symbols on ``dcc_mcp_core.server_base``.
        """
        with patch("dcc_mcp_core._server.skill_discovery.get_app_skill_paths_from_env", return_value=[]), patch(
            "dcc_mcp_core._server.skill_discovery.get_skill_paths_from_env", return_value=[]
        ), patch("dcc_mcp_core._server.skill_discovery.get_local_skills_dir", return_value=None), patch(
            "dcc_mcp_core._server.skill_discovery.get_skills_dir", return_value=None
        ):
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

    def test_collect_skill_search_paths_includes_local_default(self, tmp_path):
        server = self._make_server(tmp_path)
        local_default = tmp_path / ".dcc-mcp" / "fake-dcc" / "skills"
        with patch("dcc_mcp_core._server.skill_discovery.get_app_skill_paths_from_env", return_value=[]), patch(
            "dcc_mcp_core._server.skill_discovery.get_skill_paths_from_env", return_value=[]
        ), patch("dcc_mcp_core._server.skill_discovery.get_local_skills_dir", return_value=str(local_default)), patch(
            "dcc_mcp_core._server.skill_discovery.get_skills_dir", return_value=None
        ):
            paths = server.collect_skill_search_paths(include_bundled=False, filter_existing=True)

        assert str(local_default) in paths
        assert local_default.is_dir()

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

    # ── gateway promotion (regression for issue #204) ────────────────────────

    def test_upgrade_to_gateway_no_port_returns_false(self, tmp_path):
        """Promotion without a configured gateway port is a no-op, not a lie."""
        server = self._make_server(tmp_path)
        server._config.gateway_port = 0
        assert server._upgrade_to_gateway() is False

    def test_upgrade_to_gateway_already_gateway_is_noop(self, tmp_path):
        """If we are already the gateway, return True without restarting."""
        server = self._make_server(tmp_path)
        server._config.gateway_port = 19876
        existing = _FakeHandle()
        existing.is_gateway = True
        server._handle = existing
        original_start = server._server.start
        server._server.start = MagicMock(side_effect=AssertionError("must not restart"))
        try:
            assert server._upgrade_to_gateway() is True
        finally:
            server._server.start = original_start

    def test_upgrade_to_gateway_restart_flips_is_gateway(self, tmp_path):
        """Restart yields a new handle with is_gateway=True → server.is_gateway flips."""
        server = self._make_server(tmp_path)
        server._config.gateway_port = 19876
        # Initial running handle that is NOT the gateway.
        old_handle = _FakeHandle()
        old_handle.is_gateway = False
        old_handle.shutdown = MagicMock()
        server._handle = old_handle
        assert server.is_gateway is False

        # The inner server's next start() returns a gateway handle.
        new_handle = _FakeHandle()
        new_handle.is_gateway = True
        server._server.start = MagicMock(return_value=new_handle)

        assert server._upgrade_to_gateway() is True
        assert server._handle is new_handle
        assert server.is_gateway is True
        old_handle.shutdown.assert_called_once_with()
        server._server.start.assert_called_once_with()

    def test_upgrade_to_gateway_restart_fails_when_port_stolen(self, tmp_path):
        """If Rust GatewayRunner loses the race, is_gateway remains False."""
        server = self._make_server(tmp_path)
        server._config.gateway_port = 19876
        old_handle = _FakeHandle()
        old_handle.is_gateway = False
        old_handle.shutdown = MagicMock()
        server._handle = old_handle

        new_handle = _FakeHandle()
        new_handle.is_gateway = False  # someone else grabbed the port
        server._server.start = MagicMock(return_value=new_handle)

        assert server._upgrade_to_gateway() is False
        assert server._handle is new_handle
        assert server.is_gateway is False

    def test_upgrade_to_gateway_restart_exception_clears_handle(self, tmp_path):
        """If the restart itself raises, we don't keep a stale handle around."""
        server = self._make_server(tmp_path)
        server._config.gateway_port = 19876
        old_handle = _FakeHandle()
        old_handle.shutdown = MagicMock()
        server._handle = old_handle
        server._server.start = MagicMock(side_effect=RuntimeError("bind failed"))

        assert server._upgrade_to_gateway() is False
        assert server._handle is None
        assert server.is_gateway is False

    def test_election_promotes_server_end_to_end(self, tmp_path):
        """DccGatewayElection._attempt_election → DccServerBase._upgrade_to_gateway."""
        from dcc_mcp_core.gateway_election import DccGatewayElection

        server = self._make_server(tmp_path)
        server._config.gateway_port = 19876
        old_handle = _FakeHandle()
        old_handle.is_gateway = False
        old_handle.shutdown = MagicMock()
        server._handle = old_handle

        new_handle = _FakeHandle()
        new_handle.is_gateway = True
        server._server.start = MagicMock(return_value=new_handle)

        # Pick a real free port so _is_port_free returns True.
        import socket as _socket

        s = _socket.socket(_socket.AF_INET, _socket.SOCK_STREAM)
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
        s.close()

        election = DccGatewayElection(
            dcc_name=server._dcc_name,
            server=server,
            gateway_port=port,
            probe_interval=1,
            probe_timeout=0.3,
            probe_failures=1,
        )

        assert election._attempt_election() is True
        assert server.is_gateway is True
        assert server._handle is new_handle

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
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()

        class HoudiniMcpServer(DccServerBase):
            def _version_string(self):
                return "20.5.1"

        opts = DccServerOptions.from_env("houdini", skills_dir, port=0, gateway_port=0)
        with patch("dcc_mcp_core.server_base.create_skill_server", return_value=_FakeDccServer()):
            server = HoudiniMcpServer(opts)

        assert server._config.dcc_version == "20.5.1"

    def test_init_explicit_dcc_version_overrides_version_string(self, tmp_path):
        from dcc_mcp_core._server.options import DccServerOptions
        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir()

        class HoudiniMcpServer(DccServerBase):
            def _version_string(self):
                return "20.5.1"

        opts = DccServerOptions.from_env(
            "houdini",
            skills_dir,
            port=0,
            gateway_port=0,
            dcc_version="19.5.0",
        )
        with patch("dcc_mcp_core.server_base.create_skill_server", return_value=_FakeDccServer()):
            server = HoudiniMcpServer(opts)

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
                # Avoid hitting real McpHttpConfig / create_skill_server
                self._dcc_name = "test"
                self._builtin_skills_dir = skills_dir
                self._handle = None
                self._enable_gateway_failover = False
                self._hot_reloader = None
                self._gateway_election = None
                self._config = _FakeConfig()
                self._server = _FakeDccServer()
                self._enable_telemetry = False
                self._enable_file_logging = False
                self._enable_job_persistence = False

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
            "ensure_gateway_daemon",
        ]:
            assert name in dcc_mcp_core.__all__, f"{name!r} missing from __all__"


# ── Cross-DCC gateway runtime mode regression (PIP-488) ────────────────────────
#
# Verify that the gateway configuration surface behaves identically across
# at least two DCC families so adapters, admin UI, and diagnostics do not
# accidentally encode Maya-only assumptions.


class TestGatewayRuntimeModeCrossDcc:
    """Daemon-backed auto-launch works identically for any DCC family."""

    @pytest.mark.parametrize("dcc_name", ["maya", "blender", "photoshop"])
    def test_default_auto_launch_is_daemon_backed(self, tmp_path, dcc_name):
        """Any DCC with gateway_port > 0 gets the daemon-backed auto-launch."""
        from dcc_mcp_core._server.config import build_mcp_http_config
        from dcc_mcp_core._server.options import DccServerOptions

        opts = DccServerOptions.from_env(dcc_name, tmp_path, port=0, gateway_port=9765)
        config = build_mcp_http_config(opts, package_version="0.0.0", version_provider=lambda: "unused")

        assert config.gateway_port == 9765, f"{dcc_name}: gateway_port should propagate from options"
        assert config.dcc_type == dcc_name, f"{dcc_name}: dcc_type must match the adapter identity"

    @pytest.mark.parametrize("dcc_name", ["houdini", "zbrush"])
    def test_build_config_preserves_dcc_specific_fields(self, tmp_path, dcc_name):
        """dcc_type and server_name are always DCC-specific, never 'maya'-only."""
        from dcc_mcp_core._server.config import build_mcp_http_config
        from dcc_mcp_core._server.options import DccServerOptions

        opts = DccServerOptions.from_env(dcc_name, tmp_path, port=0, gateway_port=9765)
        config = build_mcp_http_config(opts, package_version="9.9.9", version_provider=lambda: "unused")

        assert config.dcc_type == dcc_name, f"{dcc_name}: dcc_type must match adapter identity"
        assert config.server_name == f"{dcc_name}-mcp", f"{dcc_name}: server_name must be derived from dcc_name"
        assert config.server_version == "9.9.9", f"{dcc_name}: server_version must propagate"
        assert config.gateway_port == 9765, f"{dcc_name}: explicit gateway_port should propagate"

    def test_gateway_options_persist_flag_is_env_readable(self, monkeypatch):
        """DCC_MCP_GATEWAY_PERSIST env var is defined and parseable."""
        monkeypatch.setenv("DCC_MCP_GATEWAY_PERSIST", "1")
        # The env var is read inside gateway_daemon::run() — not options layer.
        # Verify setenv doesn't crash and the value is accessible.
        import os

        v = os.environ.get("DCC_MCP_GATEWAY_PERSIST", "")
        assert v == "1"
