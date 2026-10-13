"""Trusted constructor dependency transferring one exclusive child to Core.

This is not a remote broker, shared-worker attachment, or authorization API.
The launcher owns failures before return; a successful return transfers the
original Popen handle, exclusive stdio, and all exit responsibility to Core.
"""

from __future__ import annotations

import ctypes
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import subprocess
from typing import Callable
from typing import Dict
from typing import List


@dataclass(frozen=True)
class OriginalProcessIdentity:
    """Original handle identity and its independently checked executable pin."""

    process_id: int
    creation_time_100ns: int
    executable: str
    sha256: str


class ManagedLaunchRefusal(RuntimeError):
    """Retain the transferred owner and both errors when cleanup is unknown."""

    def __init__(
        self, process: subprocess.Popen, primary: Exception, cleanup_error: Exception | None, exit_code: int | None
    ) -> None:
        super().__init__("Managed UI Control worker refused before MCP initialization.")
        self.process = process
        self.primary = primary
        self.cleanup_error = cleanup_error
        self.exit_code = exit_code
        self.cleanup_verified = cleanup_error is None and type(exit_code) is int


def _file_identity(info: os.stat_result) -> tuple[int, int, int, int]:
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def original_independent_identity(
    process: subprocess.Popen, command: list[str], sha256: str, binary_identity: os.stat_result
) -> OriginalProcessIdentity:
    """Bind image, birth and Job state through the original handle only."""
    if os.name != "nt":
        raise OSError("Managed worker proof requires Windows.")
    from ctypes import wintypes

    handle = wintypes.HANDLE(int(process._handle))
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetProcessId.argtypes = [wintypes.HANDLE]
    kernel.GetProcessId.restype = wintypes.DWORD
    kernel.IsProcessInJob.argtypes = [wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)]
    kernel.IsProcessInJob.restype = wintypes.BOOL
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.GetProcessTimes.restype = wintypes.BOOL
    kernel.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL

    def live_birth() -> int:
        if kernel.GetProcessId(handle) != process.pid or process.poll() is not None:
            raise OSError("Managed worker original identity is not live.")
        in_job = wintypes.BOOL(1)
        if not kernel.IsProcessInJob(handle, None, ctypes.byref(in_job)):
            raise ctypes.WinError(ctypes.get_last_error())
        if in_job.value:
            raise OSError("SupervisorNotIndependent: managed worker remains in a Job.")
        created, exited, system, user = (wintypes.FILETIME() for _ in range(4))
        if not kernel.GetProcessTimes(
            handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(system), ctypes.byref(user)
        ):
            raise ctypes.WinError(ctypes.get_last_error())
        birth = (created.dwHighDateTime << 32) | created.dwLowDateTime
        if not birth:
            raise OSError("Managed worker creation time is missing.")
        return birth

    birth = live_birth()
    image = ctypes.create_unicode_buffer(32768)
    length = wintypes.DWORD(len(image))
    if not kernel.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(length)):
        raise ctypes.WinError(ctypes.get_last_error())
    image_path = Path(image.value)
    if image_path.resolve() != Path(command[0]).resolve():
        raise ValueError("Managed worker image differs from the pinned executable.")
    with image_path.open("rb") as source:
        before = os.fstat(source.fileno())
        if _file_identity(before) != _file_identity(binary_identity):
            raise ValueError("Managed worker executable identity changed during launch.")
        digest = hashlib.sha256()
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
        if digest.hexdigest() != sha256 or _file_identity(os.fstat(source.fileno())) != _file_identity(before):
            raise ValueError("Managed worker executable failed pinned SHA-256 verification.")
        if _file_identity(image_path.stat()) != _file_identity(before):
            raise ValueError("Managed worker executable path was replaced during verification.")
    if live_birth() != birth:
        raise OSError("Managed worker birth changed during verification.")
    return OriginalProcessIdentity(process.pid, birth, str(image_path), sha256)


def accept_managed_process(
    process: subprocess.Popen, command: list[str], sha256: str, binary_identity: os.stat_result
) -> OriginalProcessIdentity:
    """Accept an exclusively transferred original child, never a shared owner."""
    if not isinstance(process, subprocess.Popen):
        raise TypeError("Managed launcher must transfer the original Popen child.")
    try:
        if process.args != command or process.stdin is None or process.stdout is None:
            raise ValueError("Managed launcher changed the pinned command or exclusive stdio.")
        return original_independent_identity(process, command, sha256, binary_identity)
    except Exception as primary:
        cleanup_error, exit_code = None, None
        try:
            if process.poll() is None:
                process.terminate()
            exit_code = process.wait(timeout=2)
        except Exception as error:
            cleanup_error = error
        # Keep the owner reachable even if the original handle has no terminal.
        raise ManagedLaunchRefusal(process, primary, cleanup_error, exit_code) from primary


ManagedLauncher = Callable[[List[str], Dict[str, str]], subprocess.Popen]
