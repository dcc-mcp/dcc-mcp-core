"""Every owned pixels task needs a real ACK, even without a recording grant."""

import subprocess

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import runtime


@pytest.mark.parametrize(
    "fault",
    [
        "missing",
        "old_runtime",
        "wrong_session",
        "active",
        "pending",
        "success_missing",
        "false_success",
        "failed",
        "unknown",
    ],
)
def test_default_pixels_cleanup_never_fabricates_an_ack_or_overwrites_failure(runtime, fault):
    config, process, _ = runtime
    owned = client(config)
    assert config.recording is None

    def mutate(raw):
        if fault == "missing":
            raw.pop("cleanup")
        elif fault == "old_runtime":
            raw.clear()
            raw.update(task_id="public-task", status="stopped")
        elif fault == "wrong_session":
            raw["cleanup"]["session_id"] = "native-runtime-session"
        elif fault == "active":
            raw["cleanup"]["active"] = True
        elif fault == "pending":
            raw["cleanup"]["cleanup_pending"] = True
        elif fault == "success_missing":
            raw["cleanup"].pop("success")
        elif fault == "false_success":
            raw["cleanup"]["success"] = False
        else:
            raw.update(ok=False, status="cleanup_" + fault)
            raw["cleanup"]["success"] = False

    process.mutate_stop = mutate
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as second:
        owned.stop()
    assert first.value is second.value
    expected = "cleanup_failed" if fault == "failed" else "cleanup_unknown"
    assert first.value.code == expected and first.value.native_evidence["cleanup_status"] == expected
    assert process.returncode == 0
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1


def test_default_stop_keeps_actual_ack_and_uses_five_second_budget(runtime, monkeypatch):
    config, process, _ = runtime
    owned = client(config)
    budgets = []
    original = owned._transport.rpc

    def rpc(method, params, *, timeout=None):
        budgets.append(timeout)
        return original(method, params, timeout=timeout)

    monkeypatch.setattr(owned._transport, "rpc", rpc)
    # No output constraint exists without an owner recording grant.
    process.mutate_stop = lambda raw: raw.update(recording_output_dir="not-a-recording-task")
    result = owned.stop()
    assert budgets == [5] and result["cleanup_status"] == "stopped"
    assert result["cleanup"] == {
        "type": "session_stopped",
        "session_id": "mcp-public-task",
        "success": True,
        "active": False,
        "cleanup_pending": False,
        "cleanup_issues": [],
    }
    result["cleanup"]["success"] = False
    assert owned.stop()["cleanup"]["success"] is True


@pytest.mark.parametrize("suffix", [" ", ".", "/.", "/./", "\x00", "\n", "\t"])
def test_owner_root_rejects_normalization_aliases_and_controls(tmp_path, suffix):
    from dcc_mcp_core.server import UiControlRecordingOptions

    with pytest.raises(ValueError):
        UiControlRecordingOptions(str(tmp_path) + suffix)


@pytest.mark.parametrize("value", [r"\\server\share\video.mp4", r"\\?\C:\video.mp4", r"\\.\C:\video.mp4"])
def test_ordinary_path_rejects_unc_and_device_names_without_reading_them(value):
    from dcc_mcp_core.host.ui_control_options import _ordinary_absolute_path

    with pytest.raises(ValueError, match="ordinary absolute local path"):
        _ordinary_absolute_path(value)


@pytest.mark.parametrize("last_failure", ["timeout", "oserror"])
def test_kill_wait_failure_is_typed_retained_and_always_drains_owned_pipes(runtime, last_failure):
    config, process, _ = runtime
    owned = client(config)
    operations = []
    waits = []
    process.stdin.close = lambda: operations.append("stdin_close")
    process.terminate = lambda: operations.append("terminate")

    def kill():
        operations.append("kill")
        # Release the in-memory response reader even though process exit is unknown.
        process.lines.put(b"")

    def wait(timeout):
        waits.append(timeout)
        if len(waits) == 3 and last_failure == "oserror":
            raise OSError("private process diagnostic must not escape")
        raise subprocess.TimeoutExpired("private command must not escape", timeout)

    process.kill = kill
    process.wait = wait
    process.stdout.close = lambda: operations.append("stdout_close")
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert first.value is repeated.value and first.value.code == "cleanup_failed"
    assert first.value.native_evidence["process_cleanup_failed"] is True
    assert owned._cleanup._stopped is None
    assert operations == ["stdin_close", "terminate", "kill", "stdout_close"]
    assert not owned._transport._reader.is_alive()
    assert len(waits) == 3 and waits[1:] == [2, 2]
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    assert "private" not in str(first.value)
    with pytest.raises(CuaCliError, match="forced process exit"):
        owned._transport.close()


