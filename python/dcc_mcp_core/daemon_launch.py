"""Process daemonization for long-running dcc-mcp services.

Provides a :class:`Daemon` helper that double-forks (Unix) or
detaches (Windows), writes a pidfile, and redirects stdio.

Also re-exports :func:`detached_popen_kwargs` and :func:`launch_detached`
for launching detached subprocesses from any adapter or pipeline service.

Example::

    from dcc_mcp_core.daemon_launch import Daemon

    daemon = Daemon(pidfile="/var/run/myservice.pid")
    daemon.daemonize()
    # Process is now a session leader with no controlling terminal.
    # ... run service ...
"""

from __future__ import annotations

import atexit
import contextlib
import os
from pathlib import Path
import subprocess
import sys
from typing import Any
from typing import Mapping
from typing import Sequence

# ------------------------------------------------------------------
# Detached subprocess helpers
# ------------------------------------------------------------------


def detached_popen_kwargs(
    *,
    detached: bool = True,
    cwd: str | Path | None = None,
) -> dict[str, Any]:
    """Return ``subprocess.Popen`` keyword arguments for a background child."""
    kwargs: dict[str, Any] = {
        "stdin": subprocess.DEVNULL,
        "stdout": subprocess.DEVNULL,
        "stderr": subprocess.DEVNULL,
        "close_fds": os.name != "nt",
    }
    if cwd is not None:
        kwargs["cwd"] = str(Path(cwd).expanduser())
    if detached and os.name == "nt":
        flags = 0
        flags |= getattr(subprocess, "DETACHED_PROCESS", 0)
        flags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0)
        flags |= getattr(subprocess, "CREATE_NO_WINDOW", 0)
        kwargs["creationflags"] = flags
    return kwargs


def launch_detached(
    command: Sequence[str],
    *,
    env: Mapping[str, str] | None = None,
    cwd: str | Path | None = None,
    detached: bool = True,
) -> dict[str, Any]:
    """Start ``command`` without blocking the caller.

    Returns a dict with ``ok``, ``pid`` (when spawned), ``command``, and
    ``detached``. On failure, ``reason`` is ``spawn_failed`` and ``error``
    carries the exception text.

    When ``detached=True`` on Unix, the child gets its own process group
    (``start_new_session=True``) so it is immune to the parent's terminal
    signals. On Windows the typical creation flags are applied.
    """
    cmd = [str(part) for part in command]
    popen_env = dict(os.environ if env is None else env)
    kwargs = detached_popen_kwargs(detached=detached, cwd=cwd)
    kwargs["env"] = popen_env
    if detached and sys.platform != "win32":
        kwargs["start_new_session"] = True
    try:
        proc = subprocess.Popen(cmd, **kwargs)
    except Exception as exc:
        return {
            "ok": False,
            "reason": "spawn_failed",
            "error": str(exc),
            "command": cmd,
            "detached": detached,
        }
    return {
        "ok": True,
        "reason": "spawned",
        "pid": proc.pid,
        "command": cmd,
        "detached": detached,
    }


# ------------------------------------------------------------------
# Daemon class
# ------------------------------------------------------------------


