"""Python surface regression tests for readiness bits (#1158)."""

from __future__ import annotations

import dcc_mcp_core


def test_readiness_probe_reports_bridge_bits() -> None:
    probe = dcc_mcp_core.ReadinessProbe()

    report = probe.report()
    assert report["process"] is True
    assert report["dcc"] is False
    assert report["skill_catalog"] is True
    assert report["dispatcher"] is False
    assert report["host_execution_bridge"] is False
    assert report["main_thread_executor"] is False
    assert probe.is_ready() is False

    probe.set_dispatcher_ready(True)
    probe.set_dcc_ready(True)
    assert probe.is_ready() is True

    probe.set_host_execution_bridge_ready(True)
    probe.set_main_thread_executor_ready(True)
    report = probe.report()
    assert report["host_execution_bridge"] is True
    assert report["main_thread_executor"] is True


def test_fully_ready_probe_marks_all_bits_green() -> None:
    report = dcc_mcp_core.ReadinessProbe.fully_ready().report()

    assert all(report.values())