@pytest.mark.parametrize("operation", ["terminate", "kill"])
def test_process_signal_failure_still_closes_reader_and_retains_failure(runtime, operation):
    config, process, _ = runtime
    owned = client(config)
    operations = []
    process.stdin.close = lambda: None

    def signal(name):
        operations.append(name)
        if name == "kill":
            process.lines.put(b"")
        if name == operation:
            raise OSError("private signal error")

    process.terminate = lambda: signal("terminate")
    process.kill = lambda: signal("kill")

    def wait(timeout):
        raise subprocess.TimeoutExpired("private process", timeout)

    process.wait = wait
    process.stdout.close = lambda: operations.append("stdout_close")
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert first.value is repeated.value and first.value.code == "cleanup_failed"
    assert operations == ["terminate", "kill", "stdout_close"]
    assert not owned._transport._reader.is_alive()
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1


def test_unexpected_close_failure_cannot_leave_a_cached_success(runtime, monkeypatch):
    config, process, _ = runtime
    owned = client(config)
    close = owned._transport.close

    def unexpected_close(**kwargs):
        close(**kwargs)
        raise RuntimeError("private implementation diagnostic")

    monkeypatch.setattr(owned._transport, "close", unexpected_close)
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert first.value is repeated.value and first.value.code == "cleanup_failed"
    assert owned._cleanup._stopped is None and process.returncode == 0
    assert "private" not in str(first.value)
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1


def test_alive_reader_never_closes_its_buffer_from_the_cleanup_thread(runtime, monkeypatch):
    config, process, _ = runtime
    owned = client(config)
    reader = owned._transport._reader
    join, is_alive, close_output = reader.join, reader.is_alive, process.stdout.close
    joins, signals, output_closes = [], [], []
    process.stdin.close = lambda: None
    process.terminate = lambda: signals.append("terminate")
    process.kill = lambda: signals.append("kill")

    def wait(timeout):
        raise subprocess.TimeoutExpired("private fake process", timeout)

    def unsafe_close():
        output_closes.append(True)
        raise AssertionError("cleanup must not wait for a reader-owned buffer lock")

    process.wait = wait
    process.stdout.close = unsafe_close
    monkeypatch.setattr(reader, "join", lambda timeout: joins.append(timeout))
    monkeypatch.setattr(reader, "is_alive", lambda: True)
    try:
        with pytest.raises(CuaCliError) as first:
            owned.stop()
        with pytest.raises(CuaCliError) as repeated:
            owned.stop()
        assert first.value is repeated.value and first.value.code == "cleanup_failed"
        assert joins == [1] and signals == ["terminate", "kill"] and not output_closes
        assert owned._cleanup._stopped is None
        assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
    finally:
        # Only the test releases the fake blocked reader, after the bounded
        # cleanup already returned failure. It is not an exit ACK.
        process.stdout.close = close_output
        monkeypatch.setattr(reader, "join", join)
        monkeypatch.setattr(reader, "is_alive", is_alive)
        process.lines.put(b"")
        join(timeout=1)
    assert not is_alive()


def test_reader_output_close_failure_is_typed_and_immutable(runtime):
    config, process, _ = runtime
    owned = client(config)

    def fail_close():
        raise OSError("private output-close diagnostic")

    process.stdout.close = fail_close
    with pytest.raises(CuaCliError) as first:
        owned.stop()
    with pytest.raises(CuaCliError) as repeated:
        owned.stop()
    assert first.value is repeated.value and first.value.code == "cleanup_failed"
    assert "output did not close cleanly" in str(first.value) and "private" not in str(first.value)
    assert len([r for r in process.requests if r.get("params", {}).get("name") == "stop_task"]) == 1
