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
from dcc_mcp_core.adapter_contracts import UiControlAuditRecord
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiPoint
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.adapter_contracts import UiWaitCondition
from dcc_mcp_core.adapter_contracts import UiWaitConditionKind
from dcc_mcp_core.adapter_contracts import UiWaitResult


def test_ui_point_is_available_from_the_top_level_package() -> None:
    import dcc_mcp_core

    assert dcc_mcp_core.UiPoint is UiPoint


def test_removed_app_ui_contract_aliases_are_not_exported() -> None:
    import dcc_mcp_core
    import dcc_mcp_core.adapter_contracts as contracts

    for removed_name in ("AppUiPolicy", "AppUiAuditRecord"):
        assert removed_name not in dcc_mcp_core.__all__
        assert removed_name not in contracts.__all__
        assert not hasattr(dcc_mcp_core, removed_name)
        assert not hasattr(contracts, removed_name)


def test_computer_use_error_codes_match_the_rust_wire_contract() -> None:
    assert {
        UiErrorCode.BACKEND_UNAVAILABLE,
        UiErrorCode.INVALID_ACTION,
        UiErrorCode.INPUT_FAILED,
        UiErrorCode.CAPTURE_FAILED,
    } == {
        "backend_unavailable",
        "invalid_action",
        "input_failed",
        "capture_failed",
    }


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
        "name-field",
        UiActionKind.SET_TEXT,
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


def test_ui_action_request_serializes_computer_use_inputs() -> None:
    request = UiActionRequest(
        control_id=None,
        action=UiActionKind.DRAG,
        x=10,
        y=20,
        button="left",
        scroll_x=0,
        scroll_y=-500,
        path=[UiPoint(x=10, y=20), UiPoint(x=80, y=90)],
        keys=["CTRL", "A"],
        snapshot_id="observation-1",
    )

    payload = request.to_dict()

    assert "control_id" not in payload
    assert payload["path"] == [{"x": 10, "y": 20}, {"x": 80, "y": 90}]
    assert payload["scroll_x"] == 0
    assert payload["scroll_y"] == -500
    assert payload["keys"] == ["CTRL", "A"]
    assert payload["snapshot_id"] == "observation-1"


def test_ui_control_policy_blocks_high_risk_actions_by_default() -> None:
    policy = UiControlPolicy()

    assert policy.allows_action(UiActionKind.CLICK) is True
    assert policy.allows_action(UiActionKind.SET_TEXT) is True
    assert policy.allows_action(UiActionKind.RAW_COORDINATE_CLICK) is False
    assert policy.allows_action(UiActionKind.KEYBOARD_SHORTCUT) is False
    assert policy.allows_action(UiActionKind.GET_WINDOW_STATE) is True
    assert policy.allows_action(UiActionKind.RESTORE_WINDOW) is True
    assert policy.require_scoped_window is True
    assert policy.to_dict()["allow_raw_coordinates"] is False
    assert policy.to_dict()["require_scoped_window"] is True


def test_ui_control_policy_requires_explicit_raw_input_for_computer_use_actions() -> None:
    policy = UiControlPolicy()

    for action in (
        UiActionKind.MOVE,
        UiActionKind.DOUBLE_CLICK,
        UiActionKind.SCROLL,
        UiActionKind.DRAG,
        UiActionKind.TYPE,
        UiActionKind.KEYPRESS,
    ):
        assert policy.allows_action(action) is False

    enabled = UiControlPolicy(
        allow_raw_coordinates=True,
        allow_keyboard_shortcuts=True,
    )
    for action in (
        UiActionKind.MOVE,
        UiActionKind.DOUBLE_CLICK,
        UiActionKind.SCROLL,
        UiActionKind.DRAG,
        UiActionKind.TYPE,
        UiActionKind.KEYPRESS,
    ):
        assert enabled.allows_action(action) is True

    keyboard_only = UiControlPolicy(
        allow_text_entry=False,
        allow_keyboard_shortcuts=True,
    )
    assert keyboard_only.allows_action(UiActionKind.KEYPRESS) is True
    assert keyboard_only.allows_action(UiActionKind.TYPE) is False


def test_ui_control_request_policy_can_only_narrow_the_runtime_ceiling() -> None:
    ceiling = UiControlPolicy(
        allow_raw_coordinates=False,
        allow_keyboard_shortcuts=False,
        audit_sensitive_values=False,
        allowed_process_ids=[42],
    )

    requested = ceiling.narrowed(
        {
            "allow_raw_coordinates": True,
            "allow_keyboard_shortcuts": True,
            "audit_sensitive_values": True,
            "require_scoped_window": False,
            "allowed_process_ids": [],
        }
    )

    assert requested.allow_raw_coordinates is False
    assert requested.allow_keyboard_shortcuts is False
    assert requested.audit_sensitive_values is False
    assert requested.require_scoped_window is True
    assert requested.allowed_process_ids == [42]

    denied = ceiling.narrowed(
        {
            "allowed_process_ids": [7],
        }
    )
    assert denied.scope_denied is True
    assert denied.to_dict()["scope_denied"] is True

    coordinate_click = UiActionRequest(
        control_id=None,
        action=UiActionKind.CLICK,
        x=10,
        y=20,
    )
    assert UiControlPolicy(allow_raw_coordinates=True, scope_denied=True).allows_request(coordinate_click) is False


def test_ui_control_wait_result_and_audit_record_are_structured() -> None:
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
    audit = UiControlAuditRecord(
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
