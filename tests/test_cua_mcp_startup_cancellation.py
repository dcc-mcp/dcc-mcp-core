"""Cancellation ownership with real, stdin-waiting children and mocked MCP proof.

These fixtures launch only their own Python children. They do not establish
native Windows independence, launch authorization, or a production channel.
"""

from __future__ import annotations

import asyncio
from concurrent.futures import ThreadPoolExecutor
import hashlib
import inspect
from pathlib import Path
import subprocess
import sys
import threading
from unittest.mock import Mock

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host import cua_mcp_managed_process as managed
from dcc_mcp_core.host import cua_mcp_transport as transport
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.cua_mcp_transport import OwnedCuaMcpTransport
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import VERSION
from test_cua_mcp_pixels import tool_catalog

INTERRUPTIONS = [KeyboardInterrupt, SystemExit, asyncio.CancelledError]


@pytest.fixture
def real_children(tmp_path):
    children = []
    # Match the real transport argv while keeping the child a private,
    # stdin-only Python fixture. It implements no MCP or native/UI capability.
    (tmp_path / "mcp-server").write_text("import sys; sys.stdin.buffer.read()", encoding="utf-8")

    def launch():
        command = [sys.executable, "mcp-server"]
        process = subprocess.Popen(
            command, cwd=tmp_path, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        children.append(process)
        return process

    try:
        yield launch
    finally:
        # All monkeypatch contexts end before fixture cleanup. Never touch an
        # ambient process or wait without a timeout, even after a failed test.
        cleanup_errors = []
        for process in children:
            try:
                process.stdin.close()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    try:
                        process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=2)
            except BaseException as error:
                error.process = process
                cleanup_errors.append(error)
            finally:
                if process.poll() is not None:
                    process.stdout.close()
        if cleanup_errors:
            raise cleanup_errors[0]


def accept(process):
    executable = Path(sys.executable)
    return managed.accept_managed_process(
        process, process.args, hashlib.sha256(executable.read_bytes()).hexdigest(), executable.stat()
    )


@pytest.mark.parametrize("error_type", [OSError, *INTERRUPTIONS])
def test_proof_interruption_settles_real_child_and_preserves_exception(real_children, monkeypatch, error_type):
    process = real_children()
    primary = error_type("interrupted identity proof")
    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        with pytest.raises(BaseException) as caught:
            accept(process)
    if error_type is OSError:
        refusal = caught.value
        assert isinstance(refusal, managed.ManagedLaunchRefusal)
    else:
        assert caught.value is primary
        assert caught.value.process is process
        refusal = caught.value.managed_launch_refusal
    assert refusal.primary is primary and refusal.process is process
    assert refusal.cleanup_verified and refusal.cleanup_error is None
    assert type(refusal.exit_code) is int and refusal.exit_code == process.poll()
    assert process.stdin.closed and process.stdout.closed


@pytest.mark.parametrize("error_type", INTERRUPTIONS)
@pytest.mark.parametrize("cleanup_type", [OSError, *INTERRUPTIONS])
def test_interrupted_proof_cleanup_retains_original_cancel_and_owner(
    real_children, monkeypatch, error_type, cleanup_type
):
    process = real_children()
    primary, cleanup = error_type("proof cancelled"), cleanup_type("cleanup interrupted")
    wait = Mock(side_effect=cleanup)
    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        patch.setattr(process, "terminate", Mock())
        patch.setattr(process, "wait", wait)
        with pytest.raises(error_type) as caught:
            accept(process)
    assert caught.value is primary and caught.value.process is process
    refusal = caught.value.managed_launch_refusal
    assert refusal.primary is primary and refusal.cleanup_error is cleanup
    assert refusal.process is process and not refusal.cleanup_verified and refusal.exit_code is None
    wait.assert_called_once_with(timeout=2)
    assert not process.stdin.closed and not process.stdout.closed


