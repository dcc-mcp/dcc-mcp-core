"""Canonical result-envelope integration for scene-digest evidence."""

from __future__ import annotations

from collections.abc import Mapping
import traceback
from typing import Any

from dcc_mcp_core.result_envelope import ToolResultEnvelope
from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot
from dcc_mcp_core.runtime.scene_digest import normalize_scene_digest
from dcc_mcp_core.verifier import SceneStats

_ENVELOPE_KEYS = frozenset({"success", "message", "error", "prompt", "context", "postcondition", "_meta"})


def scene_digest_postcondition(
    before: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    after: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    *,
    postcondition: Mapping[str, Any] | None,
    verified: bool | None,
) -> dict[str, Any] | None:
    """Build postcondition evidence from before/after host observations.

    A one-sided or malformed observation becomes a structured failure.  A
    changed digest is evidence that host state changed, but ``verified=True``
    still requires an adapter-owned semantic readback and is rejected when the
    digest is unchanged.
    """
    evidence = dict(postcondition) if postcondition is not None else {}
    if verified is not None:
        existing = evidence.get("verified")
        if existing is not None and existing is not verified:
            raise ValueError("'verified' conflicts with postcondition verification evidence")
        evidence["verified"] = verified
    collisions = sorted(_ENVELOPE_KEYS.intersection(evidence))
    if collisions:
        return ToolResultEnvelope.fail(
            "Scene digest postcondition uses reserved envelope keys",
            error="invalid_scene_digest_postcondition",
            reserved_keys=collisions,
        ).to_dict()
    if before is None and after is None:
        if evidence.get("verified") is True:
            return ToolResultEnvelope.fail(
                "Verified scene execution requires both before and after digest observations",
                error="scene_digest_evidence_missing",
            ).to_dict()
        return evidence or None
    if before is None or after is None:
        return ToolResultEnvelope.fail(
            "Scene digest evidence requires both before and after observations",
            error="scene_digest_evidence_missing",
        ).to_dict()
    try:
        before_snapshot = _coerce_scene_digest_snapshot(before)
        after_snapshot = _coerce_scene_digest_snapshot(after)
    except SceneDigestError as exc:
        return ToolResultEnvelope.fail(str(exc), error=exc.code).to_dict()
    if evidence.get("verified") is True and before_snapshot.fingerprint == after_snapshot.fingerprint:
        return ToolResultEnvelope.fail(
            "Verified scene mutation did not change the observed scene digest",
            error="scene_digest_postcondition_mismatch",
        ).to_dict()
    evidence.setdefault("verified", False)
    try:
        evidence["scene_digest_before"] = before_snapshot.to_dict()
        evidence["scene_digest_after"] = after_snapshot.to_dict()
    except SceneDigestError as exc:
        return ToolResultEnvelope.fail(str(exc), error=exc.code).to_dict()
    return evidence


def _coerce_scene_digest_snapshot(
    value: SceneDigestSnapshot | Mapping[str, Any] | SceneStats,
) -> SceneDigestSnapshot:
    """Normalize an envelope snapshot and preserve fail-closed semantics."""
    if isinstance(value, SceneDigestSnapshot):
        value.validate()
        return value
    return normalize_scene_digest(value)


def partial_scene_digest_postcondition(
    before: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    after: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    *,
    readback_error: SceneDigestError | None = None,
) -> dict[str, Any] | None:
    """Build indeterminate evidence when only one side was observable."""
    if before is None and after is None:
        return None
    evidence: dict[str, Any] = {
        "verified": False,
        "indeterminate": True,
        "scene_digest_readback": "unavailable",
    }
    if readback_error is not None:
        evidence["scene_digest_error"] = readback_error.code
    try:
        evidence["scene_digest_before"] = None if before is None else _coerce_scene_digest_snapshot(before).to_dict()
        evidence["scene_digest_after"] = None if after is None else _coerce_scene_digest_snapshot(after).to_dict()
    except SceneDigestError as exc:
        return ToolResultEnvelope.fail(str(exc), error=exc.code).to_dict()
    return evidence


def script_execution_failure(
    exc: BaseException,
    *,
    stdout: str,
    stderr: str,
    message: str | None,
    scene_digest_before: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    scene_digest_after: SceneDigestSnapshot | Mapping[str, Any] | SceneStats | None,
    readback_error: SceneDigestError | None = None,
) -> dict[str, Any]:
    """Build a failure envelope while retaining complete or partial evidence."""
    error_type = type(exc).__name__
    formatted_traceback = "".join(traceback.format_exception(type(exc), exc, exc.__traceback__))
    error_code = exc.code if isinstance(exc, SceneDigestError) else "script_execution_error"
    result = ToolResultEnvelope.fail(
        message or f"Script execution failed: {exc}",
        error=error_code,
        _meta={
            "dcc.error": {
                "type": error_type,
                "message": str(exc),
                "traceback": formatted_traceback,
            }
        },
        stdout=stdout,
        stderr=stderr,
        exception_type=error_type,
        exception_message=str(exc),
        traceback=formatted_traceback,
    ).to_dict()
    if scene_digest_before is None and scene_digest_after is None:
        evidence = scene_digest_postcondition(
            None,
            None,
            postcondition={"verified": False},
            verified=None,
        )
    elif scene_digest_before is None or scene_digest_after is None:
        evidence = partial_scene_digest_postcondition(
            scene_digest_before,
            scene_digest_after,
            readback_error=readback_error,
        )
    else:
        evidence = scene_digest_postcondition(
            scene_digest_before,
            scene_digest_after,
            postcondition={"verified": False},
            verified=None,
        )
    if isinstance(evidence, dict) and evidence.get("success") is False:
        return evidence
    if evidence is not None:
        result["postcondition"] = evidence
    return result


__all__ = ["partial_scene_digest_postcondition", "scene_digest_postcondition", "script_execution_failure"]
