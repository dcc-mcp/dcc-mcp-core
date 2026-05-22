"""Process and sentinel helpers for import-light install lifecycle code."""

from __future__ import annotations

import contextlib
import errno
import os
from pathlib import Path
import signal
import time
from typing import Any
from typing import Dict
from typing import Optional

_WINDOWS_LOCK_WINERRORS = {5, 32, 33}
_LOCK_ERRNOS = {errno.EACCES, errno.EPERM}


def is_windows_lock_error(exc: OSError) -> bool:
    if os.name != "nt":
        return False
    winerror = getattr(exc, "winerror", None)
    if winerror in _WINDOWS_LOCK_WINERRORS:
        return True
    if isinstance(exc, PermissionError) and getattr(exc, "errno", None) in _LOCK_ERRNOS:
        return True
    return isinstance(exc, PermissionError)


def entry_runtime_alive(sentinel_path: Any, pid: Optional[int]) -> Optional[bool]:
    sentinel = _sentinel_owner_dead(sentinel_path)
    if sentinel is True:
        return False
    if sentinel is False:
        return True
    return is_pid_alive(pid) if pid is not None else None


def is_pid_alive(pid: Optional[int]) -> bool:
    if pid is None or pid <= 0:
        return False
    if pid == os.getpid():
        return True
    if sys_platform_is_windows():
        return _is_pid_alive_windows(pid)
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    except OSError:
        return False
    return True


def terminate_pid(pid: int, timeout_secs: float, target_kind: str) -> Dict[str, Any]:
    if not is_pid_alive(pid):
        return {"pid": pid, "target": target_kind, "status": "already_stopped"}
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError as exc:
        return {
            "pid": pid,
            "target": target_kind,
            "status": "failed",
            "message": str(exc),
        }

    deadline = time.time() + max(timeout_secs, 0.0)
    while time.time() < deadline:
        if not is_pid_alive(pid):
            return {"pid": pid, "target": target_kind, "status": "stopped"}
        time.sleep(0.05)
    return {
        "pid": pid,
        "target": target_kind,
        "status": "still_running",
        "message": "Stop was requested but the process is still alive.",
    }


def sys_platform_is_windows() -> bool:
    return os.name == "nt"


def _to_path(path: Any) -> Optional[Path]:
    if path in (None, ""):
        return None
    try:
        return Path(str(path)).expanduser().resolve()
    except OSError:
        return Path(str(path)).expanduser().absolute()


def _sentinel_owner_dead(sentinel_path: Any) -> Optional[bool]:
    path = _to_path(sentinel_path)
    if path is None:
        return None
    if not path.exists():
        return True
    if sys_platform_is_windows():
        return _sentinel_owner_dead_windows(path)
    return _sentinel_owner_dead_posix(path)


def _sentinel_owner_dead_windows(path: Path) -> Optional[bool]:
    try:
        import msvcrt
    except ImportError:
        return None
    try:
        with path.open("r+b") as handle:
            try:
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError as exc:
                if (
                    getattr(exc, "errno", None) in _LOCK_ERRNOS
                    or getattr(exc, "winerror", None) in _WINDOWS_LOCK_WINERRORS
                ):
                    return False
                return None
            with contextlib.suppress(OSError):
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            return True
    except FileNotFoundError:
        return True
    except OSError:
        return None


def _sentinel_owner_dead_posix(path: Path) -> Optional[bool]:
    try:
        import fcntl
    except ImportError:
        return None
    try:
        with path.open("r+b") as handle:
            try:
                fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return False
            except OSError as exc:
                if getattr(exc, "errno", None) in {errno.EACCES, errno.EAGAIN, errno.EWOULDBLOCK}:
                    return False
                return None
            with contextlib.suppress(OSError):
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
            return True
    except FileNotFoundError:
        return True
    except OSError:
        return None


def _is_pid_alive_windows(pid: int) -> bool:
    try:
        import ctypes
    except ImportError:
        return True

    process_query_limited_information = 0x1000
    still_active = 259
    kernel32 = ctypes.windll.kernel32
    handle = kernel32.OpenProcess(process_query_limited_information, False, pid)
    if not handle:
        return False
    try:
        code = ctypes.c_ulong()
        if not kernel32.GetExitCodeProcess(handle, ctypes.byref(code)):
            return True
        return code.value == still_active
    finally:
        kernel32.CloseHandle(handle)
