"""Tests for Python adapter runtime observation contract helpers."""

from __future__ import annotations

import json

from dcc_mcp_core.adapter_contracts import AppUiAuditRecord
from dcc_mcp_core.adapter_contracts import AppUiPolicy
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
from dcc_mcp_core.adapter_contracts import UiWaitCondition
from dcc_mcp_core.adapter_contracts import UiWaitConditionKind
from dcc_mcp_core.adapter_contracts import UiWaitResult


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
    request = UiActionRequest(
        control_id="name-field",
        action=UiActionKind.SET_TEXT,
        text="hero",
        metadata={"snapshot_id": "session-1:1"},
    )
    stale = UiActionResult.stale("old-button")
    ok = UiActionResult(
        success=True,
        control_id="save",
        artifacts=[UiArtifactRef(uri="artefact://sha256/abc", mime="image/png")],
    )

    assert request.to_dict()["action"] == "set_text"
    assert request.to_dict()["metadata"]["snapshot_id"] == "session-1:1"
    assert stale.to_dict()["error_code"] == UiErrorCode.STALE_CONTROL
    assert ok.to_dict()["artifacts"][0]["mime"] == "image/png"


def test_app_ui_policy_blocks_high_risk_actions_by_default() -> None:
    policy = AppUiPolicy()

    assert policy.allows_action(UiActionKind.CLICK) is True
    assert policy.allows_action(UiActionKind.SET_TEXT) is True
    assert policy.allows_action(UiActionKind.RAW_COORDINATE_CLICK) is False
    assert policy.allows_action(UiActionKind.KEYBOARD_SHORTCUT) is False
    assert policy.to_dict()["allow_raw_coordinates"] is False


def test_app_ui_wait_result_and_audit_record_are_structured() -> None:
    condition = UiWaitCondition(
        kind=UiWaitConditionKind.TEXT_EQUALS,
        control_id="status",
        text="Applied",
        timeout_ms=250,
        interval_ms=25,
    )
    result = UiWaitResult(
        success=False,
        condition=condition,
        elapsed_ms=250.0,
        attempts=10,
        error_code=UiErrorCode.TIMEOUT,
        message="condition did not become true",
    )
    audit = AppUiAuditRecord(
        action_kind=UiActionKind.SET_TEXT,
        success=False,
        target_control_id="project-name",
        target_role="text_field",
        target_label="Project name",
        error_code=UiErrorCode.POLICY_DISABLED,
        redacted_fields=["text"],
    )

    assert result.to_dict()["condition"]["kind"] == "text_equals"
    assert result.to_dict()["error_code"] == "timeout"
    assert audit.to_dict()["error_code"] == "policy_disabled"
    assert audit.to_dict()["redacted_fields"] == ["text"]
