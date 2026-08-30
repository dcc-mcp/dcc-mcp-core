"""Shared scene-digest provider contract for script execution (issue #2260)."""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core
from dcc_mcp_core.script_execution import SceneDigestError
from dcc_mcp_core.script_execution import SceneDigestExecutionError
from dcc_mcp_core.script_execution import ScriptExecutionContext
from dcc_mcp_core.script_execution import ScriptExecutionResult
from dcc_mcp_core.script_execution import capture_state_digest
from dcc_mcp_core.script_execution import execute_with_state_digest
from dcc_mcp_core.script_execution import register_state_digest_provider


def _stats(object_count: int, *, extra=None):
    return {
        "object_count": object_count,
        "vertex_count": object_count * 8,
        "has_mesh": object_count > 0,
        "extra": {} if extra is None else extra,
    }


def test_scene_digest_contract_is_top_level_exported() -> None:
    assert dcc_mcp_core.SceneDigestError is SceneDigestError
    assert dcc_mcp_core.capture_state_digest is capture_state_digest
    assert dcc_mcp_core.execute_with_state_digest is execute_with_state_digest
    assert dcc_mcp_core.register_state_digest_provider is register_state_digest_provider


def test_state_digest_provider_is_capability_gated_and_context_owned() -> None:
    maya = ScriptExecutionContext()
    blender = ScriptExecutionContext()
    register_state_digest_provider(lambda: _stats(1), context=maya)

    assert capture_state_digest(context=maya).payload["object_count"] == 1
    with pytest.raises(SceneDigestError) as exc_info:
        capture_state_digest(context=blender)
    assert exc_info.value.code == "scene_digest_provider_missing"


def test_state_digest_provider_fail_closed_errors() -> None:
    broken = ScriptExecutionContext()

    def raise_provider():
        raise RuntimeError("host query failed")

    register_state_digest_provider(raise_provider, context=broken)
    with pytest.raises(SceneDigestError) as exc_info:
        capture_state_digest(context=broken)
    assert exc_info.value.code == "scene_digest_provider_error"
    assert "host query failed" not in str(exc_info.value)

    malformed = ScriptExecutionContext()
    register_state_digest_provider(lambda: {"object_count": 1}, context=malformed)
    with pytest.raises(SceneDigestError) as exc_info:
        capture_state_digest(context=malformed)
    assert exc_info.value.code == "scene_digest_invalid"


def test_scene_digest_validates_core_field_types_and_ranges() -> None:
    invalid_values = [
        {"object_count": True, "vertex_count": 0, "has_mesh": False},
        {"object_count": -1, "vertex_count": 0, "has_mesh": False},
        {"object_count": 0, "vertex_count": "0", "has_mesh": False},
        {"object_count": 0, "vertex_count": 0, "has_mesh": 0},
    ]
    for value in invalid_values:
        context = ScriptExecutionContext()
        register_state_digest_provider(lambda value=value: value, context=context)
        with pytest.raises(SceneDigestError) as exc_info:
            capture_state_digest(context=context)
        assert exc_info.value.code == "scene_digest_invalid"


def test_scene_digest_is_redacted_bounded_and_deterministically_fingerprinted() -> None:
    left = ScriptExecutionContext()
    right = ScriptExecutionContext()
    first = _stats(
        2,
        extra={
            "label": "mesh",
            "api_token": "do-not-publish",
            "nested": {"password": "hidden", "note": "x" * 2_000},
            "many": {str(index): index for index in range(100)},
        },
    )
    second = {
        "extra": first["extra"],
        "has_mesh": True,
        "vertex_count": 16,
        "object_count": 2,
    }
    register_state_digest_provider(lambda: first, context=left)
    register_state_digest_provider(lambda: second, context=right)

    left_digest = capture_state_digest(context=left)
    right_digest = capture_state_digest(context=right)

    assert left_digest.fingerprint == right_digest.fingerprint
    serialized = json.dumps(left_digest.to_dict(), sort_keys=True)
    assert "do-not-publish" not in serialized
    assert "hidden" not in serialized
    assert len(serialized.encode("utf-8")) <= 8_192
    assert left_digest.truncated is True


