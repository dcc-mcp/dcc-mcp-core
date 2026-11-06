"""Real Win32 ownership proof for an ordinary child, without UI or Job changes."""

from __future__ import annotations

import ctypes
import hashlib
import os
from pathlib import Path
import subprocess
import sys

import pytest

from dcc_mcp_core.host.cua_mcp_managed_process import ManagedLaunchRefusal
from dcc_mcp_core.host.cua_mcp_managed_process import accept_managed_process

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Real Windows process ownership")


def test_original_child_native_identity_or_job_refusal(record_property):
    from ctypes import wintypes

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.IsProcessInJob.argtypes = [wintypes.HANDLE, wintypes.HANDLE, ctypes.POINTER(wintypes.BOOL)]
    kernel.IsProcessInJob.restype = wintypes.BOOL
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    kernel.GetProcessTimes.restype = wintypes.BOOL
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    kernel.GetExitCodeProcess.restype = wintypes.BOOL
    kernel.QueryFullProcessImageNameW.argtypes = [
        wintypes.HANDLE,
        wintypes.DWORD,
        wintypes.LPWSTR,
        ctypes.POINTER(wintypes.DWORD),
    ]
    kernel.QueryFullProcessImageNameW.restype = wintypes.BOOL

    executable = Path(sys.executable)
    before = executable.stat()
    sha256 = hashlib.sha256(executable.read_bytes()).hexdigest()
    command = [str(executable), "-I", "-S", "-c", "import sys; sys.stdin.buffer.read()"]
    process = subprocess.Popen(
        command,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        close_fds=True,
        creationflags=subprocess.CREATE_NO_WINDOW,
    )
    original_handle = process._handle
    handle = wintypes.HANDLE(int(original_handle))
    try:
        in_job = wintypes.BOOL(1)
        assert kernel.IsProcessInJob(handle, None, ctypes.byref(in_job))
        record_property("original_child_in_job", bool(in_job.value))
        if in_job.value:
            with pytest.raises(ManagedLaunchRefusal) as rejected:
                accept_managed_process(process, command, sha256, before)
            refusal = rejected.value
            assert refusal.process is process
            assert str(refusal.primary) == "SupervisorNotIndependent: managed worker remains in a Job."
            assert refusal.cleanup_verified and refusal.cleanup_error is None
            assert refusal.exit_code == process.wait(timeout=5)
            record_property("native_outcome", "job_refused_original_child_exit_verified")
        else:
            identity = accept_managed_process(process, command, sha256, before)
            created, exited, system, user = (wintypes.FILETIME() for _ in range(4))
            assert kernel.GetProcessTimes(
                handle, ctypes.byref(created), ctypes.byref(exited), ctypes.byref(system), ctypes.byref(user)
            )
            image = ctypes.create_unicode_buffer(32768)
            length = wintypes.DWORD(len(image))
            assert kernel.QueryFullProcessImageNameW(handle, 0, image, ctypes.byref(length))
            assert identity.process_id == process.pid
            assert identity.creation_time_100ns == (created.dwHighDateTime << 32) | created.dwLowDateTime
            assert Path(identity.executable).resolve() == Path(image.value).resolve() == executable.resolve()
            assert identity.sha256 == sha256
            process.stdin.close()
            assert process.wait(timeout=5) == 0
            record_property("native_outcome", "independent_identity_and_eof_exit_verified")
        assert process._handle is original_handle
        assert kernel.WaitForSingleObject(handle, 0) == 0
        exit_code = wintypes.DWORD(259)
        assert kernel.GetExitCodeProcess(handle, ctypes.byref(exit_code))
        assert exit_code.value == process.returncode and exit_code.value != 259
    finally:
        # EOF settles only this fixture child, including a failed assertion.
        process.stdin.close()
        try:
            process.wait(timeout=5)
        except Exception as error:
            error.process = process
            raise
        finally:
            if process.poll() is not None:
                process.stdout.close()
