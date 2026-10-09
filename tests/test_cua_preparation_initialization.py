"""Native asynchronous pending DTOs retain cleanup without granting capture."""

from copy import deepcopy

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from test_cua_capture_preparation import begin
from test_cua_capture_preparation import route as route
from test_cua_capture_preparation import runtime as runtime
from test_cua_capture_preparation import status
from test_cua_mcp_pixels import client


def pending(root, *, cached=False):
    value = status(root, phase="pending_promotion")
    value.update(
        original=[],
        affected_readback=[],
        pending_sequence=1,
        last_completed_sequence=None,
        capture_revoked=False,
        journal_path="" if cached else value["journal_path"],
    )
    return value


@pytest.mark.parametrize("cached", [False, True])
def test_pending_then_initialized_same_task_allows_passive_capture(runtime, cached):
    config, process, _ = runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]
    process.mutate_preparation = lambda raw: raw.update(result=pending(process.root, cached=cached))
    first = owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    assert first["result"]["original"] == []
    with pytest.raises(CuaCliError, match="active"):
        owned.preparation.call("snapshot")
    initialized = status(process.root, phase="pending_promotion")
    initialized.update(pending_sequence=1, last_completed_sequence=None, affected_readback=[], capture_revoked=False)
    process.mutate_preparation = lambda raw: raw.update(result=deepcopy(initialized))
    assert len(owned.preparation.call("state")["result"]["original"]) == 1
    process.mutate_preparation = lambda raw: None
    ready = owned.preparation.call("state")["result"]
    assert ready["phase"] == "active" and len(ready["original"]) == 1
    assert owned.preparation.call("snapshot")["input_authorized"] is False
    assert owned.preparation.call("stop")["result"]["cleanup_verified"]
    owned.stop()


@pytest.mark.parametrize("fault", ["identity", "preparation_id", "deadline_ms", "journal_path"])
def test_first_initialization_keeps_original_task_binding(runtime, fault):
    config, process, _ = runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]
    process.mutate_preparation = lambda raw: raw.update(result=pending(process.root, cached=True))
    owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    value = status(process.root)
    if fault == "identity":
        value["original"][0]["identity"]["native_instance"]["process_creation_time_100ns"] += 1
    elif fault == "preparation_id":
        value[fault] = [8] * 16
    elif fault == "deadline_ms":
        value[fault] += 1
    else:
        value[fault] = str(process.root.parent / "escaped.json")
    with pytest.raises(CuaCliError):
        owned.preparation.validate(value)
    process.mutate_preparation = lambda raw: None
    owned.stop()


def test_worker_loss_before_initialization_remains_unknown(runtime):
    config, process, _ = runtime
    owned = client(config)
    value = pending(process.root)
    value.update(phase="cleanup_unknown", capture_revoked=True, failure={"reason": "worker_lost", "os_error": None})
    result = owned.preparation.validate(value)
    assert result["phase"] == "cleanup_unknown" and not result["cleanup_verified"]
    owned.stop()


def test_formal_route_keeps_pending_stop_unverified_until_no_write_settlement(route):
    call, process, _, _ = route
    value = pending(process.root, cached=True)
    process.mutate_preparation = lambda raw: raw.update(result=deepcopy(value))
    assert begin(call)["success"] is True
    value["capture_revoked"] = True
    stopped = call("stop")
    assert not stopped["success"] and stopped["context"]["cleanup_pending"]
    value.update(
        phase="refused",
        pending_sequence=None,
        cleanup_verified=False,
        failure={"reason": "readback_failed", "os_error": None},
        journal_path=str(process.root / "epoch" / "journal.json"),
    )
    assert not call("stop")["success"]
    value["cleanup_verified"] = True
    settled = call("stop")
    assert settled["success"] and settled["context"]["capture_preparation"]["original"] == []
    process.mutate_stop = lambda raw: raw["cleanup"].update(capture_preparation=deepcopy(value))


@pytest.mark.parametrize(
    "fault", ["active", "restore_pending", "restored", "readback", "mutation", "completed", "verified"]
)
def test_empty_original_never_proves_capture_or_mutation(runtime, fault):
    config, process, _ = runtime
    owned = client(config)
    value = pending(process.root)
    if fault in {"active", "restore_pending", "restored"}:
        value.update(phase=fault, pending_sequence=None)
    elif fault == "readback":
        value["affected_readback"] = status(process.root)["original"]
    elif fault == "mutation":
        value["last_mutation"] = {}
    elif fault == "completed":
        value["last_completed_sequence"] = 1
    else:
        value["cleanup_verified"] = True
    with pytest.raises(CuaCliError):
        owned.preparation.validate(value)
    owned.stop()


@pytest.mark.parametrize("fault", ["forget", "identity", "original", "journal"])
def test_initialized_original_and_journal_cannot_change(runtime, fault):
    config, process, _ = runtime
    owned = client(config)
    token = owned.window_state()["state"]["window_state_id"]
    owned.preparation.call("begin", window_state_id=token, lifetime_ms=1000)
    value = status(process.root)
    if fault == "forget":
        value = pending(process.root)
    elif fault == "identity":
        value["original"][0]["identity"]["window_handle"] += 1
    elif fault == "original":
        value["original"][0]["topmost"] = True
    else:
        value["journal_path"] = str(process.root / "different" / "journal.json")
    with pytest.raises(CuaCliError):
        owned.preparation.validate(value)
    owned.stop()
