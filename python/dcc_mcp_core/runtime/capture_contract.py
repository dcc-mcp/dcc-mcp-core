# ruff: noqa: UP006, UP007, UP045

"""Deterministic, payload-bounded capture response contract.

Capture implementations remain DCC-owned.  This module only turns encoded
frame bytes into a stable response shape that every adapter can use.
"""

from __future__ import annotations

import base64
from dataclasses import dataclass
from enum import Enum
import hashlib
import re
from typing import Any
from typing import Dict
from typing import Optional
from typing import Tuple
from typing import Union

__all__ = [
    "CaptureReturnMode",
    "CaptureTargetSpec",
    "build_capture_response",
    "capture_response",
]

_SHA256_RE = re.compile(r"^(?:sha256:)?[0-9a-fA-F]{64}$")
_TARGETS = frozenset(("dcc_window", "active_viewport", "scene_viewer", "crop"))


class CaptureReturnMode(str, Enum):
    """Allowed capture payload modes."""

    ARTIFACT_ONLY = "artifact_only"
    IMAGE = "image"
    BOTH = "both"

    @classmethod
    def parse(cls, value: Union[str, CaptureReturnMode]) -> CaptureReturnMode:
        if isinstance(value, cls):
            return value
        try:
            return cls(str(value).strip().lower())
        except ValueError as exc:
            raise ValueError("return_mode must be artifact_only, image, or both") from exc


@dataclass(frozen=True)
class CaptureTargetSpec:
    """Explicit capture target, optionally narrowed to a pixel crop."""

    kind: str
    crop_rect: Optional[Tuple[int, int, int, int]] = None
    visible: bool = True

    def __post_init__(self) -> None:
        kind = str(self.kind).strip().lower()
        if kind not in _TARGETS:
            raise ValueError("target must be dcc_window, active_viewport, scene_viewer, or crop")
        object.__setattr__(self, "kind", kind)
        if not self.visible:
            raise ValueError("target is hidden or blocked")
        if kind == "crop" and self.crop_rect is None:
            raise ValueError("crop target requires x, y, width, and height")
        if kind != "crop" and self.crop_rect is not None:
            raise ValueError("crop is only valid for a crop target")
        if self.crop_rect is not None:
            values = tuple(self.crop_rect)
            if len(values) != 4 or any(not isinstance(value, int) for value in values):
                raise ValueError("crop must contain four integer values")
            if values[2] <= 0 or values[3] <= 0:
                raise ValueError("crop width and height must be positive")
            object.__setattr__(self, "crop_rect", values)

    @classmethod
    def dcc_window(cls) -> CaptureTargetSpec:
        return cls("dcc_window")

    @classmethod
    def active_viewport(cls) -> CaptureTargetSpec:
        return cls("active_viewport")

    @classmethod
    def scene_viewer(cls) -> CaptureTargetSpec:
        return cls("scene_viewer")

    @classmethod
    def crop(cls, x: int, y: int, width: int, height: int) -> CaptureTargetSpec:
        return cls("crop", (x, y, width, height))


def _target_spec(value: Union[str, CaptureTargetSpec]) -> CaptureTargetSpec:
    if isinstance(value, CaptureTargetSpec):
        return value
    return CaptureTargetSpec(str(value))


def _digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _normalise_hash(value: str) -> str:
    candidate = str(value).strip().lower()
    if not _SHA256_RE.fullmatch(candidate):
        raise ValueError("unchanged_if_hash must be a 64-character SHA-256 digest")
    return candidate[7:] if candidate.startswith("sha256:") else candidate


def build_capture_response(
    data: bytes,
    *,
    width: int,
    height: int,
    format: str,
    backend: str,
    target: Union[str, CaptureTargetSpec] = "active_viewport",
    return_mode: Union[str, CaptureReturnMode] = CaptureReturnMode.ARTIFACT_ONLY,
    unchanged_if_hash: Optional[str] = None,
    artifact_uri: Optional[str] = None,
) -> Dict[str, Any]:
    """Build a deterministic capture response without duplicating pixels.

    ``artifact_only`` is the compatibility-safe default.  Inline image data
    is base64 encoded only for ``image``/``both`` and is omitted when
    ``unchanged_if_hash`` matches the computed digest.
    """
    if not isinstance(data, (bytes, bytearray, memoryview)):
        raise TypeError("capture data must be bytes-like")
    if int(width) <= 0 or int(height) <= 0:
        raise ValueError("width and height must be positive")
    normalized_format = str(format).strip().lower().lstrip(".")
    if not normalized_format:
        raise ValueError("format must not be empty")
    normalized_backend = str(backend).strip()
    if not normalized_backend:
        raise ValueError("backend must not be empty")
    mode = CaptureReturnMode.parse(return_mode)
    target_spec = _target_spec(target)
    payload = bytes(data)
    sha256 = _digest(payload)
    metadata: Dict[str, Any] = {
        "dimensions": [int(width), int(height)],
        "bytes": len(payload),
        "sha256": sha256,
        "format": normalized_format,
        "backend": normalized_backend,
        "target": target_spec.kind,
    }
    if target_spec.crop_rect is not None:
        metadata["crop"] = list(target_spec.crop_rect)

    result: Dict[str, Any] = {"metadata": metadata}
    if mode in (CaptureReturnMode.ARTIFACT_ONLY, CaptureReturnMode.BOTH):
        uri = artifact_uri or f"artefact://sha256/{sha256}"
        if not isinstance(uri, str) or not uri.strip():
            raise ValueError("artifact_uri must be a non-empty URI")
        if not uri.startswith("artefact://"):
            raise ValueError("artifact_uri must use the artefact:// scheme")
        result["artifact"] = {"uri": uri, "digest": f"sha256:{sha256}"}

    unchanged = False
    if unchanged_if_hash is not None:
        unchanged = _normalise_hash(unchanged_if_hash) == sha256
    if unchanged:
        result["unchanged"] = True
    elif mode in (CaptureReturnMode.IMAGE, CaptureReturnMode.BOTH):
        result["image"] = base64.b64encode(payload).decode("ascii")
    return result


def capture_response(frame: Any, **kwargs: Any) -> Dict[str, Any]:
    """Adapt a ``CaptureFrame``-like object to :func:`build_capture_response`."""
    return build_capture_response(
        frame.data,
        width=frame.width,
        height=frame.height,
        format=frame.format,
        backend=kwargs.pop("backend", "unknown"),
        **kwargs,
    )
