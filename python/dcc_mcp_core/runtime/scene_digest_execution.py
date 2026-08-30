"""Immutable scene-digest evidence helpers for script execution."""

from __future__ import annotations

from collections.abc import Mapping
import json
from typing import Any
from typing import Callable

from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot
from dcc_mcp_core.runtime.scene_digest import normalize_scene_digest


def freeze_scene_digest(snapshot: SceneDigestSnapshot) -> bytes:
    """Serialize trusted evidence before running untrusted script code."""
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


def restore_scene_digest(
    decoder: Callable[[Mapping[str, Any]], SceneDigestSnapshot], wire: bytes
) -> SceneDigestSnapshot:
    """Decode immutable evidence through a closure-bound validator."""
    try:
        decoded = json.loads(wire.decode("utf-8"))
    except (TypeError, UnicodeError, ValueError) as exc:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Serialized scene digest evidence is malformed",
        ) from exc
    if not isinstance(decoded, Mapping):
        raise SceneDigestError(
            "scene_digest_invalid",
            "Serialized scene digest evidence must be a mapping",
        )
    return decoder(decoded)


def scene_digest_restorer(wire: bytes) -> Callable[[], SceneDigestSnapshot]:
    """Bind immutable evidence to a closure with no mutable snapshot."""
    decoder = normalize_scene_digest

    def restore() -> SceneDigestSnapshot:
        return restore_scene_digest(decoder, wire)

    return restore
