"""Owned-launch boundary and real client framing with in-memory process pipes.

Native API fixtures are mocks; these tests do not prove an independent worker.
"""

from __future__ import annotations

import ctypes
import os
from pathlib import Path
import subprocess
from types import SimpleNamespace
from unittest.mock import Mock

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host import cua_mcp_managed_process as managed
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from test_cua_mcp_pixels import FakeMcpProcess
from test_cua_mcp_pixels import options


class TransferredProcess(FakeMcpProcess):
    def __init__(self, command):
        super().__init__()
        self.args = command
        self.pid = 314
        self._handle = 987
        self.terminations = 0

    def terminate(self):
        self.terminations += 1
        self.finish()


@pytest.fixture
def owner(tmp_path, monkeypatch):
    config = options(tmp_path)
    process = TransferredProcess([config.binary, "mcp-server"])
    monkeypatch.setattr(managed.subprocess, "Popen", TransferredProcess)
    return config, process


def test_client_initializes_starts_stops_and_closes_transferred_child(owner, monkeypatch):
    config, process = owner
    identity = managed.OriginalProcessIdentity(process.pid, 12345, config.binary, config.sha256)
    proof = Mock(return_value=identity)
    monkeypatch.setattr(managed, "original_independent_identity", proof)
    monkeypatch.setenv("DCC_CUA_RECORDING_OUTPUT_ROOT", "must-not-inherit")
    monkeypatch.setenv("DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT", "must-not-inherit")
    calls = []

    def launch(command, environment):
        calls.append((command, environment))
        assert process.requests == []  # Native proof must precede initialize/start_task.
        return process

    client = PixelsMcpHostClient(
        session_id="owned",
        dcc_type="unreal",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        options=config,
        managed_launcher=launch,
    )
    assert client._transport.managed_process_identity == identity
    assert len(calls) == 1 and calls[0][0] == process.args
    assert "DCC_CUA_RECORDING_OUTPUT_ROOT" not in calls[0][1]
    assert "DCC_CUA_CAPTURE_PREPARATION_JOURNAL_ROOT" not in calls[0][1]
    proof.assert_called_once()
    assert client.task_id == "public-task"
    client.stop()
    assert any(r.get("params", {}).get("name") == "stop_task" for r in process.requests)
    assert process.returncode == 0 and process.terminations == 0


@pytest.mark.parametrize("failure", ["eof", "identity"])
def test_transferred_startup_failure_closes_without_relaunch(owner, monkeypatch, failure):
    config, process = owner
    monkeypatch.setattr(managed, "original_independent_identity", Mock(return_value=object()))
    if failure == "eof":
        process.stdin.write = lambda data: process.finish()
    else:
        process.version = "foreign-runtime"
    launch = Mock(return_value=process)
    with pytest.raises(CuaCliError):
        PixelsMcpHostClient(
            session_id="owned",
            dcc_type="unreal",
            process_id=42,
            window_handle=500,
            allow_raw_input=False,
            options=config,
            managed_launcher=launch,
        )
    launch.assert_called_once()
    assert process.returncode == 0
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


@pytest.mark.parametrize("unknown", [False, True])
def test_refusal_retains_original_error_owner_and_unknown_terminal(owner, monkeypatch, unknown):
    config, process = owner
    primary = OSError("SupervisorNotIndependent")
    monkeypatch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
    cleanup = subprocess.TimeoutExpired(process.args, 2)
    if unknown:
        process.wait = Mock(side_effect=cleanup)
    with pytest.raises(managed.ManagedLaunchRefusal) as rejected:
        managed.accept_managed_process(process, process.args, config.sha256, Path(config.binary).stat())
    error = rejected.value
    assert error.primary is primary and error.process is process
    assert error.cleanup_verified is (not unknown)
    assert error.cleanup_error is (cleanup if unknown else None)
    assert error.exit_code == (None if unknown else 0)
    assert process.terminations == 1 and process.requests == []