@pytest.mark.parametrize("error_type", INTERRUPTIONS)
def test_cancellation_during_refusal_cleanup_is_not_wrapped(real_children, monkeypatch, error_type):
    process = real_children()
    primary, cleanup = OSError("proof refused"), error_type("cleanup interrupted")
    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        patch.setattr(process, "wait", Mock(side_effect=cleanup))
        with pytest.raises(error_type) as caught:
            accept(process)
    assert caught.value is cleanup
    refusal = cleanup.managed_launch_refusal
    assert refusal.primary is primary and refusal.cleanup_error is cleanup and refusal.process is process
    assert not refusal.cleanup_verified


@pytest.mark.parametrize("error_type", [OSError, *INTERRUPTIONS])
def test_unknown_proof_exit_never_flushes_or_closes_transferred_pipes(real_children, monkeypatch, error_type):
    process = real_children()
    primary, cleanup = error_type("proof refused"), OSError("wait unproven")
    close_input, close_output = Mock(), Mock()
    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        patch.setattr(process, "terminate", Mock())
        patch.setattr(process, "wait", Mock(side_effect=cleanup))
        patch.setattr(process.stdin, "close", close_input)
        patch.setattr(process.stdout, "close", close_output)
        with pytest.raises(BaseException) as caught:
            accept(process)
    refusal = caught.value if error_type is OSError else caught.value.managed_launch_refusal
    assert refusal.primary is primary and refusal.cleanup_error is cleanup and refusal.process is process
    assert not refusal.cleanup_verified and process.poll() is None
    close_input.assert_not_called()
    close_output.assert_not_called()


def bootstrap_rpc(calls, phase, primary):
    def rpc(self, method, params, **kwargs):
        calls.append((method, params))
        if method == phase or (phase == "task_response" and params.get("name") == "start_task"):
            raise primary
        if method == "initialize":
            return {
                "protocolVersion": "2025-06-18",
                "serverInfo": {"name": "dcc-cua-task-automation", "version": VERSION},
                "capabilities": {"tools": {}},
            }
        if method == "tools/list":
            return {"tools": tool_catalog()}
        assert method == "tools/call"
        raw = {
            "ok": True,
            "provider": "dcc-cua",
            "runtime_version": VERSION,
            "task_id": "public-task",
            "target": dict(TARGET),
        }
        if params["name"] == "start_task":
            raw["status"] = "started"
        else:
            assert params == {"name": "stop_task", "arguments": {"task_id": "public-task"}}
            raw.update(
                status="stopped",
                cleanup={
                    "type": "session_stopped",
                    "session_id": "mcp-public-task",
                    "success": True,
                    "active": False,
                    "cleanup_pending": False,
                },
            )
        return {"structuredContent": raw}

    return rpc


def interrupted_client(process, patch, phase, primary, *, managed_launch=True):
    executable = Path(sys.executable)
    config = UiControlRuntimeOptions(
        binary=str(executable), sha256=hashlib.sha256(executable.read_bytes()).hexdigest(), runtime_version=VERSION
    )
    identity = managed.OriginalProcessIdentity(process.pid, 1, str(executable), config.sha256)
    # Proof is mocked independently of the real pipes/process lifetime. The
    # stdin-only fixture is not an MCP executable or an independent launcher.
    patch.setattr(managed, "original_independent_identity", Mock(return_value=identity))
    calls = []
    patch.setattr(OwnedCuaMcpTransport, "rpc", bootstrap_rpc(calls, phase, primary))
    if phase == "reader_create":
        patch.setattr(transport.threading, "Thread", Mock(side_effect=primary))
    elif phase == "reader_start":
        patch.setattr(transport.threading.Thread, "start", Mock(side_effect=primary))
    elif phase == "target_validation":
        patch.setattr(PixelsMcpHostClient, "_check_target", Mock(side_effect=primary))
    launch = Mock(return_value=process)
    if not managed_launch:
        patch.setattr(transport.subprocess, "Popen", launch)
    with pytest.raises(type(primary)) as caught:
        PixelsMcpHostClient(
            session_id="cancelled",
            dcc_type="unreal",
            process_id=42,
            window_handle=500,
            allow_raw_input=False,
            options=config,
            **({"managed_launcher": launch} if managed_launch else {}),
        )
    assert caught.value is primary
    launch.assert_called_once()
    return calls, identity


