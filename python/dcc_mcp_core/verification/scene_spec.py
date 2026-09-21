"""Scene-vs-spec validation (issue #2261).

``validate_scene_vs_spec`` turns a normalised scene-state export into a
``{passed, failures[]}`` verdict *before* export, so adapters catch the
evaluation's real defect class — meshes shipped without UVs, missing parts,
unbound materials, non-manifold geometry, out-of-bounds Euler — without
shipping scene data to a model.

The scene/spec shapes are documented in :mod:`dcc_mcp_core.verification` and
kept intentionally permissive: adapters fill in what their host can report.

Dispatch note: ``validate_scene_vs_spec`` runs **all six** checkers on every
call. A spec's ``checks`` key (RFC 0004, D2) records which checks the caller
*intends* to run, but it does not select anything yet -- it is metadata, not
dispatch. Wiring ``checks`` into dispatch is a Step 1 code change, so no
caller should read this module as promising that unlisted checks are skipped.

Because every checker runs, an adapter that cannot populate a checker's input
is expected to declare that check in the state export's ``unavailable`` field
(RFC 0004, D1), so that once D3 lands the gap rolls up to ``unknown`` instead
of passing silently.

**This function does not read ``unavailable`` yet.** It only gains that
behaviour when D3 ships; today a declared gap is still reported as ``pass``
for that check. Listing a check in ``unavailable`` is therefore a contract
obligation on the adapter's export, not a guarantee about the verdict this
function returns now.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import math
from typing import Any
from typing import Dict
from typing import List

__all__ = [
    "SceneSpecFailure",
    "SceneSpecResult",
    "validate_scene_vs_spec",
]

# Default minimum fraction of meshes that must carry at least one UV set.
# The evaluation logged 31% of meshes without UVs; a sane floor is 90%.
_DEFAULT_MIN_UV_COVERAGE = 0.9


@dataclass(frozen=True)
class SceneSpecFailure:
    """One failed validation check."""

    check: str
    message: str
    detail: Dict[str, Any] = field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-safe dict form."""
        return {"check": self.check, "message": self.message, "detail": dict(self.detail)}


