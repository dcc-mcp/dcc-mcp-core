"""Tests for daemon_launch module (PIP-513)."""

from __future__ import annotations

import contextlib
import os
from pathlib import Path
import signal
import sys
import tempfile

import pytest

from dcc_mcp_core.daemon_launch import Daemon
from dcc_mcp_core.daemon_launch import detached_popen_kwargs
from dcc_mcp_core.daemon_launch import launch_detached


class TestDetachedPopenKwargs:
    def test_default_detached_kwargs(self):
        import subprocess as _sp

        kwargs = detached_popen_kwargs()
        assert kwargs["stdin"] is _sp.DEVNULL
        assert kwargs["stdout"] is _sp.DEVNULL
        assert kwargs["stderr"] is _sp.DEVNULL
        assert "cwd" not in kwargs

    def test_cwd_is_passed(self, tmp_path):
        kwargs = detached_popen_kwargs(cwd=tmp_path)
        assert kwargs["cwd"] == str(tmp_path)

    def test_windows_flags_are_set_on_windows(self):
        kwargs = detached_popen_kwargs(detached=True)
        if sys.platform == "win32":
            assert "creationflags" in kwargs
        else:
            assert "creationflags" not in kwargs


class TestLaunchDetached:
    def test_launch_echo_succeeds(self):
        cmd = ["python", "-c", "exit(0)"] if sys.platform != "win32" else ["cmd", "/c", "exit 0"]
        result = launch_detached(cmd)
        assert result.get("ok") is True
        assert "pid" in result

    def test_launch_nonexistent_binary_reports_error(self):
        result = launch_detached(["nonexistent_binary_12345"])
        assert result.get("ok") is False
        assert result.get("reason") == "spawn_failed"


class TestDaemonPidfile:
    def test_write_and_remove_pidfile(self, tmp_path):
        pidfile = tmp_path / "test.pid"
        daemon = Daemon(pidfile=pidfile)
        daemon._pidfile = pidfile
        daemon._pid = os.getpid()
        daemon._write_pidfile()

        assert pidfile.exists()
        content = pidfile.read_text().strip()
        assert content == str(os.getpid())

        daemon._remove_pidfile()
        assert not pidfile.exists()

    def test_remove_nonexistent_pidfile_does_not_raise(self, tmp_path):
        pidfile = tmp_path / "nonexistent.pid"
        daemon = Daemon(pidfile=pidfile)
        daemon._remove_pidfile()  # Must not raise


class TestDaemonApi:
    def test_initial_state(self):
        daemon = Daemon()
        assert daemon.pid is None
        assert not daemon.is_running

    def test_initial_state_with_pidfile(self, tmp_path):
        pidfile = tmp_path / "daemon.pid"
        daemon = Daemon(pidfile=pidfile)
        assert daemon.pid is None
        assert not daemon.is_running

    def test_repr(self):
        daemon = Daemon(pidfile="/tmp/foo.pid")
        r = repr(daemon)
        assert "Daemon" in r
        assert "foo.pid" in r

    @pytest.mark.skipif(sys.platform == "win32", reason="no real daemonize on Windows")
    def test_daemonize_on_unix_fork_and_setsid(self):
        """Integration smoke: daemonize() does a double-fork on Unix."""
        pidfile = tempfile.mktemp(suffix=".pid")
        daemon = Daemon(pidfile=pidfile, workdir="/tmp")
        try:
            pid = os.fork()
            if pid == 0:
                # Child: daemonize then exit immediately
                daemon.daemonize()
                os._exit(0)
            else:
                # Parent: wait for child (the intermediate process should exit
                # quickly, and the grandchild writes pidfile).
                os.waitpid(pid, 0)
                # Small delay for pidfile write
                import time

                pf = Path(pidfile)
                for _ in range(50):
                    time.sleep(0.02)
                    if pf.exists():
                        break
                assert pf.exists(), "pidfile should be written by the daemon grandchild"
                grand_pid = int(pf.read_text().strip())
                # Kill the grandchild
                with contextlib.suppress(OSError):
                    os.kill(grand_pid, signal.SIGTERM)
        finally:
            with contextlib.suppress(FileNotFoundError):
                Path(pidfile).unlink()


class TestDaemonImports:
    """Verify all public symbols are importable from the top-level package."""

    def test_daemon_importable(self):
        from dcc_mcp_core import Daemon

        assert Daemon is not None

    def test_detached_popen_kwargs_importable(self):
        from dcc_mcp_core import detached_popen_kwargs

        assert detached_popen_kwargs is not None

    def test_launch_detached_importable(self):
        from dcc_mcp_core import launch_detached

        assert launch_detached is not None
