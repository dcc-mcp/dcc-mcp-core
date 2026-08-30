"""Bounded, redacted script-runtime scene digest contracts (issue #2260)."""

from __future__ import annotations

from dataclasses import dataclass
import hashlib
import hmac
import json
import math
import re
import secrets
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
_MAX_INTEGER_BITS = 512
_TRUNCATION_MARKER = "_dcc_mcp_truncated"
# The public fingerprint remains a deterministic, content-addressed SHA-256 so
# equivalent host observations compare equal.  Completeness, however, is a
# trust-boundary bit: a caller must not be able to remove the marker, recompute
# the public hash, and turn an incomplete observation into verified evidence.
# A process-local HMAC authenticates the complete canonical snapshot whenever
# it crosses the ``to_dict``/envelope boundary.  The key is intentionally not
# serialized or exported; snapshots are validated by the adapter process that
# captured them, while the public SHA remains stable for diagnostics.
_SNAPSHOT_INTEGRITY_KEY = secrets.token_bytes(32)
_SENSITIVE_KEY = re.compile(
    r"(?:api[_-]?key|authorization|credential|password|secret|token|file[_-]?path|path)",
    re.IGNORECASE,
)
# Cover drive-qualified, UNC, and rooted current-drive Windows paths as well
# as POSIX paths.  Relative paths are intentionally left intact because they
# are not independently identifiable as host-local locations.
_ABSOLUTE_PATH = re.compile(r"^(?:[A-Za-z]:[\\/]|[/\\]{1,2})")


def _integrity_matches(left: str, right: str) -> bool:
    """Compare integrity text without leaking ``compare_digest`` type errors."""
    try:
        return hmac.compare_digest(left.encode("utf-8"), right.encode("utf-8"))
    except (AttributeError, TypeError, UnicodeError):
        return False


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
    """Execution failure plus whatever host state readback was available."""

    def __init__(
        self,
        cause: Exception,
        scene_digest_before: SceneDigestSnapshot,
        scene_digest_after: SceneDigestSnapshot | None,
        *,
        readback_error: SceneDigestError | None = None,
    ) -> None:
        super().__init__(
            "Script execution failed after scene digest capture"
            if readback_error is None
            else "Script execution completed without a trusted after-state readback",
        )
        self.cause = cause
        self.scene_digest_before = scene_digest_before
        self.scene_digest_after = scene_digest_after
        self.readback_error = readback_error