@dataclass(frozen=True)
class SceneSpecResult:
    """Aggregate verdict of a scene-vs-spec validation."""

    passed: bool
    checks: List[Dict[str, Any]]
    failures: List[SceneSpecFailure]

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-safe dict form."""
        return {
            "passed": self.passed,
            "checks": [dict(check) for check in self.checks],
            "failures": [failure.to_dict() for failure in self.failures],
        }


def _as_list(value: Any) -> List[Any]:
    if value is None:
        return []
    if isinstance(value, (list, tuple)):
        return list(value)
    return [value]


def _mesh_names(scene: Dict[str, Any]) -> List[str]:
    return [str(mesh.get("name", "")) for mesh in _as_list(scene.get("meshes")) if isinstance(mesh, dict)]


def _check_parts(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    required = _as_list(spec.get("required_parts"))
    present = {str(part) for part in _as_list(scene.get("parts"))}
    for part in required:
        if str(part) not in present:
            failures.append(SceneSpecFailure("parts", "missing required part", {"part": str(part)}))
    return failures


def _check_hierarchy(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    required = _as_list(spec.get("required_hierarchy"))
    present = {str(path) for path in _as_list(scene.get("hierarchy"))}
    for path in required:
        if str(path) not in present:
            failures.append(SceneSpecFailure("hierarchy", "missing required hierarchy path", {"path": str(path)}))
    return failures


def _check_materials(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    required = {str(material) for material in _as_list(spec.get("required_materials"))}
    meshes = [mesh for mesh in _as_list(scene.get("meshes")) if isinstance(mesh, dict)]
    for mesh in meshes:
        name = str(mesh.get("name", "<unnamed>"))
        material = mesh.get("material")
        if material is None or str(material).strip() == "":
            failures.append(SceneSpecFailure("materials", "mesh has no material binding", {"mesh": name}))
        elif required and str(material) not in required:
            failures.append(
                SceneSpecFailure(
                    "materials",
                    "mesh material is not in required_materials",
                    {"mesh": name, "material": str(material)},
                )
            )
    return failures


def _check_uv_coverage(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    min_coverage = float(spec.get("min_uv_coverage", _DEFAULT_MIN_UV_COVERAGE))
    meshes = [mesh for mesh in _as_list(scene.get("meshes")) if isinstance(mesh, dict)]
    if not meshes:
        return failures
    covered = 0
    missing: List[str] = []
    for mesh in meshes:
        name = str(mesh.get("name", "<unnamed>"))
        try:
            uv_sets = int(mesh.get("uv_sets", mesh.get("uv_count", 0)))
        except (TypeError, ValueError):
            uv_sets = 0
        if uv_sets > 0:
            covered += 1
        else:
            missing.append(name)
    coverage = covered / len(meshes)
    if coverage < min_coverage:
        failures.append(
            SceneSpecFailure(
                "uv_coverage",
                "UV coverage below minimum",
                {
                    "covered": covered,
                    "total": len(meshes),
                    "coverage": round(coverage, 4),
                    "min_coverage": min_coverage,
                    "meshes_without_uvs": missing,
                },
            )
        )
    return failures


def _check_non_manifold(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    if spec.get("allow_non_manifold", False):
        return failures
    for mesh in _as_list(scene.get("meshes")):
        if not isinstance(mesh, dict):
            continue
        if bool(mesh.get("is_non_manifold", False)):
            failures.append(
                SceneSpecFailure(
                    "non_manifold",
                    "mesh is non-manifold",
                    {"mesh": str(mesh.get("name", "<unnamed>"))},
                )
            )
    return failures


def _check_euler(scene: Dict[str, Any], spec: Dict[str, Any]) -> List[SceneSpecFailure]:
    failures: List[SceneSpecFailure] = []
    max_abs = float(spec.get("euler_max_abs_degrees", 360.0))
    objects = [obj for obj in _as_list(scene.get("objects")) if isinstance(obj, dict)]
    for obj in objects:
        euler = obj.get("euler")
        if euler is None:
            continue
        name = str(obj.get("name", "<unnamed>"))
        try:
            values = [float(value) for value in _as_list(euler)]
        except (TypeError, ValueError):
            failures.append(SceneSpecFailure("euler", "object euler is not numeric", {"object": name}))
            continue
        if any(not math.isfinite(value) for value in values):
            failures.append(SceneSpecFailure("euler", "object euler is non-finite", {"object": name}))
            continue
        if any(abs(value) > max_abs for value in values):
            failures.append(
                SceneSpecFailure(
                    "euler",
                    "object euler exceeds bounds",
                    {"object": name, "euler": values, "max_abs_degrees": max_abs},
                )
            )
    return failures


_CHECKERS = {
    "parts": _check_parts,
    "hierarchy": _check_hierarchy,
    "materials": _check_materials,
    "uv_coverage": _check_uv_coverage,
    "non_manifold": _check_non_manifold,
    "euler": _check_euler,
}


def validate_scene_vs_spec(scene: Dict[str, Any], spec: Dict[str, Any]) -> SceneSpecResult:
    """Validate a normalised scene export against a written spec.

    The scene dict is the adapter's state export::

        {
            "parts": ["body", "rotor_main"],
            "hierarchy": ["root", "root/body"],
            "meshes": [
                {"name": "body", "uv_sets": 1, "material": "mat_metal", "is_non_manifold": false},
                {"name": "rotor", "uv_sets": 0, "material": null, "is_non_manifold": false},
            ],
            "objects": [{"name": "body", "euler": [0.0, 0.0, 0.0]}],
        }

    The spec carries the per-check configuration the evaluator reads::

        {
            "required_parts": ["body", "rotor_main"],
            "required_hierarchy": ["root/body"],
            "required_materials": ["mat_metal"],
            "min_uv_coverage": 0.9,
            "allow_non_manifold": false,
            "euler_max_abs_degrees": 360.0,
        }

    Every entry in :data:`_CHECKERS` runs on every call, so the result's
    ``checks`` list always has one entry per known check. A spec's own
    ``checks`` key does not narrow this set -- see the module docstring.

    Returns a :class:`SceneSpecResult` with a ``passed`` boolean, one entry
    per executed check, and a ``failures`` list naming every violation.
    """
    if not isinstance(scene, dict):
        raise TypeError("scene must be a mapping")
    if not isinstance(spec, dict):
        raise TypeError("spec must be a mapping")

    failures: List[SceneSpecFailure] = []
    checks: List[Dict[str, Any]] = []
    for name, checker in _CHECKERS.items():
        check_failures = checker(scene, spec)
        checks.append({"check": name, "passed": not check_failures})
        failures.extend(check_failures)

    return SceneSpecResult(passed=not failures, checks=checks, failures=failures)
