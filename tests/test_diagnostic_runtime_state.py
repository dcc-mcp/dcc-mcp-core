from __future__ import annotations

import sys
from types import SimpleNamespace

import dcc_mcp_core
from dcc_mcp_core._server.diagnostic_state import DiagnosticRuntimeState
from dcc_mcp_core.server_base import DccServerBase


def test_diagnostic_runtime_state_is_public() -> None:
    assert dcc_mcp_core.DiagnosticRuntimeState is DiagnosticRuntimeState
    assert "DiagnosticRuntimeState" in dcc_mcp_core.__all__
    assert callable(dcc_mcp_core.reset_default_diagnostic_state_for_tests)


def test_action_recorders_are_namespaced_per_state_and_dcc(monkeypatch) -> None:
    created: list[str] = []

    class FakeRecorder:
        def __init__(self, name: str) -> None:
            self.name = name
            created.append(name)

    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", SimpleNamespace(ToolRecorder=FakeRecorder))
    maya = DiagnosticRuntimeState("maya")
    blender = DiagnosticRuntimeState("blender")

    maya_recorder = maya.get_action_recorder()
    blender_recorder = blender.get_action_recorder()

    assert maya_recorder.name == "dcc-mcp-maya"
    assert blender_recorder.name == "dcc-mcp-blender"
    assert maya_recorder is not blender_recorder
    assert created == ["dcc-mcp-maya", "dcc-mcp-blender"]


def test_recorder_is_recreated_when_compatibility_state_changes_dcc(monkeypatch) -> None:
    class FakeRecorder:
        def __init__(self, name: str) -> None:
            self.name = name

    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", SimpleNamespace(ToolRecorder=FakeRecorder))
    state = DiagnosticRuntimeState("maya")

    first = state.get_action_recorder("maya")
    second = state.get_action_recorder("photoshop")

    assert first.name == "dcc-mcp-maya"
    assert second.name == "dcc-mcp-photoshop"
    assert first is not second


def test_reset_for_tests_preserves_context_identity_and_clears_collaborators() -> None:
    state = DiagnosticRuntimeState("maya")
    context = state.instance_context
    state.dispatcher = object()
    state.server = object()
    state.configure_instance(dcc_pid=42)

    state.reset_for_tests("zbrush")

    assert state.instance_context is context
    assert context["dcc_name"] == "zbrush"
    assert context["dcc_pid"] is None
    assert state.dispatcher is None
    assert state.server is None


def test_dcc_server_base_lazy_seam_is_instance_owned() -> None:
    maya = DccServerBase.__new__(DccServerBase)
    blender = DccServerBase.__new__(DccServerBase)
    maya._dcc_name = "maya"
    blender._dcc_name = "blender"

    maya_state = maya.diagnostic_state
    blender_state = blender.diagnostic_state

    assert maya_state is maya.diagnostic_state
    assert blender_state is blender.diagnostic_state
    assert maya_state is not blender_state
    assert maya_state.instance_context["dcc_name"] == "maya"
    assert blender_state.instance_context["dcc_name"] == "blender"