@dataclass(frozen=True)
class SceneDigestSnapshot:
    """Canonical bounded scene state plus its deterministic fingerprint."""

    payload: dict[str, Any]
    fingerprint: str
    truncated: bool = False
    schema_version: str = SCENE_DIGEST_SCHEMA_VERSION
    integrity: str = ""

    def to_dict(self) -> dict[str, Any]:
        """Return the adapter-facing JSON-safe evidence shape."""
        self.validate()
        try:
            payload = json.loads(_canonical_json(self.payload))
        except Exception:
            # ``payload`` is intentionally a plain dict for compatibility,
            # but it can be contaminated by an adapter or a suffix that keeps
            # a reference to the snapshot.  Never leak a raw JSON TypeError
            # across the result boundary.
            raise SceneDigestError(
                "scene_digest_invalid",
                "Scene digest payload is no longer JSON serializable",
            ) from None
        return {
            "schema_version": self.schema_version,
            "fingerprint": self.fingerprint,
            "payload": payload,
            "truncated": self.truncated,
            "integrity": self.integrity,
        }

    def validate(self) -> None:
        """Reject corrupted or mismatched evidence before envelope attachment."""
        try:
            if self.schema_version != SCENE_DIGEST_SCHEMA_VERSION:
                raise SceneDigestError(
                    "scene_digest_fingerprint_mismatch",
                    "Scene digest evidence does not match its canonical fingerprint",
                )
            if not isinstance(self.truncated, bool):
                raise SceneDigestError(
                    "scene_digest_fingerprint_mismatch",
                    "Scene digest evidence does not match its canonical fingerprint",
                )
            if not isinstance(self.integrity, str):
                raise SceneDigestError(
                    "scene_digest_fingerprint_mismatch",
                    "Scene digest integrity must be a string",
                )
            payload, inferred_truncated = _canonical_snapshot_payload(self.payload)
            if inferred_truncated != self.truncated:
                raise SceneDigestError(
                    "scene_digest_fingerprint_mismatch",
                    "Scene digest completeness does not match its canonical payload",
                )
            expected = _fingerprint(payload, truncated=self.truncated)
            expected_integrity = _snapshot_integrity(
                schema_version=self.schema_version,
                fingerprint=self.fingerprint,
                payload=payload,
                truncated=self.truncated,
            )
        except SceneDigestError:
            raise
        except Exception:
            # A caller may have mutated a nested payload value after capture;
            # convert serializer/type failures into the stable fail-closed
            # contract error expected by envelope builders.
            raise SceneDigestError(
                "scene_digest_invalid",
                "Scene digest payload is corrupted or not JSON serializable",
            ) from None
        if (
            self.fingerprint != expected
            or self.payload != payload
            or not _integrity_matches(self.integrity, expected_integrity)
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
    try:
        if isinstance(raw, SceneStats):
            source = raw.to_dict()
        elif isinstance(raw, Mapping):
            source = dict(raw)
        else:
            raise SceneDigestError(
                "scene_digest_invalid",
                "Scene digest provider must return SceneStats or a mapping",
            )

        # A serialized snapshot is a different contract from a raw provider
        # mapping: preserve its canonical payload/completeness bit and verify
        # the process-local integrity tag before accepting it as evidence.
        if "payload" in source and "fingerprint" in source and "truncated" in source:
            return _snapshot_from_dict(source)

        supplied_fingerprint = source.pop("fingerprint", None)
        supplied_truncated = source.pop("truncated", None)
        supplied_integrity = source.pop("integrity", None)
        supplied_payload_truncated = source.pop(_TRUNCATION_MARKER, None)
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
        # Preserve a tamper-evident marker in the canonical payload.  Once
        # bounded values have been reduced, re-normalization cannot infer
        # whether source items were omitted from the remaining list/string;
        # retaining this marker lets validation reject a forged flag even if
        # a caller reuses the canonical payload and fingerprint.
        if truncated[0]:
            payload[_TRUNCATION_MARKER] = True
        if supplied_payload_truncated is not None and (
            not isinstance(supplied_payload_truncated, bool) or supplied_payload_truncated != truncated[0]
        ):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest payload truncation marker does not match canonical state",
            )
        fingerprint = _fingerprint(payload, truncated=truncated[0])
        integrity = _snapshot_integrity(
            schema_version=SCENE_DIGEST_SCHEMA_VERSION,
            fingerprint=fingerprint,
            payload=payload,
            truncated=truncated[0],
        )
        if supplied_fingerprint is not None and supplied_fingerprint != fingerprint:
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Provider-supplied scene digest fingerprint does not match canonical state",
            )
        if supplied_truncated is not None and (
            not isinstance(supplied_truncated, bool) or supplied_truncated != truncated[0]
        ):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Provider-supplied scene digest truncation flag does not match canonical state",
            )
        if supplied_integrity is not None and (
            not isinstance(supplied_integrity, str) or not _integrity_matches(supplied_integrity, integrity)
        ):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Provider-supplied scene digest integrity does not match canonical state",
            )
        return SceneDigestSnapshot(
            payload=payload,
            fingerprint=fingerprint,
            truncated=truncated[0],
            integrity=integrity,
        )
    except SceneDigestError:
        raise
    except Exception:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Scene digest provider returned invalid or non-serializable state",
        ) from None


