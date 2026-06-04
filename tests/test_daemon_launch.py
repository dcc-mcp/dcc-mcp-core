"""Tests for daemon_launch module (PIP-513)."""

from __future__ import annotations

import contextlib
import os
from pathlib import Path
import signal
import subprocess as _sp  # imported as _sp to avoid shadowing daemon_launch.subprocess
import sys
import textwrap

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

    @pytest.mark.skipif(sys.platform == "win32", reason="Unix start_new_session test")
    def test_launch_detached_uses_new_session_on_unix(self):
        """Child process group differs from parent when detached=True."""
        cmd = ["python", "-c", "import time; time.sleep(2)"]
        result = launch_detached(cmd, detached=True)
        assert result.get("ok") is True
        child_pid = result["pid"]
        try:
            parent_pgid = os.getpgid(0)
            child_pgid = os.getpgid(child_pid)
            assert child_pgid != parent_pgid, (
                f"detached child pgid {child_pgid} should differ from parent pgid {parent_pgid}"
            )
        finally:
            with contextlib.suppress(ProcessLookupError, OSError):
                os.kill(child_pid, signal.SIGTERM)


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

    @pytest.mark.skipif(sys.platform == "win32", reason="Unix daemonize respawn test")
    def test_daemonize_on_unix_fork_and_setsid(self, tmp_path):
        """daemonize() on Unix detaches and continues after daemonize()."""
        pidfile = tmp_path / "daemon.pid"
        marker = tmp_path / "daemon_ready.txt"
        error = tmp_path / "daemon_error.txt"
        workdir = tmp_path / "work"
        workdir.mkdir()
        worker = tmp_path / "daemon_worker.py"
        package_root = Path(__file__).resolve().parent.parent / "python"
        worker.write_text(
            textwrap.dedent(f"""\
            from pathlib import Path
            import os
            import sys
            import time
            import traceback

            sys.path.insert(0, {str(package_root)!r})
            from dcc_mcp_core.daemon_launch import Daemon

            pidfile = Path({str(pidfile)!r})
            marker = Path({str(marker)!r})
            error = Path({str(error)!r})

            try:
                d = Daemon(pidfile=pidfile, workdir={str(workdir)!r})
                d.daemonize()
                marker.write_text(
                    f"daemonized pid={{os.getpid()}} pgid={{os.getpgrp()}} cwd={{os.getcwd()}}",
                    encoding="ascii",
                )
                time.sleep(10)
            except BaseException:
                error.write_text(traceback.format_exc(), encoding="utf-8")
                raise
            """),
            encoding="ascii",
        )
        proc = _sp.Popen(
            [sys.executable, str(worker)],
            stdout=_sp.DEVNULL,
            stderr=_sp.DEVNULL,
        )
        proc.wait(timeout=15)
        assert proc.returncode == 0

        import time

        daemon_pid: int | None = None
        try:
            for _ in range(100):
                time.sleep(0.05)
                if error.exists():
                    pytest.fail(error.read_text(encoding="utf-8"))
                if marker.exists() and pidfile.exists():
                    break

            assert marker.exists(), "daemon_ready marker should be written by daemon child"
            assert pidfile.exists(), "pidfile should be written by daemon child"
            daemon_pid = int(pidfile.read_text(encoding="ascii").strip())
            assert daemon_pid != proc.pid
            assert f"pid={daemon_pid}" in marker.read_text(encoding="ascii")
        finally:
            if daemon_pid is not None:
                with contextlib.suppress(ProcessLookupError, OSError):
                    os.kill(daemon_pid, signal.SIGTERM)
            with contextlib.suppress(FileNotFoundError):
                pidfile.unlink()

    @pytest.mark.skipif(sys.platform != "win32", reason="Windows daemonize respawn test")
    def test_daemonize_on_windows_respawns_detached(self, tmp_path):
        """daemonize() on Windows respawns the process detached and exits parent."""
        pidfile = tmp_path / "daemon.pid"
        marker = tmp_path / "daemon_ready.txt"
        worker = tmp_path / "daemon_worker.py"
        worker.write_text(
            textwrap.dedent(f"""\
            from pathlib import Path
            import sys
            sys.path.insert(0, {str(Path(__file__).resolve().parent.parent / "python")!r})
            from dcc_mcp_core.daemon_launch import Daemon

            d = Daemon(pidfile={str(pidfile)!r})
            d.daemonize()
            # Only the respawned child reaches here
            import os
            os.environ.pop("DCC_MCP__DAEMONIZED", None)
            Path({str(marker)!r}).write_text(f"daemonized pid={{os.getpid()}}")
            import time
            time.sleep(10)  # Keep pidfile alive for test to read it
            """),
            encoding="ascii",
        )
        proc = _sp.Popen(
            [sys.executable, str(worker)],
            stdout=_sp.DEVNULL,
            stderr=_sp.DEVNULL,
        )
        proc.wait(timeout=15)
        # Parent exits immediately (os._exit) after spawning detached child.
        import time

        for _ in range(100):
            time.sleep(0.05)
            if marker.exists() and pidfile.exists():
                break
        assert marker.exists(), "daemon_ready marker should be written by detached child"
        assert pidfile.exists(), "pidfile should be written by detached child"
        # Cleanup: kill the detached child
        child_pid = int(pidfile.read_text().strip())
        with contextlib.suppress(ProcessLookupError, OSError):
            os.kill(child_pid, signal.SIGTERM)


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
