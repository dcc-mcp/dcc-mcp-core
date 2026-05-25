"""Regression tests for adapter readiness binding (#1206)."""

from __future__ import annotations

import json
from pathlib import Path
import threading
import time
import urllib.error
import urllib.request

from dcc_mcp_core import AdapterReadinessBinder
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core import readiness_report_subset
from dcc_mcp_core._testing import make_test_server
from dcc_mcp_core.host import QueueDispatcher


class _FakeInnerServer:
    def __init__(self) -> None:
        self.readiness_probe = None

    def set_readiness_probe(self, probe) -> None:
        self.readiness_probe = probe


def _make_base_server(tmp_path: Path):
    skills_dir = tmp_path / "skills"
    skills_dir.mkdir()
    return make_test_server(
        server=_FakeInnerServer(),
        dcc_name="fake-dcc",
        _builtin_skills_dir=skills_dir,
        _handle=None,
        _enable_gateway_failover=False,
        _hot_reloader=None,
        _gateway_election=None,
        _config=object(),
        _enable_telemetry=False,
        _enable_file_logging=False,
        _enable_job_persistence=False,
    )


def _get_json(url: str) -> tuple[int, dict]:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        return exc.code, json.loads(exc.read().decode("utf-8"))


def _remove_suffix(value: str, suffix: str) -> str:
    if suffix and value.endswith(suffix):
        return value[: -len(suffix)]
    return value


def test_inline_binder_publishes_probe_and_marks_bits_ready(tmp_path: Path) -> None:
    server = _make_base_server(tmp_path)

    binder = AdapterReadinessBinder.bind_inline(server, dcc_ready_probe=lambda: True)

    assert binder.published is True
    assert server._server.readiness_probe is binder.probe
    assert binder.report_subset() == {
        "process": True,
        "dcc": True,
        "skill_catalog": True,
        "dispatcher": True,
        "host_execution_bridge": True,
        "main_thread_executor": True,
    }


def test_queue_dispatcher_waits_for_first_pump_before_dcc_ready(tmp_path: Path) -> None:
    server = _make_base_server(tmp_path)
    dispatcher = QueueDispatcher()

    def pump_once() -> None:
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if dispatcher.has_pending():
                dispatcher.tick(16)
                return
            time.sleep(0.01)

    pump_thread = threading.Thread(target=pump_once)
    pump_thread.start()
    binder = AdapterReadinessBinder.bind_queue_dispatcher(
        server,
        dispatcher,
        dcc_ready_probe=lambda: True,
        require_first_pump=True,
        first_pump_timeout_secs=2.0,
    )
    pump_thread.join(timeout=2.0)

    assert binder.first_pump_observed is True
    assert binder.report_subset() == {
        "process": True,
        "dcc": True,
        "skill_catalog": True,
        "dispatcher": True,
        "host_execution_bridge": True,
        "main_thread_executor": True,
    }


def test_never_pumping_dispatcher_stays_not_ready(tmp_path: Path) -> None:
    server = _make_base_server(tmp_path)
    dispatcher = QueueDispatcher()

    binder = AdapterReadinessBinder.bind_queue_dispatcher(
        server,
        dispatcher,
        dcc_ready_probe=lambda: True,
        require_first_pump=True,
        first_pump_timeout_secs=0.01,
    )

    assert binder.first_pump_observed is False
    assert binder.report_subset() == {
        "process": True,
        "dcc": False,
        "skill_catalog": True,
        "dispatcher": True,
        "host_execution_bridge": True,
        "main_thread_executor": False,
    }


def test_readiness_report_subset_ignores_future_bits() -> None:
    report = {"process": True, "dcc": False, "dispatcher": True, "future_bit": True}

    assert readiness_report_subset(report, keys=("process", "dcc")) == {
        "process": True,
        "dcc": False,
    }


def test_binder_probe_controls_readyz_route() -> None:
    state = {"dcc_ready": False}
    server = McpHttpServer(ToolRegistry(), McpHttpConfig(port=0, server_name="readiness-binder-test"))
    binder = AdapterReadinessBinder.bind_inline(server, dcc_ready_probe=lambda: state["dcc_ready"])
    handle = server.start()
    try:
        base_url = _remove_suffix(handle.mcp_url(), "/mcp")
        status, payload = _get_json(f"{base_url}/v1/readyz")
        assert status == 503
        assert payload["dcc"] is False
        assert payload["dispatcher"] is True

        state["dcc_ready"] = True
        binder.refresh_dcc_ready()
        status, payload = _get_json(f"{base_url}/v1/readyz")
        assert status == 200
        assert payload["dcc"] is True
    finally:
        handle.shutdown()
