"""Immutable scene-digest evidence helpers for script execution."""

from __future__ import annotations

import json

from dcc_mcp_core.runtime.scene_digest import SceneDigestError
from dcc_mcp_core.runtime.scene_digest import SceneDigestSnapshot


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
