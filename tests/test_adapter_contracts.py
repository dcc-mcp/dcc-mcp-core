"""Tests for Python adapter runtime observation contract helpers."""

from __future__ import annotations

import json

from dcc_mcp_core.adapter_contracts import DebugPathMapping
from dcc_mcp_core.adapter_contracts import DebugSessionDescriptor
from dcc_mcp_core.adapter_contracts import DebugSessionStatus
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionRequest
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiArtifactRef
from dcc_mcp_core.adapter_contracts import UiBounds
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot


def test_debug_session_descriptor_serializes_unavailable_guidance() -> None:
    descriptor = DebugSessionDescriptor.unavailable(
        "debugpy",
        "Install adapter debug support and restart the DCC.",
    )

    payload = descriptor.to_dict()

    assert payload["status"] == DebugSessionStatus.UNAVAILABLE
    assert "setup_instructions" in payload
    assert "host" not in payload
    json.dumps(payload)


def test_debug_session_descriptor_supports_path_mappings() -> None:
    descriptor = DebugSessionDescriptor.listening("native", "127.0.0.1", 9000)
    descriptor.path_mappings.append(
        DebugPathMapping(local_root="C:/show", remote_root="/mnt/show"),
    )

    payload = descriptor.to_dict()

    assert payload["status"] == DebugSessionStatus.LISTENING
    assert payload["path_mappings"][0]["remote_root"] == "/mnt/show"


def test_ui_snapshot_serializes_controls_and_metadata() -> None:
    button = UiControlNode(
        id="save",
        role="button",
        label="Save",
        bounds=UiBounds(x=1, y=2, width=80, height=24),
        metadata={"qt": {"class": "QPushButton"}},
    )
    snapshot = UiSnapshot(root=button, session_id="maya-session", focus_id="save")

    payload = snapshot.to_dict()

    assert payload["root"]["label"] == "Save"
    assert payload["root"]["metadata"]["qt"]["class"] == "QPushButton"
    assert "children" not in payload["root"]
    json.dumps(payload)


def test_ui_action_contracts_include_stale_error_and_artifacts() -> None:
    request = UiActionRequest(control_id="name-field", action=UiActionKind.SET_TEXT, text="hero")
    stale = UiActionResult.stale("old-button")
    ok = UiActionResult(
        success=True,
        control_id="save",
        artifacts=[UiArtifactRef(uri="artefact://sha256/abc", mime="image/png")],
    )

    assert request.to_dict()["action"] == "set_text"
    assert stale.to_dict()["error_code"] == UiErrorCode.STALE_CONTROL
    assert ok.to_dict()["artifacts"][0]["mime"] == "image/png"
