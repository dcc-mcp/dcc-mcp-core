"""Cross-DCC asset verification contracts.

This module defines the minimal data contract that a *verifier* skill — a
tool that imports an asset produced by another DCC and reports what it
observed — is expected to return.  The core library ships only the shape;
the actual DCC-specific implementation (opening the file in Blender, Maya,
Unreal, Photoshop, ...) lives in the downstream DCC-specific repositories.

Why it lives here
-----------------

Keeping :class:`SceneStats` in ``dcc-mcp-core`` gives every downstream
adapter a single, versioned contract to target.  Contract tests in this
repo (see ``tests/test_verifier_contract.py``) freeze the shape; downstream
CI pipelines can then assert their verifier returns the same fields so
round-trip scenarios (producer DCC → file → verifier DCC) stay portable.

Intentional minimalism
----------------------

The contract only ships the three fields needed to assert the most common
round-trip invariants (object count, vertex count, mesh presence).
Anything beyond that — bounding boxes, material counts, animation frame
counts — should be placed in :attr:`SceneStats.extra`, a free-form mapping
that survives the :meth:`to_dict` / :meth:`from_dict` round-trip but is
never interpreted by the core contract.  This keeps the core surface
stable while allowing each DCC adapter to publish richer diagnostics.

Example:
-------
::

    from dcc_mcp_core import SceneStats

    produced = SceneStats(object_count=1, vertex_count=482, has_mesh=True)
    observed = verifier_skill_import_and_inspect("/tmp/sphere.fbx")

    assert produced.matches(observed, vertex_tolerance=0.05), (
        f"round-trip drift: expected {produced}, got {observed}"
    )

"""

# Import future modules
from __future__ import annotations

# Import built-in modules
from dataclasses import dataclass
from dataclasses import field
from typing import Any
from typing import Dict
from typing import Mapping

__all__ = ["SceneStats"]


@dataclass(frozen=True)
class SceneStats:
    """Structured result of importing and inspecting an exported asset.

    Downstream verifier skills (for example ``blender-fbx-verifier`` or
    ``maya-fbx-verifier``) MUST return this shape from their
    ``import_and_inspect(file_path)`` entry point.  Extension fields live
    in :attr:`extra` so the core contract can grow without breaking the
    shape guarantees.

    Parameters
    ----------
    object_count:
        Number of top-level scene objects the verifier saw after import.
    vertex_count:
        Total vertex count across all geometry objects in the imported
        scene.  Used by :meth:`matches` for fuzzy comparison because FBX
        normals / tangents can introduce small variance on re-import.
    has_mesh:
        ``True`` if at least one of the imported objects carries polygon
        geometry.  Guards against silent empty imports.
    extra:
        Free-form mapping for DCC-specific enrichments.  The core
        contract preserves this mapping through
        :meth:`to_dict` / :meth:`from_dict` but does not interpret it.

    """

    object_count: int
    vertex_count: int
    has_mesh: bool
    extra: Dict[str, Any] = field(default_factory=dict)  # noqa: UP006 — wheel ships abi3-py38, keep typing.Dict for Py3.7 parity

    def to_dict(self) -> Dict[str, Any]:  # noqa: UP006 — see above
        """Serialize this stats object to a plain dict.

        The return value is safe to nest inside a ``success_result``
        context or any JSON-encodable payload.
        """
        return {
            "object_count": self.object_count,
            "vertex_count": self.vertex_count,
            "has_mesh": self.has_mesh,
            "extra": dict(self.extra),
        }

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> SceneStats:
        """Reconstruct a :class:`SceneStats` from its dict form.

        Missing ``extra`` defaults to an empty dict so payloads produced
        by older verifiers keep deserialising cleanly.

        Raises
        ------
        KeyError
            If any of the three required contract fields is missing.
        TypeError
            If a field is present but has an incompatible type.

        """
        extra_value = data.get("extra", {})
        if not isinstance(extra_value, Mapping):
            raise TypeError(f"SceneStats.extra must be a mapping, got {type(extra_value).__name__}")
        return cls(
            object_count=int(data["object_count"]),
            vertex_count=int(data["vertex_count"]),
            has_mesh=bool(data["has_mesh"]),
            extra=dict(extra_value),
        )

    def matches(self, other: SceneStats, vertex_tolerance: float = 0.05) -> bool:
        """Return True when ``other`` is a plausible round-trip of ``self``.

        The comparison is strict on :attr:`object_count` and
        :attr:`has_mesh` (structural invariants) and fuzzy on
        :attr:`vertex_count` (numeric invariant) because FBX and other
        interchange formats can introduce vertex splits from normals or
        UV seams on re-import.

        Parameters
        ----------
        other:
            The stats reported by the verifier side.
        vertex_tolerance:
            Allowed relative drift on :attr:`vertex_count`, expressed as
            a fraction (``0.05`` meaning ±5%).  Must be non-negative.

        Raises
        ------
        ValueError
            If ``vertex_tolerance`` is negative.

        """
        if vertex_tolerance < 0:
            raise ValueError("vertex_tolerance must be non-negative")
        if self.object_count != other.object_count:
            return False
        if self.has_mesh != other.has_mesh:
            return False
        if self.vertex_count == 0:
            return other.vertex_count == 0
        drift = abs(self.vertex_count - other.vertex_count) / max(self.vertex_count, 1)
        return drift <= vertex_tolerance