def _validate_core_fields(source: Mapping[str, Any]) -> None:
    for field_name in ("object_count", "vertex_count", "has_mesh"):
        if field_name not in source:
            raise SceneDigestError("scene_digest_invalid", "Scene digest is missing a required core field")
    for field_name in ("object_count", "vertex_count"):
        value = source[field_name]
        if isinstance(value, bool) or not isinstance(value, int) or value < 0 or value.bit_length() > _MAX_INTEGER_BITS:
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
    if value is None or isinstance(value, bool):
        return value
    if isinstance(value, int):
        if value.bit_length() > _MAX_INTEGER_BITS:
            raise SceneDigestError("scene_digest_invalid", "Scene digest integers exceed the bounded range")
        return value
    if isinstance(value, float):
        if math.isfinite(value):
            return value
        raise SceneDigestError("scene_digest_invalid", "Scene digest contains a non-finite number")
    if isinstance(value, str):
        if _ABSOLUTE_PATH.match(value):
            return "<redacted>"
        if len(value) > _MAX_STRING_CHARS:
            truncated[0] = True
            return value[:_MAX_STRING_CHARS] + "..."
        return value
    if isinstance(value, Mapping):
        result: dict[str, Any] = {}
        # Read only one sentinel past the advertised bound.  Sorting a bounded
        # prefix keeps the wire representation stable for ordered provider
        # mappings without materialising attacker-sized dictionaries.
        items: list[tuple[str, Any]] = []
        for index, (key, item) in enumerate(value.items()):
            if index >= _MAX_ITEMS:
                truncated[0] = True
                # Once a mapping exceeds the observation budget, do not
                # retain an insertion-order-dependent prefix. A fixed
                # sentinel keeps equivalent host mappings deterministic while
                # still reading only one bounded sentinel item.
                return {"_truncated": True}
            items.append((str(key), item))
        items.sort(key=lambda pair: pair[0])
        for key, item in items:
            if _SENSITIVE_KEY.search(key):
                result[key] = "<redacted>"
            else:
                result[key] = _bounded_value(item, depth=depth + 1, truncated=truncated)
        return result
    if isinstance(value, (list, tuple, set, frozenset)):
        # Lists and sets are sampled with a sentinel rather than copied in
        # full.  Sets are sorted only after the bounded prefix is collected;
        # provider output is explicitly incomplete once the sentinel appears.
        items: list[Any] = []
        for index, item in enumerate(value):
            if index >= _MAX_ITEMS:
                truncated[0] = True
                if isinstance(value, (set, frozenset)):
                    return {"_truncated": True}
                break
            items.append(item)
        if isinstance(value, (set, frozenset)):
            items.sort(key=repr)
        return [_bounded_value(item, depth=depth + 1, truncated=truncated) for item in items]
    raise SceneDigestError(
        "scene_digest_invalid",
        f"Scene digest contains unsupported value type: {type(value).__name__}",
    )


def _fingerprint(payload: Mapping[str, Any], *, truncated: bool = False) -> str:
    canonical = _canonical_json(payload)
    # Preserve historical digest bytes for complete payloads while
    # authenticating the truncation bit for bounded observations.  Without
    # this suffix a caller could reuse a complete payload/fingerprint pair
    # and simply flip ``truncated`` on the snapshot.
    if truncated:
        canonical += "|truncated"
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _snapshot_integrity(
    *,
    schema_version: str,
    fingerprint: str,
    payload: Mapping[str, Any],
    truncated: bool,
) -> str:
    """Authenticate a canonical snapshot without changing its public hash."""
    message = "\x00".join(
        (
            schema_version,
            fingerprint,
            "1" if truncated else "0",
            _canonical_json(payload),
        )
    ).encode("utf-8")
    return "hmac-sha256:" + hmac.new(_SNAPSHOT_INTEGRITY_KEY, message, hashlib.sha256).hexdigest()