@pytest.mark.parametrize("error_type", INTERRUPTIONS)
@pytest.mark.parametrize("managed_launch", [True, False])
@pytest.mark.parametrize(
    "phase", ["reader_create", "reader_start", "initialize", "tools/list", "task_response", "target_validation"]
)
def test_startup_cancellation_closes_owned_child(real_children, monkeypatch, error_type, managed_launch, phase):
    process = real_children()
    primary = error_type("startup cancelled")
    with monkeypatch.context() as patch:
        calls, _ = interrupted_client(process, patch, phase, primary, managed_launch=managed_launch)
    assert process.poll() == 0 and process.stdin.closed and process.stdout.closed
    if phase == "target_validation":
        assert [params["name"] for method, params in calls if method == "tools/call"] == ["start_task", "stop_task"]
    elif phase != "task_response":
        assert all(method != "tools/call" for method, _ in calls)


@pytest.mark.parametrize("error_type", INTERRUPTIONS)
@pytest.mark.parametrize("managed_launch", [True, False])
@pytest.mark.parametrize("phase", ["initialize", "target_validation"])
def test_unknown_startup_cleanup_retains_transport_or_task_owner(
    real_children, monkeypatch, error_type, managed_launch, phase
):
    process = real_children()
    primary = error_type("startup cancelled")
    cleanup = SystemExit("cleanup interrupted")
    with monkeypatch.context() as patch:
        patch.setattr(process.stdin, "close", Mock(side_effect=cleanup))
        _, identity = interrupted_client(process, patch, phase, primary, managed_launch=managed_launch)
        refusal = primary.managed_launch_refusal
        assert primary.process is process and refusal.process is process
        assert refusal.primary is primary and refusal.cleanup_error is cleanup
        assert refusal.identity is (identity if managed_launch else None)
        assert not refusal.cleanup_verified and refusal.exit_code is None and process.poll() is None
        owner = refusal.owner
        owned_transport = owner if phase == "initialize" else owner._transport
        assert owned_transport._process is process
        if phase == "target_validation":
            assert owner.task_id == "public-task"
        with pytest.raises(CuaCliError, match="unverified"):
            owned_transport.close()
    process.stdin.close()
    process.wait(timeout=5)
    owned_transport._reader.join(timeout=2)
    assert not owned_transport._reader.is_alive()


def test_concurrent_cancellations_only_settle_their_transferred_children(real_children, monkeypatch):
    children = [real_children(), real_children()]
    untouched = real_children()
    primary = {process.pid: KeyboardInterrupt("proof cancelled") for process in children}
    barrier = threading.Barrier(2)

    def proof(process, *args):
        barrier.wait(timeout=5)
        raise primary[process.pid]

    def attempt(process):
        try:
            accept(process)
        except KeyboardInterrupt as error:
            return error
        pytest.fail("The proof interruption did not propagate")

    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", proof)
        with ThreadPoolExecutor(max_workers=2) as pool:
            errors = list(pool.map(attempt, children))
    for process, error in zip(children, errors):
        assert error is primary[process.pid] and error.process is process
        assert error.managed_launch_refusal.cleanup_verified and process.poll() is not None
    assert untouched.poll() is None and not untouched.stdin.closed


def test_async_cancelled_error_keeps_task_cancelled(real_children, monkeypatch):
    process = real_children()
    primary = asyncio.CancelledError("proof cancelled")

    async def run():
        async def start():
            accept(process)

        task = asyncio.create_task(start())
        with pytest.raises(asyncio.CancelledError) as caught:
            await task
        assert task.cancelled() and caught.value is primary

    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        asyncio.run(run())
    assert primary.managed_launch_refusal.cleanup_verified and process.poll() is not None


