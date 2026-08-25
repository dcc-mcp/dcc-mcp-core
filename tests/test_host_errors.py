"""Cross-DCC startup and runtime host error capture tests."""

from __future__ import annotations

import json
import logging
from pathlib import Path
import subprocess
import sys
import threading

import pytest

from dcc_mcp_core.feedback import FeedbackStore
from dcc_mcp_core.host_errors import _HostErrorCapture
from dcc_mcp_core.host_errors import capture_bootstrap_errors
from dcc_mcp_core.schemas.finding import FindingRuntimeContext


def _payload(path: Path) -> dict:
    line = path.read_text(encoding="utf-8").splitlines()[-1]
    return json.loads(line.split(": ", 1)[1])


def test_bootstrap_error_is_persisted_and_reraised(tmp_path: Path) -> None:
    with pytest.raises(ImportError, match="UiControlAuditRecord"):
        with capture_bootstrap_errors(
            "3ds Max",
            adapter_version="0.3.0",
            min_core_version="0.18.0",
            log_dir=str(tmp_path),
        ):
            raise ImportError("cannot import name 'UiControlAuditRecord'")

    path = next(tmp_path.glob("dcc-mcp-3ds_Max.*.host-errors.log"))
    event = _payload(path)
    assert event["event"] == "dcc_host_error"
    assert event["dcc_type"] == "3ds Max"
    assert event["phase"] == "bootstrap"
    assert event["exception_type"] == "builtins.ImportError"
    assert "UiControlAuditRecord" in event["traceback"]
    assert event["adapter_version"] == "0.3.0"
    assert event["min_core_version"] == "0.18.0"
    assert event["python_executable"] == sys.executable


def test_bootstrap_helpers_do_not_load_native_core() -> None:
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            (
                "import sys; "
                "from dcc_mcp_core import capture_bootstrap_errors, record_bootstrap_error; "
                "assert capture_bootstrap_errors; assert record_bootstrap_error; "
                "assert 'dcc_mcp_core._core' not in sys.modules"
            ),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr


class _Output:
    resource_uri = "output://instance/photoshop-77"

    def __init__(self) -> None:
        self.entries: list[tuple[str, str]] = []

    def push(self, stream: str, text: str) -> None:
        self.entries.append((stream, text))


class _Events:
    resource_uri = "events://session/photoshop-77"

    def __init__(self) -> None:
        self.entries: list[dict] = []

    def append(self, **event: object) -> None:
        self.entries.append(dict(event))


def _capture(tmp_path: Path) -> tuple[_HostErrorCapture, _Output, _Events, list[str]]:
    output = _Output()
    events = _Events()
    notifications: list[str] = []
    capture = _HostErrorCapture(
        "photoshop",
        77,
        instance_id="photoshop-77",
        core_version="0.18.0",
        adapter_version="0.4.0",
        log_dir=str(tmp_path),
        output_capture=output,
        session_events=events,
        notify_updated=notifications.append,
    )
    return capture, output, events, notifications


def test_runtime_error_reuses_output_and_session_event_resources(tmp_path: Path) -> None:
    capture, output, events, notifications = _capture(tmp_path)
    try:
        raise RuntimeError("renderer failed")
    except RuntimeError as exc:
        event = capture.report_exception(
            type(exc),
            exc,
            exc.__traceback__,
            source="photoshop.console",
        )

    assert event["dcc_type"] == "photoshop"
    assert output.entries[0][0] == "stderr"
    assert "renderer failed" in output.entries[0][1]
    assert events.entries[0]["source"] == "photoshop.console"
    assert events.entries[0]["level"] == "error"
    assert events.entries[0]["metadata"]["phase"] == "runtime"
    assert notifications == [
        "output://instance/photoshop-77",
        "events://session/photoshop-77",
    ]
    assert _payload(next(tmp_path.glob("*.log")))["message"] == "renderer failed"


def test_startup_error_persists_needs_review_finding_without_request_id(tmp_path: Path) -> None:
    store = FeedbackStore(path=tmp_path / "feedback" / "photoshop-77.jsonl")
    capture = _HostErrorCapture(
        "photoshop",
        77,
        instance_id="photoshop-77",
        core_version="0.20.15",
        adapter_version="0.4.0",
        log_dir=str(tmp_path),
        finding_context=FindingRuntimeContext(
            dcc_type="photoshop",
            adapter="dcc-mcp-photoshop",
            adapter_version="0.4.0",
            core_version="0.20.15",
            host_version="26.4.1",
            os="win32",
            owning_repo="dcc-mcp/dcc-mcp-photoshop",
        ),
        feedback_store=store,
    )

    try:
        raise RuntimeError("adapter listener bind failed")
    except RuntimeError as exc:
        capture.report_exception(
            type(exc),
            exc,
            exc.__traceback__,
            source="dcc_server.start",
            phase="startup",
        )

    [finding] = store.recent()
    assert finding["schema_version"] == 1
    assert finding["phase"] == "startup"
    assert finding["severity"] == "blocker"
    assert finding["dcc_type"] == "photoshop"
    assert finding["adapter"] == "dcc-mcp-photoshop"
    assert finding["observed"] == "builtins.RuntimeError: adapter listener bind failed"
    assert finding["evidence"] == {
        "error_kind": "builtins.RuntimeError",
        "instance_id": "photoshop-77",
    }
    assert "request_id" not in finding["evidence"]
    assert finding["redaction_status"]["mode"] == "needs-review"
    assert (tmp_path / "feedback" / "photoshop-77.jsonl").is_file()


