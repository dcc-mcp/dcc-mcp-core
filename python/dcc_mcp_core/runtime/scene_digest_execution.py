"""Immutable scene-digest evidence helpers for script execution."""

from __future__ import annotations

import json
from typing import Any

from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestExecution
from dcc_mcp_core.runtime.scene_digest import SceneDigestExecutionError
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot
from dcc_mcp_core.runtime.scene_digest import StateDigestProvider
from dcc_mcp_core.runtime.scene_digest import normalize_scene_digest
from dcc_mcp_core.runtime.scene_digest import snapshot_from_provider


def freeze_scene_digest(snapshot: SceneDigestSnapshot) -> bytes:
    """Serialize one validated provider observation before script execution."""
    try:
        rendered = snapshot.to_dict()
        return json.dumps(
            rendered,
            sort_keys=True,
            separators=(",", ":"),
            ensure_ascii=False,
            allow_nan=False,
        ).encode("utf-8")
    except SceneDigestError:
        raise
    except (TypeError, ValueError, UnicodeError) as exc:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Scene digest evidence could not be serialized",
        ) from exc


def capture_scene_digest_wire(provider: StateDigestProvider) -> bytes:
    """Capture and freeze one observation from a transaction-pinned provider."""
    return freeze_scene_digest(snapshot_from_provider(provider))


def thaw_scene_digest(wire: Any) -> SceneDigestSnapshot:
    """Rehydrate one native-custodied wire as the public snapshot type."""
    try:
        if not isinstance(wire, bytes):
            raise TypeError("scene digest wire must be bytes")
        source = json.loads(wire.decode("utf-8"))
        if not isinstance(source, dict):
            raise TypeError("scene digest wire must contain an object")
        return normalize_scene_digest(source)
    except SceneDigestError:
        raise
    except (TypeError, ValueError, UnicodeError) as exc:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Native scene digest evidence is malformed",
        ) from exc


def resolve_scene_digest_transaction(transaction: tuple[Any, Any, bytes, bytes | None, Any]) -> SceneDigestExecution:
    """Map one native transaction tuple into the stable public contract."""
    succeeded, value_or_error, before_wire, after_wire, readback_error = transaction
    before = thaw_scene_digest(before_wire)
    after = None if after_wire is None else thaw_scene_digest(after_wire)
    if readback_error is not None:
        if not isinstance(readback_error, SceneDigestError):
            readback_error = SceneDigestError(
                "scene_digest_provider_error",
                "The registered scene digest provider could not read host state",
            )
        cause = value_or_error if not succeeded else readback_error
        raise SceneDigestExecutionError(cause, before, None, readback_error=readback_error) from None
    if after is None:
        raise SceneDigestError(
            "scene_digest_custody_error",
            "Native scene digest transaction omitted its after-state evidence",
        )
    if not succeeded:
        if not isinstance(value_or_error, BaseException):
            raise SceneDigestError(
                "scene_digest_custody_error",
                "Native scene digest transaction returned an invalid script error",
            )
        raise SceneDigestExecutionError(value_or_error, before, after) from None
    return SceneDigestExecution(value_or_error, before, after)
