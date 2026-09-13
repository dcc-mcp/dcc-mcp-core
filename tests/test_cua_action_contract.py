"""Exercise Core's outgoing Host contract without sending desktop input."""

import pytest

from test_cua_cli_host_client import _BACKEND_PATH
from test_cua_cli_host_client import _CLIENT_PATH
from test_cua_cli_host_client import FakeBridge
from test_cua_cli_host_client import _load


@pytest.mark.parametrize("action", ["click", "double_click", "drag", "keypress"])
@pytest.mark.parametrize("intent", ["navigate", "ordinary_edit", "delete_or_overwrite"])
def test_native_action_declares_foreground_without_lowering_intent(action, intent):
    backend = _load(_BACKEND_PATH, "_test_foreground_contract")
    params = {"action": action, "intent": intent, "x": 24, "y": 18}
    if action == "drag":
        params["path"] = [{"x": 24, "y": 18}, {"x": 30, "y": 25}]
    if action == "keypress":
        params = {"action": action, "intent": intent, "keys": ["Tab"]}
    payload = backend._action_payload(params, True)
    assert payload["delivery_mode"] == "foreground"
    assert payload["intent"] == intent
    assert payload["input_kind"] == "raw_input"
    assert "approved" not in payload
    assert "confirmed" not in payload


def test_semantic_action_keeps_host_control_delivery_policy():
    backend = _load(_BACKEND_PATH, "_test_semantic_delivery_contract")
    payload = backend._action_payload({"action": "click"}, False)
    assert "delivery_mode" not in payload
    assert payload["input_kind"] == "semantic"


@pytest.mark.parametrize("dcc", ["capcut", "blender"])
@pytest.mark.parametrize("allow_mutation", [False, True])
def test_confirmation_permission_preserves_operator_scope(dcc, allow_mutation):
    module = _load(_CLIENT_PATH, "_test_confirmation_permission")
    bridge = FakeBridge()
    module.UiControlHostClient(
        session_id=dcc,
        task_grant_id="grant",
        dcc_type=dcc,
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        allow_menu_invoke=allow_mutation,
        bridge=bridge,
    )
    grant = bridge.calls[0][1]["grant"]
    assert grant["allow_trusted_confirmation"] is allow_mutation
    assert grant["allow_raw_input"] is False
    assert (grant["process_id"], grant["window_handle"]) == (42, 500)
    assert [method for method, _ in bridge.calls] == ["open_session"]