@pytest.mark.parametrize("invalid", ["command", "stdin", "stdout"])
def test_changed_launch_contract_refuses_before_native_calls(owner, monkeypatch, invalid):
    config, process = owner
    if invalid == "command":
        process.args = ["foreign.exe", "mcp-server"]
    else:
        setattr(process, invalid, None)
    proof = Mock()
    monkeypatch.setattr(managed, "original_independent_identity", proof)
    with pytest.raises(managed.ManagedLaunchRefusal):
        managed.accept_managed_process(
            process, [config.binary, "mcp-server"], config.sha256, Path(config.binary).stat()
        )
    proof.assert_not_called()
    assert process.terminations == 1


def test_pid_record_is_not_an_owned_process(owner):
    config, _ = owner
    with pytest.raises(TypeError):
        managed.accept_managed_process(
            {"pid": 314}, [config.binary, "mcp-server"], config.sha256, Path(config.binary).stat()
        )


def test_client_refusal_preserves_owner_without_fallback_or_mcp_requests(owner, monkeypatch):
    config, process = owner
    primary = OSError("SupervisorNotIndependent")
    monkeypatch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
    launch = Mock(return_value=process)
    with pytest.raises(managed.ManagedLaunchRefusal) as refused:
        PixelsMcpHostClient(
            session_id="owned",
            dcc_type="unreal",
            process_id=42,
            window_handle=500,
            allow_raw_input=False,
            options=config,
            managed_launcher=launch,
        )
    assert refused.value.primary is primary and refused.value.process is process
    assert process.requests == [] and process.terminations == 1
    launch.assert_called_once()


@pytest.mark.skipif(os.name != "nt", reason="Win32 API shape fixture")
@pytest.mark.parametrize("failure", [None, "job", "job_api", "image", "image_api", "file_id", "hash", "birth", "pid"])
def test_original_handle_image_birth_and_job_proof(owner, monkeypatch, failure):
    from ctypes import wintypes

    config, process = owner
    binary = Path(config.binary)
    before = binary.stat()
    births = iter([12345, 54321 if failure == "birth" else 12345])

    def job(handle, job_handle, membership):
        assert handle.value == process._handle and job_handle is None
        ctypes.cast(membership, ctypes.POINTER(wintypes.BOOL)).contents.value = int(failure == "job")
        return int(failure != "job_api")

    def times(handle, created, *unused):
        assert handle.value == process._handle
        value = ctypes.cast(created, ctypes.POINTER(wintypes.FILETIME)).contents
        value.dwLowDateTime, value.dwHighDateTime = next(births), 0
        return 1

    def image(handle, flags, buffer, length):
        assert handle.value == process._handle and flags == 0
        buffer.value = str(binary if failure != "image" else binary.with_name("foreign.exe"))
        return int(failure != "image_api")

    kernel = SimpleNamespace(
        GetProcessId=Mock(return_value=0 if failure == "pid" else process.pid),
        IsProcessInJob=Mock(side_effect=job),
        GetProcessTimes=Mock(side_effect=times),
        QueryFullProcessImageNameW=Mock(side_effect=image),
    )
    monkeypatch.setattr(ctypes, "WinDLL", Mock(return_value=kernel))
    if failure == "file_id":
        binary.rename(binary.with_suffix(".old"))
        binary.write_bytes(b"not executable: the test replaces Popen")
    expected_hash = "0" * 64 if failure == "hash" else config.sha256
    if failure:
        with pytest.raises((OSError, ValueError)):
            managed.original_independent_identity(process, process.args, expected_hash, before)
    else:
        identity = managed.original_independent_identity(process, process.args, expected_hash, before)
        assert identity.process_id == process.pid and identity.creation_time_100ns == 12345
        assert identity.executable == str(binary) and identity.sha256 == config.sha256
        assert kernel.GetProcessTimes.call_count == 2


def test_managed_worker_requires_native_platform_proof(owner, monkeypatch):
    config, process = owner
    # Check before pathlib/WinDLL use; no alternate platform launcher is selected.
    monkeypatch.setattr(managed, "os", SimpleNamespace(name="posix"))
    with pytest.raises(OSError, match="requires Windows"):
        managed.original_independent_identity(process, process.args, config.sha256, Path(config.binary).stat())
