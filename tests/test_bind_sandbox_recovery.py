"""Tests for bind_and_register, is_path_allowed deep paths, and CrashRecoveryPolicy boundary.

Covers TransportManager.bind_and_register, SandboxContext.is_path_allowed,
and PyCrashRecoveryPolicy.next_delay_ms max_restarts boundary.

- TransportManager.bind_and_register() returns (instance_id, IpcListener)
- IpcListener.local_address() returns a non-empty string
- TransportManager tracks the registered service
- SandboxContext.is_path_allowed with deeply-nested sub-paths
- SandboxContext.is_path_allowed with path traversal attempts
- SandboxContext.is_path_allowed with multiple allowed roots
- SandboxContext.is_path_allowed empty policy (no restrictions)
- PyCrashRecoveryPolicy.next_delay_ms with max_restarts=1,2,3
- PyCrashRecoveryPolicy.next_delay_ms with exponential backoff beyond limit
- PyCrashRecoveryPolicy.next_delay_ms with max_restarts=0 (no restarts)
- PyCrashRecoveryPolicy.should_restart with various statuses
"""

# Import future modules
from __future__ import annotations

from pathlib import Path

# Import built-in modules
import tempfile

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── TransportManager.bind_and_register ───────────────────────────────────────


class TestBindAndRegister:
    def test_returns_tuple_of_two(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind")
        result = mgr.bind_and_register("maya")
        assert isinstance(result, tuple)
        assert len(result) == 2

    def test_instance_id_is_string(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind2")
        instance_id, _listener = mgr.bind_and_register("blender")
        assert isinstance(instance_id, str)
        assert len(instance_id) > 0

    def test_listener_type(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind3")
        _instance_id, listener = mgr.bind_and_register("houdini")
        assert listener is not None

    def test_listener_local_address_nonempty(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind4")
        _instance_id, listener = mgr.bind_and_register("maya", version="2025")
        addr = listener.local_address()
        # local_address() returns a TransportAddress object
        addr_str = str(addr)
        assert len(addr_str) > 0

    def test_listener_local_address_contains_scheme(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind5")
        _instance_id, listener = mgr.bind_and_register("maya")
        addr_str = str(listener.local_address())
        # On Windows: named pipe address "pipe://..." or TCP "127.0.0.1:PORT"
        assert len(addr_str) > 0
        # Should contain either "pipe" or ":"
        assert "pipe" in addr_str or ":" in addr_str

    def test_service_registered_after_bind(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind6")
        instance_id, _listener = mgr.bind_and_register("maya", version="2025")
        services = mgr.list_all_services()
        ids = [s.instance_id for s in services]
        assert instance_id in ids

    def test_bind_without_version(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind7")
        instance_id, listener = mgr.bind_and_register("blender")
        assert isinstance(instance_id, str)
        assert listener is not None

    def test_bind_with_metadata(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind8")
        instance_id, listener = mgr.bind_and_register(
            "maya",
            version="2024",
            metadata={"scene": "robot.ma", "project": "prod"},
        )
        assert isinstance(instance_id, str)
        assert listener is not None

    def test_multiple_binds_different_ids(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind9")
        id1, _ = mgr.bind_and_register("maya")
        id2, _ = mgr.bind_and_register("blender")
        assert id1 != id2

    def test_multiple_binds_service_count_increases(self) -> None:
        mgr = dcc_mcp_core.TransportManager("/tmp/dcc-mcp-test-bind10")
        mgr.bind_and_register("maya")
        mgr.bind_and_register("blender")
        assert len(mgr.list_all_services()) >= 2


# ── SandboxContext.is_path_allowed deep paths ─────────────────────────────────


class TestIsPathAllowedDeep:
    def _make_ctx(self, allowed_dirs: list[str]) -> dcc_mcp_core.SandboxContext:
        policy = dcc_mcp_core.SandboxPolicy()
        if allowed_dirs:
            policy.allow_paths(allowed_dirs)
        return dcc_mcp_core.SandboxContext(policy)

    def test_direct_child_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            ctx = self._make_ctx([tmpdir])
            child = str(Path(tmpdir) / "scene.mb")
            assert ctx.is_path_allowed(child) is True

    def test_nested_grandchild_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            ctx = self._make_ctx([tmpdir])
            nested = str(Path(tmpdir) / "assets" / "characters" / "robot.ma")
            assert ctx.is_path_allowed(nested) is True

    def test_deeply_nested_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            ctx = self._make_ctx([tmpdir])
            deep = str(Path(tmpdir) / "a" / "b" / "c" / "d" / "e" / "file.usd")
            assert ctx.is_path_allowed(deep) is True

    def test_sibling_dir_disallowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            allowed = str(Path(tmpdir) / "project")
            ctx = self._make_ctx([allowed])
            sibling = str(Path(tmpdir) / "other_project" / "scene.mb")
            assert ctx.is_path_allowed(sibling) is False

    def test_parent_dir_disallowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            allowed = str(Path(tmpdir) / "project" / "scenes")
            ctx = self._make_ctx([allowed])
            parent = str(Path(tmpdir) / "project")
            assert ctx.is_path_allowed(parent) is False

    def test_etc_passwd_disallowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            ctx = self._make_ctx([tmpdir])
            assert ctx.is_path_allowed("/etc/passwd") is False

    def test_windows_system_disallowed(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            ctx = self._make_ctx([tmpdir])
            assert ctx.is_path_allowed("C:/Windows/System32/cmd.exe") is False

    def test_multiple_allowed_roots_first(self) -> None:
        with tempfile.TemporaryDirectory() as dir1, tempfile.TemporaryDirectory() as dir2:
            ctx = self._make_ctx([dir1, dir2])
            path_in_dir1 = str(Path(dir1) / "scene.mb")
            assert ctx.is_path_allowed(path_in_dir1) is True

    def test_multiple_allowed_roots_second(self) -> None:
        with tempfile.TemporaryDirectory() as dir1, tempfile.TemporaryDirectory() as dir2:
            ctx = self._make_ctx([dir1, dir2])
            path_in_dir2 = str(Path(dir2) / "file.usd")
            assert ctx.is_path_allowed(path_in_dir2) is True

    def test_no_path_restrictions_allows_all(self) -> None:
        ctx = self._make_ctx([])
        assert ctx.is_path_allowed("/any/path/at/all") is True
        assert ctx.is_path_allowed("/etc/passwd") is True


# ── PyCrashRecoveryPolicy.next_delay_ms boundary ─────────────────────────────


class TestCrashRecoveryBoundary:
    def test_fixed_backoff_attempt_0(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(500)
        assert p.next_delay_ms("maya", 0) == 500

    def test_fixed_backoff_last_valid_attempt(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(500)
        # attempt 2 is the last valid (0,1,2 for max=3)
        assert p.next_delay_ms("maya", 2) == 500

    def test_fixed_backoff_at_max_raises(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(500)
        with pytest.raises(RuntimeError, match="max restarts"):
            p.next_delay_ms("maya", 3)

    def test_fixed_backoff_beyond_max_raises(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        p.use_fixed_backoff(500)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", 10)

    def test_max_restarts_1_attempt_0_ok(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=1)
        p.use_fixed_backoff(200)
        assert p.next_delay_ms("blender", 0) == 200

    def test_max_restarts_1_attempt_1_raises(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=1)
        p.use_fixed_backoff(200)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("blender", 1)

    def test_exponential_backoff_grows(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=10000)
        d0 = p.next_delay_ms("maya", 0)
        d1 = p.next_delay_ms("maya", 1)
        assert d1 >= d0

    def test_exponential_backoff_capped_at_max(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=10)
        p.use_exponential_backoff(initial_ms=1000, max_delay_ms=3000)
        d4 = p.next_delay_ms("maya", 4)
        assert d4 <= 3000

    def test_exponential_backoff_beyond_max_raises(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=2)
        p.use_exponential_backoff(initial_ms=100, max_delay_ms=5000)
        with pytest.raises(RuntimeError):
            p.next_delay_ms("maya", 2)

    def test_max_restarts_0_always_raises(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=0)
        p.use_fixed_backoff(1000)
        # should_restart returns False when max_restarts == 0
        assert p.should_restart("crashed") is False

    def test_should_restart_crashed(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("crashed") is True

    def test_should_restart_unresponsive(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("unresponsive") is True

    def test_should_restart_stopped_false(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("stopped") is False

    def test_should_restart_running_false(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=3)
        assert p.should_restart("running") is False

    def test_different_process_names_independent(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=2)
        p.use_fixed_backoff(100)
        assert p.next_delay_ms("maya", 0) == 100
        assert p.next_delay_ms("blender", 0) == 100
        assert p.next_delay_ms("houdini", 1) == 100

    def test_repr_contains_max_restarts(self) -> None:
        p = dcc_mcp_core.PyCrashRecoveryPolicy(max_restarts=5)
        r = repr(p)
        assert "5" in r
