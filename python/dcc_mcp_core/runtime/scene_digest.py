"""Bounded, redacted script-runtime scene digest contracts (issue #2260)."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import math
import re
from typing import Any
from typing import Mapping

from dcc_mcp_core._typing import Protocol
from dcc_mcp_core.errors import DccMcpError
from dcc_mcp_core.verifier import SceneStats

SCENE_DIGEST_SCHEMA_VERSION = "dcc-mcp.scene-digest.v1"
_MAX_DEPTH = 4
_MAX_ITEMS = 16
_MAX_STRING_CHARS = 128
_MAX_PAYLOAD_BYTES = 4_096
_SENSITIVE_KEY = re.compile(
    r"(?:api[_-]?key|authorization|credential|password|secret|token|file[_-]?path|path)",
    re.IGNORECASE,
)
# Cover drive-qualified, UNC, and rooted current-drive Windows paths as well
# as POSIX paths.  Relative paths are intentionally left intact because they
# are not independently identifiable as host-local locations.
_ABSOLUTE_PATH = re.compile(r"^(?:[A-Za-z]:[\\/]|[/\\]{1,2})")


class StateDigestProvider(Protocol):
    """Adapter callback that reads cheap host-owned scene statistics."""

    def __call__(self) -> Mapping[str, Any] | SceneStats:
        """Return the current :class:`SceneStats`-compatible state."""
        ...


class SceneDigestError(DccMcpError):
    """Fail-closed scene-digest contract error with a stable public code."""

    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code


class SceneDigestExecutionError(DccMcpError):
    """Script exception plus state captured on both sides of the attempt."""

    def __init__(
        self,
        cause: Exception,
        scene_digest_before: SceneDigestSnapshot,
        scene_digest_after: SceneDigestSnapshot,
    ) -> None:
        super().__init__("Script execution failed after scene digest capture")
        self.cause = cause
        self.scene_digest_before = scene_digest_before
        self.scene_digest_after = scene_digest_after


@dataclass(frozen=True)
class SceneDigestSnapshot:
    """Canonical bounded scene state plus its deterministic fingerprint."""

    payload: dict[str, Any]
    fingerprint: str
    truncated: bool = False
    schema_version: str = SCENE_DIGEST_SCHEMA_VERSION

    def to_dict(self) -> dict[str, Any]:
        """Return the adapter-facing JSON-safe evidence shape."""
        return {
            "schema_version": self.schema_version,
            "fingerprint": self.fingerprint,
            # Re-parse canonical JSON rather than returning a shallow copy.
            # ``extra`` may contain nested mappings/lists; a shallow copy
            # would let a caller mutate the snapshot that was used to compute
            # ``fingerprint`` after it crossed the host trust boundary.
            "payload": json.loads(_canonical_json(self.payload)),
            "truncated": self.truncated,
        }

    def validate(self) -> None:
        """Reject corrupted or mismatched evidence before envelope attachment."""
        normalized = normalize_scene_digest(self.payload)
        expected = _fingerprint(self.payload)
        if (
            self.schema_version != SCENE_DIGEST_SCHEMA_VERSION
            or self.fingerprint != expected
            or normalized.payload != self.payload
        ):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest evidence does not match its canonical fingerprint",
            )


@dataclass(frozen=True)
class SceneDigestExecution:
    """A script return value and the state observed immediately around it."""

    value: Any
    scene_digest_before: SceneDigestSnapshot
    scene_digest_after: SceneDigestSnapshot


def snapshot_from_provider(provider: StateDigestProvider) -> SceneDigestSnapshot:
    """Invoke and normalize one provider observation without leaking host errors."""
    try:
        raw = provider()
    except Exception:
        raise SceneDigestError(
            "scene_digest_provider_error",
            "The registered scene digest provider could not read host state",
        ) from None
    return normalize_scene_digest(raw)


def normalize_scene_digest(raw: Mapping[str, Any] | SceneStats) -> SceneDigestSnapshot:
    """Validate, redact, bound, and fingerprint a provider result."""
    if isinstance(raw, SceneStats):
        source = raw.to_dict()
    elif isinstance(raw, Mapping):
        source = dict(raw)
    else:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Scene digest provider must return SceneStats or a mapping",
        )

    supplied_fingerprint = source.pop("fingerprint", None)
    _validate_core_fields(source)
    extra = source.get("extra", {})
    if not isinstance(extra, Mapping):
        raise SceneDigestError("scene_digest_invalid", "Scene digest extra must be a mapping")

    truncated = [False]
    payload = {
        "object_count": source["object_count"],
        "vertex_count": source["vertex_count"],
        "has_mesh": source["has_mesh"],
        "extra": _bounded_value(extra, depth=0, truncated=truncated),
    }
    if len(_canonical_json(payload).encode("utf-8")) > _MAX_PAYLOAD_BYTES:
        payload["extra"] = {"_truncated": True}
        truncated[0] = True
    fingerprint = _fingerprint(payload)
    if supplied_fingerprint is not None and supplied_fingerprint != fingerprint:
        raise SceneDigestError(
            "scene_digest_fingerprint_mismatch",
            "Provider-supplied scene digest fingerprint does not match canonical state",
        )
    return SceneDigestSnapshot(payload=payload, fingerprint=fingerprint, truncated=truncated[0])


def _validate_core_fields(source: Mapping[str, Any]) -> None:
    for field_name in ("object_count", "vertex_count", "has_mesh"):
        if field_name not in source:
            raise SceneDigestError("scene_digest_invalid", "Scene digest is missing a required core field")
    for field_name in ("object_count", "vertex_count"):
        value = source[field_name]
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise SceneDigestError(
                "scene_digest_invalid",
                "Scene digest count fields must be non-negative integers",
            )
    if not isinstance(source["has_mesh"], bool):
        raise SceneDigestError("scene_digest_invalid", "Scene digest has_mesh must be a boolean")


def _bounded_value(value: Any, *, depth: int, truncated: list[bool]) -> Any:
    if depth >= _MAX_DEPTH:
        truncated[0] = True
        return "<truncated>"
    if value is None or isinstance(value, (bool, int)):
        return value
    if isinstance(value, float):
        if math.isfinite(value):
            return value
        truncated[0] = True
        return "<non-finite>"
    if isinstance(value, str):
        if _ABSOLUTE_PATH.match(value):
            return "<redacted>"
        if len(value) > _MAX_STRING_CHARS:
            truncated[0] = True
            return value[:_MAX_STRING_CHARS] + "..."
        return value
    if isinstance(value, Mapping):
        result: dict[str, Any] = {}
        items = sorted(((str(key), item) for key, item in value.items()), key=lambda pair: pair[0])
        if len(items) > _MAX_ITEMS:
            truncated[0] = True
        for key, item in items[:_MAX_ITEMS]:
            if _SENSITIVE_KEY.search(key):
                result[key] = "<redacted>"
            else:
                result[key] = _bounded_value(item, depth=depth + 1, truncated=truncated)
        return result
    if isinstance(value, (list, tuple, set, frozenset)):
        items = list(value)
        if isinstance(value, (set, frozenset)):
            items.sort(key=repr)
        if len(items) > _MAX_ITEMS:
            truncated[0] = True
        return [_bounded_value(item, depth=depth + 1, truncated=truncated) for item in items[:_MAX_ITEMS]]
    truncated[0] = True
    return f"<unsupported:{type(value).__name__}>"


def _fingerprint(payload: Mapping[str, Any]) -> str:
    canonical = _canonical_json(payload)
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _canonical_json(payload: Mapping[str, Any]) -> str:
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


__all__ = [
    "SCENE_DIGEST_SCHEMA_VERSION",
    "SceneDigestError",
    "SceneDigestExecution",
    "SceneDigestExecutionError",
    "SceneDigestSnapshot",
    "StateDigestProvider",
    "normalize_scene_digest",
]