@pytest.mark.parametrize("path", [r"\\server\share\scene.ma", r"\scene.ma", r"C:\scene.ma", "/tmp/scene.ma"])
def test_scene_digest_redacts_windows_and_posix_absolute_paths(path: str) -> None:
    context = ScriptExecutionContext()
    register_state_digest_provider(lambda: _stats(1, extra={"path": path}), context=context)

    digest = capture_state_digest(context=context)

    assert digest.payload["extra"]["path"] == "<redacted>"


def test_scene_digest_snapshot_and_envelope_are_deeply_detached() -> None:
    source = _stats(1, extra={"nested": {"labels": ["mesh"]}})
    context = ScriptExecutionContext()
    register_state_digest_provider(lambda: source, context=context)

    snapshot = capture_state_digest(context=context)
    rendered = snapshot.to_dict()
    rendered["payload"]["extra"]["nested"]["labels"].append("mutated")
    source["extra"]["nested"]["labels"].append("host mutation")

    assert snapshot.payload["extra"]["nested"]["labels"] == ["mesh"]
    snapshot.validate()

    result = ScriptExecutionResult.from_value(
        "ok",
        scene_digest_before=snapshot,
        scene_digest_after=snapshot,
    )
    result["postcondition"]["scene_digest_before"]["payload"]["extra"]["nested"]["labels"].append("wire mutation")
    assert snapshot.payload["extra"]["nested"]["labels"] == ["mesh"]


def test_contaminated_snapshot_fails_closed_through_validate_and_result_path() -> None:
    context = ScriptExecutionContext()
    register_state_digest_provider(lambda: _stats(1), context=context)
    snapshot = capture_state_digest(context=context)
    snapshot.payload["extra"] = {"poison": object()}

    with pytest.raises(SceneDigestError) as exc_info:
        snapshot.validate()
    assert exc_info.value.code == "scene_digest_invalid"

    result = ScriptExecutionResult.from_value(
        "ok",
        scene_digest_before=snapshot,
        scene_digest_after=snapshot,
    )
    assert result["success"] is False
    assert result["error"] == "scene_digest_invalid"


def test_contaminated_execute_outcome_fails_closed_on_from_outcome() -> None:
    context = ScriptExecutionContext()
    state = {"objects": 0}
    register_state_digest_provider(lambda: _stats(state["objects"]), context=context)
    context.register_dcc_namespace({"state": state})
    outcome = context.execute_with_state_digest("state['objects'] += 1; result = 'ok'")
    outcome.scene_digest_after.payload["extra"] = {"poison": object()}

    result = ScriptExecutionResult.from_outcome(outcome)

    assert result["success"] is False
    assert result["error"] == "scene_digest_invalid"


def test_state_digest_provider_can_be_unregistered() -> None:
    context = ScriptExecutionContext()
    register_state_digest_provider(lambda: _stats(1), context=context)
    context.register_state_digest_provider(None)
    with pytest.raises(SceneDigestError) as exc_info:
        capture_state_digest(context=context)
    assert exc_info.value.code == "scene_digest_provider_missing"


def test_scene_digest_rejects_provider_fingerprint_mismatch() -> None:
    context = ScriptExecutionContext()
    register_state_digest_provider(
        lambda: dict(_stats(1), fingerprint="sha256:" + "0" * 64),
        context=context,
    )

    with pytest.raises(SceneDigestError) as exc_info:
        capture_state_digest(context=context)
    assert exc_info.value.code == "scene_digest_fingerprint_mismatch"


def test_execute_with_state_digest_captures_before_and_after() -> None:
    context = ScriptExecutionContext()
    state = {"objects": 0}
    register_state_digest_provider(lambda: _stats(state["objects"]), context=context)
    context.register_dcc_namespace({"state": state})

    outcome = execute_with_state_digest(
        "state['objects'] += 1; result = 'created'",
        context=context,
    )

    assert outcome.value == "created"
    assert outcome.scene_digest_before.payload["object_count"] == 0
    assert outcome.scene_digest_after.payload["object_count"] == 1
    assert outcome.scene_digest_before.fingerprint != outcome.scene_digest_after.fingerprint