def test_runtime_error_does_not_create_startup_finding(tmp_path: Path) -> None:
    store = FeedbackStore(path=tmp_path / "feedback.jsonl")
    capture = _HostErrorCapture(
        "photoshop",
        77,
        instance_id="photoshop-77",
        core_version="0.20.15",
        adapter_version="0.4.0",
        log_dir=str(tmp_path),
        finding_context=FindingRuntimeContext(
            dcc_type="photoshop",
            adapter="dcc-mcp-photoshop",
            adapter_version="0.4.0",
            core_version="0.20.15",
            host_version="26.4.1",
            os="win32",
            owning_repo="dcc-mcp/dcc-mcp-photoshop",
        ),
        feedback_store=store,
    )

    capture.report("frame render failed", source="photoshop.console", phase="runtime")

    assert store.recent() == []
    assert not (tmp_path / "feedback.jsonl").exists()


def test_startup_finding_failure_does_not_mask_host_error(tmp_path: Path, caplog) -> None:
    class _FailingStore:
        def append(self, _entry: dict) -> None:
            raise OSError("feedback storage unavailable")

    capture = _HostErrorCapture(
        "photoshop",
        77,
        instance_id="photoshop-77",
        core_version="0.20.15",
        adapter_version="0.4.0",
        log_dir=str(tmp_path),
        finding_context=FindingRuntimeContext(
            dcc_type="photoshop",
            adapter="dcc-mcp-photoshop",
            adapter_version="0.4.0",
            core_version="0.20.15",
            host_version="26.4.1",
            os="win32",
            owning_repo="dcc-mcp/dcc-mcp-photoshop",
        ),
        feedback_store=_FailingStore(),
    )

    with caplog.at_level(logging.WARNING, logger="dcc_mcp_core.host_errors"):
        event = capture.report(
            "listener bind failed",
            source="dcc_server.start",
            phase="startup",
            exception_type="builtins.RuntimeError",
        )

    assert event["message"] == "listener bind failed"
    assert _payload(next(tmp_path.glob("*.log")))["message"] == "listener bind failed"
    assert "Could not persist startup Finding v1" in caplog.text


def test_startup_finding_uses_bounded_collision_resistant_error_identity(tmp_path: Path) -> None:
    context = FindingRuntimeContext(
        dcc_type="photoshop",
        adapter="dcc-mcp-photoshop",
        adapter_version="0.4.0",
        core_version="0.20.15",
        host_version="26.4.1",
        os="win32",
        owning_repo="dcc-mcp/dcc-mcp-photoshop",
    )
    shared_prefix = "AdapterStartupError" + "x" * 300
    first_type = type(shared_prefix + "A", (RuntimeError,), {"__module__": "dcc_adapter.bootstrap"})
    second_type = type(shared_prefix + "B", (RuntimeError,), {"__module__": "dcc_adapter.bootstrap"})

    def record(error_type: type, name: str) -> dict:
        store = FeedbackStore(path=tmp_path / f"{name}.jsonl")
        capture = _HostErrorCapture(
            "photoshop",
            77,
            instance_id="photoshop-77",
            core_version="0.20.15",
            adapter_version="0.4.0",
            log_dir=str(tmp_path),
            persist_to_file=False,
            finding_context=context,
            feedback_store=store,
        )
        try:
            raise error_type("listener bind failed")
        except error_type as exc:
            capture.report_exception(
                type(exc),
                exc,
                exc.__traceback__,
                source="dcc_server.start",
                phase="startup",
            )
        findings = store.recent()
        assert len(findings) == 1
        return findings[0]

    first = record(first_type, "first")
    repeated = record(first_type, "repeated")
    second = record(second_type, "second")

    first_error_kind = first["evidence"]["error_kind"]
    assert len(first_error_kind) <= 256
    assert first_error_kind == repeated["evidence"]["error_kind"]
    assert first["fingerprint"] == repeated["fingerprint"]
    assert first_error_kind != second["evidence"]["error_kind"]
    assert first["fingerprint"] != second["fingerprint"]

    class Alpha:
        class StartupFailure(RuntimeError):
            pass

    class Beta:
        class StartupFailure(RuntimeError):
            pass

    alpha = record(Alpha.StartupFailure, "alpha")
    alpha_repeated = record(Alpha.StartupFailure, "alpha-repeated")
    beta = record(Beta.StartupFailure, "beta")
    assert alpha["evidence"]["error_kind"] == alpha_repeated["evidence"]["error_kind"]
    assert alpha["fingerprint"] == alpha_repeated["fingerprint"]
    assert alpha["evidence"]["error_kind"] != beta["evidence"]["error_kind"]
    assert alpha["fingerprint"] != beta["fingerprint"]


def test_process_hooks_are_restored_and_capture_logging_errors(tmp_path: Path) -> None:
    capture, _, events, _ = _capture(tmp_path)
    previous_sys_hook = sys.excepthook
    previous_thread_hook = getattr(threading, "excepthook", None)
    capture.install()
    try:
        logging.getLogger("blender.render").error("frame 12 failed")
        assert events.entries[-1]["source"] == "blender.render"
        assert events.entries[-1]["message"] == "frame 12 failed"
    finally:
        capture.close()

    assert sys.excepthook is previous_sys_hook
    if previous_thread_hook is not None:
        assert threading.excepthook is previous_thread_hook