class Daemon:
    """Detach the calling process into a well-behaved daemon.

    On Unix this performs the classic double-fork sequence:
    1. Fork → parent exits
    2. Child calls ``setsid()`` to become session leader
    3. Fork again → first child exits
    4. Grandchild is the daemon: chdir to ``/``, redirect stdio,
       write pidfile, register cleanup handlers.

    On Windows the process is detached via ``DETACHED_PROCESS`` +
    ``CREATE_NEW_PROCESS_GROUP`` flags (no double-fork is possible).

    Parameters
    ----------
    pidfile:
        Path to the PID file. Written atomically after daemonization and
        removed on exit (via ``atexit``).
    workdir:
        Working directory after daemonization. Defaults to ``/`` on Unix,
        the current directory on Windows.
    umask:
        File creation mask. Default ``0o022``.
    stdin:
        Path to redirect stdin from, or ``None`` to use ``os.devnull``.
    stdout:
        Path to redirect stdout to, or ``None`` to use ``os.devnull``.
    stderr:
        Path to redirect stderr to, or ``None`` to use ``os.devnull``.

    """

    def __init__(
        self,
        pidfile: str | Path | None = None,
        *,
        workdir: str | Path | None = None,
        umask: int = 0o022,
        stdin: str | Path | None = None,
        stdout: str | Path | None = None,
        stderr: str | Path | None = None,
    ) -> None:
        self._pidfile = Path(pidfile) if pidfile else None
        self._workdir = workdir
        self._umask = umask
        self._stdin = stdin
        self._stdout = stdout
        self._stderr = stderr

        self._running = False
        self._pid: int | None = None

    @property
    def pid(self) -> int | None:
        """The daemon's PID after :meth:`daemonize`, or ``None``."""
        return self._pid

    @property
    def is_running(self) -> bool:
        """True after :meth:`daemonize` has detached the process."""
        return self._running

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def daemonize(self) -> None:
        """Detach the current process into a daemon.

        After this call the original process has exited (Unix) or the
        child is fully detached (Windows). The daemon process continues
        executing the caller's code immediately after this method returns.
        """
        if self._running:
            return

        if sys.platform == "win32":
            self._daemonize_windows()
        else:
            self._daemonize_unix()

        self._running = True
        self._pid = os.getpid()

    # ------------------------------------------------------------------
    # Unix double-fork
    # ------------------------------------------------------------------

    def _daemonize_unix(self) -> None:
        # 1. First fork — parent exits, child continues.
        pid = os.fork()
        if pid > 0:
            os._exit(0)

        # 2. Become session leader, detach from controlling terminal.
        os.setsid()

        # 3. Second fork — session leader exits, grandchild is the
        #    real daemon (can never re-acquire a controlling terminal).
        pid = os.fork()
        if pid > 0:
            os._exit(0)

        # 4. Grandchild setup.
        self._setup_daemon_environment()

    # ------------------------------------------------------------------
    # Windows detach
    # ------------------------------------------------------------------

    def _daemonize_windows(self) -> None:
        # Already daemonized — sentinel env var set by the respawn below.
        if os.environ.get("DCC_MCP__DAEMONIZED") == "1":
            self._setup_daemon_environment()
            return

        # Respawn ourselves with DETACHED_PROCESS so the parent exits
        # and the child runs as a fully detached background process.
        exe = sys.executable
        args = [exe, *sys.argv]
        env = dict(os.environ)
        env["DCC_MCP__DAEMONIZED"] = "1"

        flags = 0
        flags |= getattr(subprocess, "DETACHED_PROCESS", 0x0000_0008)
        flags |= getattr(subprocess, "CREATE_NEW_PROCESS_GROUP", 0x0000_0200)

        child = subprocess.Popen(
            args,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=flags,
        )

        # Write pidfile for the spawned child before parent exits.
        if self._pidfile:
            self._pidfile.parent.mkdir(parents=True, exist_ok=True)
            self._pidfile.write_text(f"{child.pid}\n", encoding="ascii")

        os._exit(0)

    # ------------------------------------------------------------------
    # Shared post-detach setup
    # ------------------------------------------------------------------

    def _setup_daemon_environment(self) -> None:
        # Working directory.
        if self._workdir is not None:
            os.chdir(self._workdir)
        elif sys.platform != "win32":
            os.chdir("/")

        # File creation mask.
        os.umask(self._umask)

        # Redirect standard file descriptors.
        # We need raw fd objects for dup2, so Path.open() and context
        # managers don't apply.
        devnull = os.devnull
        si = open(self._stdin or devnull, "rb")  # noqa: PTH123, SIM115
        so = open(self._stdout or devnull, "ab")  # noqa: PTH123, SIM115
        se = open(self._stderr or devnull, "ab")  # noqa: PTH123, SIM115
        os.dup2(si.fileno(), 0)
        os.dup2(so.fileno(), 1)
        os.dup2(se.fileno(), 2)
        si.close()
        so.close()
        se.close()

        # Write pidfile.
        if self._pidfile:
            self._write_pidfile()
            atexit.register(self._remove_pidfile)

    # ------------------------------------------------------------------
    # Pidfile helpers
    # ------------------------------------------------------------------

    def _write_pidfile(self) -> None:
        assert self._pidfile is not None
        self._pidfile.parent.mkdir(parents=True, exist_ok=True)
        self._pidfile.write_text(f"{os.getpid()}\n", encoding="ascii")

    def _remove_pidfile(self) -> None:
        if self._pidfile is not None:
            with contextlib.suppress(FileNotFoundError):
                self._pidfile.unlink()

    def __repr__(self) -> str:
        return f"Daemon(pid={self._pid}, running={self._running}, pidfile={self._pidfile!s})"