def test_execute_with_state_digest_preserves_evidence_when_script_raises() -> None:
    context = ScriptExecutionContext()
    state = {"objects": 0}
    register_state_digest_provider(lambda: _stats(state["objects"]), context=context)
    context.register_dcc_namespace({"state": state})

    with pytest.raises(SceneDigestExecutionError) as exc_info:
        execute_with_state_digest(
            "state['objects'] += 1; raise RuntimeError('script failed')",
            context=context,
        )

    failure = exc_info.value
    assert isinstance(failure.cause, RuntimeError)
    assert failure.scene_digest_before.payload["object_count"] == 0
    assert failure.scene_digest_after.payload["object_count"] == 1

    result = ScriptExecutionResult.from_exception(
        failure.cause,
        scene_digest_before=failure.scene_digest_before,
        scene_digest_after=failure.scene_digest_after,
    )
    assert result["success"] is False
    assert result["postcondition"]["verified"] is False
    assert result["postcondition"]["scene_digest_after"]["payload"]["object_count"] == 1


def test_script_result_attaches_bounded_digest_evidence_without_inventing_verification() -> None:
    context = ScriptExecutionContext()
    state = {"objects": 0}
    register_state_digest_provider(lambda: _stats(state["objects"]), context=context)
    context.register_dcc_namespace({"state": state})
    outcome = execute_with_state_digest("state['objects'] += 1; result = 7", context=context)

    result = ScriptExecutionResult.from_value(
        outcome.value,
        scene_digest_before=outcome.scene_digest_before,
        scene_digest_after=outcome.scene_digest_after,
    )

    evidence = result["postcondition"]
    assert evidence["verified"] is False
    assert evidence["scene_digest_before"]["fingerprint"].startswith("sha256:")
    assert evidence["scene_digest_after"]["payload"]["object_count"] == 1


def test_script_result_merges_digest_evidence_into_adapter_postcondition() -> None:
    context = ScriptExecutionContext()
    state = {"objects": 0}
    register_state_digest_provider(lambda: _stats(state["objects"]), context=context)
    context.register_dcc_namespace({"state": state})
    outcome = execute_with_state_digest("state['objects'] += 1; result = 7", context=context)

    result = ScriptExecutionResult.from_value(
        outcome.value,
        postcondition={"method": "object_count_readback", "verified": True},
        scene_digest_before=outcome.scene_digest_before,
        scene_digest_after=outcome.scene_digest_after,
    )

    assert result["success"] is True
    assert result["postcondition"]["method"] == "object_count_readback"
    assert result["postcondition"]["verified"] is True


def test_script_result_fails_closed_for_missing_or_unchanged_verified_digest() -> None:
    context = ScriptExecutionContext()
    register_state_digest_provider(lambda: _stats(1), context=context)
    digest = capture_state_digest(context=context)

    missing = ScriptExecutionResult.from_value(1, scene_digest_before=digest)
    unchanged = ScriptExecutionResult.from_value(
        1,
        scene_digest_before=digest,
        scene_digest_after=digest,
        verified=True,
    )

    assert missing["success"] is False
    assert missing["error"] == "scene_digest_evidence_missing"
    assert unchanged["success"] is False
    assert unchanged["error"] == "scene_digest_postcondition_mismatch"


def test_script_result_normalizes_mapping_snapshots_and_rejects_malformed_values() -> None:
    result = ScriptExecutionResult.from_value(
        1,
        scene_digest_before=_stats(0),
        scene_digest_after=_stats(1),
    )
    assert result["success"] is True
    assert result["postcondition"]["scene_digest_after"]["payload"]["object_count"] == 1

    malformed = ScriptExecutionResult.from_value(
        1,
        scene_digest_before={"object_count": 1},
        scene_digest_after=_stats(1),
    )
    assert malformed["success"] is False
    assert malformed["error"] == "scene_digest_invalid"