def _canonical_snapshot_payload(payload: Mapping[str, Any]) -> tuple[dict[str, Any], bool]:
    """Validate a bounded payload while preserving its authenticated marker.

    Re-normalizing a bounded list cannot tell whether its source contained more
    than ``_MAX_ITEMS`` entries.  The marker is therefore accepted only as a
    boolean canonical field and is included in both the public fingerprint and
    private integrity tag; callers cannot silently drop it and claim a complete
    observation.
    """
    if not isinstance(payload, Mapping):
        raise SceneDigestError("scene_digest_invalid", "Scene digest payload must be a mapping")
    try:
        source = dict(payload)
        marker = source.pop(_TRUNCATION_MARKER, None)
        _validate_core_fields(source)
        extra = source.get("extra", {})
        if not isinstance(extra, Mapping):
            raise SceneDigestError("scene_digest_invalid", "Scene digest extra must be a mapping")
        bounded_truncated = [False]
        canonical = {
            "object_count": source["object_count"],
            "vertex_count": source["vertex_count"],
            "has_mesh": source["has_mesh"],
            "extra": _bounded_value(extra, depth=0, truncated=bounded_truncated),
        }
        if len(_canonical_json(canonical).encode("utf-8")) > _MAX_PAYLOAD_BYTES:
            canonical["extra"] = {"_truncated": True}
            bounded_truncated[0] = True
        if marker is not None and not isinstance(marker, bool):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest payload truncation marker must be boolean",
            )
        inferred = bool(marker) if marker is not None else bounded_truncated[0]
        if marker is False and bounded_truncated[0]:
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest payload truncation marker does not match canonical state",
            )
        if inferred:
            canonical[_TRUNCATION_MARKER] = True
        if canonical != dict(payload):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest evidence does not match its canonical payload",
            )
        return canonical, inferred
    except SceneDigestError:
        raise
    except Exception:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Scene digest payload is corrupted or not JSON serializable",
        ) from None


def _snapshot_from_dict(source: Mapping[str, Any]) -> SceneDigestSnapshot:
    """Rehydrate and authenticate the exact shape emitted by :meth:`to_dict`."""
    try:
        schema_version = source.get("schema_version")
        fingerprint = source.get("fingerprint")
        payload = source.get("payload")
        truncated = source.get("truncated")
        integrity = source.get("integrity")
        if (
            not isinstance(schema_version, str)
            or not isinstance(fingerprint, str)
            or not isinstance(truncated, bool)
            or not isinstance(integrity, str)
        ):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Serialized scene digest evidence is missing integrity fields",
            )
        canonical, inferred_truncated = _canonical_snapshot_payload(payload)
        if inferred_truncated != truncated:
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest completeness does not match its canonical payload",
            )
        expected_fingerprint = _fingerprint(canonical, truncated=truncated)
        if fingerprint != expected_fingerprint:
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest evidence does not match its canonical fingerprint",
            )
        expected_integrity = _snapshot_integrity(
            schema_version=schema_version,
            fingerprint=fingerprint,
            payload=canonical,
            truncated=truncated,
        )
        if not _integrity_matches(integrity, expected_integrity):
            raise SceneDigestError(
                "scene_digest_fingerprint_mismatch",
                "Scene digest evidence integrity verification failed",
            )
        return SceneDigestSnapshot(
            payload=canonical,
            fingerprint=fingerprint,
            truncated=truncated,
            schema_version=schema_version,
            integrity=integrity,
        )
    except SceneDigestError:
        raise
    except Exception:
        raise SceneDigestError(
            "scene_digest_invalid",
            "Serialized scene digest evidence is malformed",
        ) from None


def _canonical_json(payload: Mapping[str, Any]) -> str:
    return json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


__all__ = [
    "SCENE_DIGEST_SCHEMA_VERSION",
    "SceneDigestError",
    "SceneDigestExecution",
    "SceneDigestExecutionError",
    "SceneDigestSnapshot",
    "StateDigestProvider",
    "normalize_scene_digest",
]
