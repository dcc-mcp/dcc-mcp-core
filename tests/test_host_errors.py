"""Cross-DCC startup and runtime host error capture tests."""

from __future__ import annotations

import json
import logging
from pathlib import Path
import subprocess
import sys
import threading

import pytest

from dcc_mcp_core.host_errors import _HostErrorCapture
from dcc_mcp_core.host_errors import capture_bootstrap_errors


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