@pytest.mark.parametrize("error_type", INTERRUPTIONS)
@pytest.mark.parametrize("unknown", [False, True])
@pytest.mark.parametrize("boundary", ["before_proof", "after_proof", "client_transfer"])
def test_interrupt_at_each_post_transfer_boundary_retains_or_settles_owner(
    real_children, monkeypatch, error_type, unknown, boundary
):
    process = real_children()
    primary = error_type("interrupted at ownership boundary")
    constructor = PixelsMcpHostClient.__init__ if boundary == "client_transfer" else OwnedCuaMcpTransport.__init__
    markers = {
        "before_proof": "self.managed_process_identity = accept_managed_process(",
        "after_proof": "self._reader = threading.Thread(",
        "client_transfer": "opened = self._transport.tool(",
    }
    lines, first = inspect.getsourcelines(constructor)
    line = first + next(index for index, value in enumerate(lines) if markers[boundary] in value)
    hit = []

    def trace(frame, event, arg):
        if frame.f_code is constructor.__code__ and event == "line" and frame.f_lineno == line:
            owner = frame.f_locals["self"]
            owned_transport = owner._transport if boundary == "client_transfer" else owner
            assert owned_transport._process is process
            hit.append(owner)
            raise primary
        return trace

    with monkeypatch.context() as patch:
        if unknown:
            patch.setattr(process.stdin, "close", Mock(side_effect=SystemExit("cleanup interrupted")))
        previous_trace = sys.gettrace()
        try:
            sys.settrace(trace)
            interrupted_client(process, patch, "transfer_gap", primary)
        finally:
            sys.settrace(previous_trace)
        assert len(hit) == 1
        assert managed.original_independent_identity.call_count == (0 if boundary == "before_proof" else 1)
        if unknown:
            refusal = primary.managed_launch_refusal
            assert refusal.process is process and refusal.owner is hit[0]
            assert not refusal.cleanup_verified and process.poll() is None
        else:
            assert process.poll() == 0 and process.stdin.closed and process.stdout.closed
    if unknown:
        owner = hit[0]
        owned_transport = owner._transport if boundary == "client_transfer" else owner
        process.stdin.close()
        process.wait(timeout=5)
        if owned_transport._reader is not None:
            owned_transport._reader.join(timeout=2)
            assert not owned_transport._reader.is_alive()


@pytest.mark.parametrize("error_type", [OSError, *INTERRUPTIONS])
@pytest.mark.parametrize("unknown", [False, True])
def test_transport_does_not_repeat_completed_verifier_cleanup(real_children, monkeypatch, error_type, unknown):
    process = real_children()
    primary = error_type("proof interrupted")
    executable = Path(sys.executable)
    config = UiControlRuntimeOptions(
        binary=str(executable), sha256=hashlib.sha256(executable.read_bytes()).hexdigest(), runtime_version=VERSION
    )
    terminate = Mock(wraps=process.terminate)
    wait = Mock(side_effect=OSError("wait unproven")) if unknown else Mock(wraps=process.wait)
    with monkeypatch.context() as patch:
        patch.setattr(managed, "original_independent_identity", Mock(side_effect=primary))
        patch.setattr(process, "terminate", terminate)
        patch.setattr(process, "wait", wait)
        close = Mock(wraps=OwnedCuaMcpTransport.close)
        patch.setattr(OwnedCuaMcpTransport, "close", close)
        with pytest.raises(BaseException) as caught:
            OwnedCuaMcpTransport(config, managed_launcher=lambda *_: process)
        close.assert_not_called()
    refusal = caught.value if error_type is OSError else caught.value.managed_launch_refusal
    assert refusal.primary is primary and refusal.process is process
    assert refusal.cleanup_verified is (not unknown)
    terminate.assert_called_once()
    wait.assert_called_once_with(timeout=2)
