"""Tests for the shared registration phase pipeline (PIP-689).

These exercise the host-agnostic core seam only: the phase base class,
the executor's ordering / error-handling contract, and the standard phase
list. They intentionally avoid importing ``DccServerBase`` so they stay
stable without a built ``dcc_mcp_core._core`` binary.
"""

from __future__ import annotations

import pytest

from dcc_mcp_core._registration import RegistrationContext
from dcc_mcp_core._registration import RegistrationPhase
from dcc_mcp_core._registration import get_standard_phases
from dcc_mcp_core._registration import run_registration_phases


class _RecordingPhase(RegistrationPhase):
    """A phase that records its invocation and can optionally fail."""

    def __init__(self, name: str, calls: list, fail: type[Exception] | None = None) -> None:
        self.name = name
        self._calls = calls
        self._fail = fail

    def run(self, context: RegistrationContext) -> None:
        self._calls.append(self.name)
        if self._fail is not None:
            raise self._fail("boom")


class _FatalPhase(_RecordingPhase):
    """A phase whose failures are fatal (abort the pipeline)."""

    fatal_exceptions = (ValueError,)


def test_get_standard_phases_is_ordered() -> None:
    names = [phase.name for phase in get_standard_phases()]
    assert names == [
        "core_builtin_actions",
        "strict_skill_scan",
        "metadata_driven_tools",
        "introspect_tools",
        "feedback_tool",
        "qt_ui_inspector",
        "capability_manifest",
        "project_tools",
        "resources",
        "skill_catalog_ready",
    ]


def test_phases_run_in_order_and_all_succeed() -> None:
    calls: list[str] = []
    context = RegistrationContext(server=object())

    report = run_registration_phases(
        [
            _RecordingPhase("one", calls),
            _RecordingPhase("two", calls),
            _RecordingPhase("three", calls),
        ],
        context,
    )

    assert calls == ["one", "two", "three"]
    assert [outcome.name for outcome in report.outcomes] == ["one", "two", "three"]
    assert all(outcome.success for outcome in report.outcomes)
    assert report.success is True


def test_non_fatal_failure_continues_and_is_reported() -> None:
    calls: list[str] = []
    context = RegistrationContext(server=object())

    report = run_registration_phases(
        [
            _RecordingPhase("one", calls),
            _RecordingPhase("two", calls, fail=RuntimeError),
            _RecordingPhase("three", calls),
        ],
        context,
    )

    # A non-fatal failure must not stop later phases.
    assert calls == ["one", "two", "three"]
    assert [outcome.success for outcome in report.outcomes] == [True, False, True]
    assert report.success is False
    assert report.outcomes[1].error == "boom"


def test_fatal_failure_aborts_pipeline() -> None:
    calls: list[str] = []
    context = RegistrationContext(server=object())

    with pytest.raises(ValueError):
        run_registration_phases(
            [
                _RecordingPhase("one", calls),
                _FatalPhase("two", calls, fail=ValueError),
                _RecordingPhase("three", calls),
            ],
            context,
        )

    # The fatal phase aborts before "three" runs.
    assert calls == ["one", "two"]


def test_host_hook_override_is_dispatched() -> None:
    """Verify that RegistrationPhase subclasses correctly dispatch to the server hooks."""

    class MockServer:
        def __init__(self):
            self.calls = []

        def _register_core_builtin_actions(self, context):
            self.calls.append("core")

        def _run_strict_skill_scan_phase(self, context):
            self.calls.append("strict")

        def _register_metadata_driven_tools(self, context):
            self.calls.append("metadata")

        def _register_introspect_tools(self):
            self.calls.append("introspect")

        def _register_feedback_tool(self):
            self.calls.append("feedback")

        def _register_qt_ui_inspector(self):
            self.calls.append("qt")

        def _register_capability_manifest_tool(self):
            self.calls.append("manifest")

        def _attach_project_tools(self):
            self.calls.append("project")

        def _attach_resources(self):
            self.calls.append("resources")

        def _mark_skill_catalog_ready(self):
            self.calls.append("ready")

    server = MockServer()
    context = RegistrationContext(server=server)
    phases = get_standard_phases()

    for phase in phases:
        phase.run(context)

    assert server.calls == [
        "core",
        "strict",
        "metadata",
        "introspect",
        "feedback",
        "qt",
        "manifest",
        "project",
        "resources",
        "ready",
    ]


def test_standard_phases_accept_real_dcc_server_base(monkeypatch: pytest.MonkeyPatch) -> None:
    """The default ``DccServerBase`` hooks must keep the phase dispatcher contract."""
    from dcc_mcp_core.server_base import DccServerBase

    class MinimalServer(DccServerBase):
        pass

    server = MinimalServer.__new__(MinimalServer)
    server._dcc_name = "maya"
    server._server = object()

    monkeypatch.setattr(DccServerBase, "register_builtin_actions", lambda self, **kwargs: None)
    monkeypatch.setattr(DccServerBase, "collect_skill_search_paths", lambda self, **kwargs: [])

    import dcc_mcp_core.feedback as feedback
    import dcc_mcp_core.introspect as introspect
    import dcc_mcp_core.metadata_registration as metadata_registration

    class _MetadataReport:
        ok = True
        registered_count = 0
        skipped_count = 0
        failed_count = 0

    monkeypatch.setattr(
        metadata_registration, "register_metadata_driven_tools", lambda *args, **kwargs: _MetadataReport()
    )
    monkeypatch.setattr(introspect, "register_introspect_tools", lambda *args, **kwargs: None)
    monkeypatch.setattr(feedback, "register_feedback_tool", lambda *args, **kwargs: None)

    report = run_registration_phases(get_standard_phases(), RegistrationContext(server=server))

    assert report.success is True
    assert [outcome.success for outcome in report.outcomes] == [True] * len(report.outcomes)
