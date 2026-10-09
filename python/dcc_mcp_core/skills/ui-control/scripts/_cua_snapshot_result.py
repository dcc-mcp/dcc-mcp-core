"""Shared projection of a validated CUA capture into the canonical tool result."""

from __future__ import annotations

import base64
from typing import Any
from typing import Dict

from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.skill import skill_success


def render(
    capture: Dict[str, Any], session_id: str, policy: UiControlPolicy, params: Dict[str, Any]
) -> Dict[str, Any]:
    if not capture.get("success"):
        return capture
    accessibility_available = bool(capture.get("accessibility_available", True))
    pixels_only = capture.get("observation_mode") == "pixels_only"
    return skill_success(
        (
            "Captured scoped CUA application snapshot."
            if accessibility_available or pixels_only
            else "Captured screenshot-only CUA application observation."
        ),
        prompt=(
            "Inspect these pixels, perform one authorized physical ui_control__act with this snapshot_id, "
            "then take a fresh snapshot and verify the application state. Semantic controls are unavailable."
            if pixels_only
            else "Use ui_control__find or one scoped ui_control__act with this snapshot_id, then snapshot again."
            if accessibility_available
            else (
                "CUA accessibility was unavailable for this frame. Inspect the pixels, but do not act "
                "until a fresh snapshot returns accessibility_available=true."
            )
        ),
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        observation=capture["observation"],
        state_delta=capture.get("state_delta"),
        accessibility_available=accessibility_available,
        observation_mode=capture.get("observation_mode"),
        accessibility_state_id=capture.get("accessibility_state_id"),
        task_context=capture.get("task_context"),
        target=capture.get("target"),
        policy=policy.to_dict(),
        __rich__={
            "kind": "image",
            "data": base64.b64encode(capture["image"]).decode("ascii"),
            "mime": capture["mime_type"],
            "alt": "{} UI Control screenshot".format(params.get("app_name") or "DCC"),
        },
    )

