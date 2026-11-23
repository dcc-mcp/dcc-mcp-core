"""Scene-vs-spec validation (issue #2261).

``validate_scene_vs_spec`` turns a normalised scene-state export into a
``{status, passed, failures[]}`` verdict *before* export, so adapters catch the
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

from dcc_mcp_core.verification.schemas import SCHEMA_MODEL_STATE
from dcc_mcp_core.verification.schemas import validate_state_export

__all__ = [
    "SceneSpecFailure",
    "SceneSpecResult",
    "validate_scene_vs_spec",
    "validate_spec_document",
]

# Default minimum fraction of meshes that must carry at least one UV set.
# The evaluation logged 31% of meshes without UVs; a sane floor is 90%.
_DEFAULT_MIN_UV_COVERAGE = 0.9
_DEFAULT_EULER_MAX_ABS_DEGREES = 360.0
_MODEL_CHECK_NAMES = ("parts", "hierarchy", "materials", "uv_coverage", "non_manifold", "euler")
_SPEC_KEYS = {
    "checks",
    "required_parts",
    "required_hierarchy",
    "required_materials",
    "min_uv_coverage",
    "allow_non_manifold",
    "euler_max_abs_degrees",
}


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
    status: str = ""

    def __post_init__(self) -> None:
        status = self.status or ("pass" if self.passed else "fail")
        if status not in {"pass", "fail", "unknown"}:
            raise ValueError("status must be one of pass, fail, or unknown")
        object.__setattr__(self, "status", status)
        object.__setattr__(self, "passed", status == "pass")

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-safe dict form."""
        return {
            "status": self.status,
            "passed": self.passed,
            "checks": [dict(check) for check in self.checks],
            "failures": [failure.to_dict() for failure in self.failures],
        }


def _validate_string_list(spec: Dict[str, Any], key: str) -> None:
    value = spec.get(key)
    if value is None:
        return
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ValueError(f"spec.{key} must be an array of strings")


def validate_spec_document(spec: Dict[str, Any]) -> None:
    """Validate the strict ``dcc-mcp/model-spec@1`` input document.

    The low-level evaluator keeps accepting the legacy mapping shape so direct
    callers can migrate independently. The agent-facing verification tool calls
    this function before evaluating a spec, which turns misspelled keys and an
    omitted check list into actionable input errors.
    """
    if not isinstance(spec, dict):
        raise TypeError("spec must be a mapping")
    unknown = sorted(set(spec).difference(_SPEC_KEYS))
    if unknown:
        raise ValueError(f"spec contains unsupported keys: {', '.join(unknown)}")
    checks = spec.get("checks")
    if not isinstance(checks, list) or not checks:
        raise ValueError("spec.checks must be a non-empty array")
    if not all(isinstance(check, str) for check in checks):
        raise ValueError("spec.checks must contain strings")
    invalid = sorted(set(checks).difference(_MODEL_CHECK_NAMES))
    if invalid:
        raise ValueError(f"spec.checks contains unsupported checks: {', '.join(invalid)}")
    if len(checks) != len(set(checks)):
        raise ValueError("spec.checks must not contain duplicates")
    for key in ("required_parts", "required_hierarchy", "required_materials"):
        _validate_string_list(spec, key)
    if "min_uv_coverage" in spec:
        value = spec["min_uv_coverage"]
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)):
            raise ValueError("spec.min_uv_coverage must be a finite number")
        if not 0.0 <= float(value) <= 1.0:
            raise ValueError("spec.min_uv_coverage must be between 0 and 1")
    if "allow_non_manifold" in spec and not isinstance(spec["allow_non_manifold"], bool):
        raise ValueError("spec.allow_non_manifold must be a boolean")
    if "euler_max_abs_degrees" in spec:
        value = spec["euler_max_abs_degrees"]
        if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)):
            raise ValueError("spec.euler_max_abs_degrees must be a finite number")
        if float(value) < 0.0:
            raise ValueError("spec.euler_max_abs_degrees must be non-negative")


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
    max_abs = float(spec.get("euler_max_abs_degrees", _DEFAULT_EULER_MAX_ABS_DEGREES))
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

    A declared ``dcc-mcp/model-state@1`` payload is structurally validated
    before evaluation. Legacy scene mappings remain supported for callers that
    have not yet adopted the versioned export.

    Returns a :class:`SceneSpecResult` with ``status`` set to ``pass``, ``fail``,
    or ``unknown``. ``passed`` remains a compatibility alias for
    ``status == "pass"``. A partial export or a check named in ``unavailable``
    produces ``unknown`` for that check and for the aggregate unless another
    check fails.
    """
    if not isinstance(scene, dict):
        raise TypeError("scene must be a mapping")
    if not isinstance(spec, dict):
        raise TypeError("spec must be a mapping")
    if "schema_name" in scene:
        validate_state_export(scene, SCHEMA_MODEL_STATE)

    failures: List[SceneSpecFailure] = []
    checks: List[Dict[str, Any]] = []
    unavailable = {str(value) for value in _as_list(scene.get("unavailable"))}
    partial = bool(scene.get("partial", False))
    has_unknown = False
    for name, checker in _CHECKERS.items():
        if partial or name in unavailable:
            has_unknown = True
            reason = "partial modeling state export" if partial else "check unavailable in modeling state export"
            checks.append({"check": name, "status": "unknown", "passed": False, "message": reason})
            continue
        check_failures = checker(scene, spec)
        checks.append(
            {
                "check": name,
                "status": "fail" if check_failures else "pass",
                "passed": not check_failures,
            }
        )
        failures.extend(check_failures)

    status = "fail" if failures else "unknown" if has_unknown else "pass"
    return SceneSpecResult(passed=status == "pass", checks=checks, failures=failures, status=status)
