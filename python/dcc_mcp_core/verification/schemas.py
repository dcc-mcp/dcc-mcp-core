"""Canonical, versioned state-export schemas for the behavior verification contract.

This module is the single source of truth for the four read-only state-export
contracts introduced by the behavior-verification track (core#2269):

* ``dcc-mcp/anim-curves@1`` — animation curve times/values/tangents/infinity
  plus a redundant ``key_count`` for direct assertion.
* ``dcc-mcp/rig-state@1`` — joint count/hierarchy, constraint targets, and
  per-mesh skin summaries.
* ``dcc-mcp/sim-status@1`` — simulation cache existence, frame count, and
  per-frame particle/vertex counts.
* ``dcc-mcp/graph-state@1`` — node/connection/param/output topology for
  graph-based hosts (Designer, Nuke).

Each schema is packaged as a JSON Schema document (draft 2020-12) so that
downstream adapters can validate their exports in their own CI, and as a
dependency-free structural validator here so that harnesses can assert a
payload without pulling in ``jsonschema``.

The module is pure Python and Python 3.7 compatible (Maya 2022 / Blender 2.83):
no third-party imports, no compiled extension, and only ``typing`` generics.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any
from typing import Mapping

SCHEMA_ANIM_CURVES = "dcc-mcp/anim-curves"
SCHEMA_RIG_STATE = "dcc-mcp/rig-state"
SCHEMA_SIM_STATUS = "dcc-mcp/sim-status"
SCHEMA_GRAPH_STATE = "dcc-mcp/graph-state"

# Schema version published under each marketplace vocabulary name.
SCHEMA_VERSIONS: dict[str, int] = {
    SCHEMA_ANIM_CURVES: 1,
    SCHEMA_RIG_STATE: 1,
    SCHEMA_SIM_STATUS: 1,
    SCHEMA_GRAPH_STATE: 1,
}

# Vocabulary name -> packaged JSON Schema document (relative to this file's
# sibling ``dcc_mcp_core.schemas`` package).
_SCHEMA_FILENAMES: dict[str, str] = {
    SCHEMA_ANIM_CURVES: "anim-curves-v1.schema.json",
    SCHEMA_RIG_STATE: "rig-state-v1.schema.json",
    SCHEMA_SIM_STATUS: "sim-status-v1.schema.json",
    SCHEMA_GRAPH_STATE: "graph-state-v1.schema.json",
}

# Common tangent/edge behaviour labels. Kept permissive on the JSON Schema side
# (plain string) so adapters can extend; these are the canonical values.
INFINITY_KINDS = ("constant", "linear", "cycle", "cycle_relative", "oscillate")


class SchemaValidationError(ValueError):
    """Raised when a state-export payload violates its versioned schema."""

    def __init__(self, schema_name: str, message: str, *, path: str = "$") -> None:
        self.schema_name = schema_name
        self.message = message
        self.path = path
        super().__init__(f"{schema_name}: {message} (at {path})")


def schema_names() -> list[str]:
    """Return the supported vocabulary schema names, sorted."""
    return sorted(_SCHEMA_FILENAMES)


def schema_document(schema_name: str) -> dict[str, Any]:
    """Load and return the packaged JSON Schema document for *schema_name*.

    Raises
    ------
    KeyError
        If *schema_name* is not a supported vocabulary schema.
    SchemaValidationError
        If the packaged document cannot be parsed as a JSON object.

    """
    filename = _SCHEMA_FILENAMES.get(schema_name)
    if filename is None:
        raise KeyError(f"Unknown verification schema: {schema_name}")
    # The schema documents live next to the ``finding`` / ``install-sop``
    # documents inside ``dcc_mcp_core.schemas``.
    schemas_dir = Path(__file__).resolve().parent.parent / "schemas"
    raw = (schemas_dir / filename).read_text(encoding="utf-8")
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise SchemaValidationError(schema_name, "packaged schema is not valid JSON") from exc
    if not isinstance(payload, dict):
        raise SchemaValidationError(schema_name, "packaged schema must be a JSON object")
    return payload


def _require_mapping(payload: Any, schema_name: str, path: str) -> Mapping[str, Any]:
    if not isinstance(payload, Mapping):
        raise SchemaValidationError(schema_name, "payload must be an object", path=path)
    return payload


def _require_field(
    payload: Mapping[str, Any],
    field: str,
    schema_name: str,
    *,
    path: str = "$",
    expected_type: Any = None,
) -> Any:
    if field not in payload:
        raise SchemaValidationError(schema_name, f"missing required field: {field}", path=path)
    value = payload[field]
    if expected_type is not None:
        # ``bool`` is a subclass of ``int``, so a plain ``isinstance(value, int)``
        # would happily accept ``True`` where the packaged JSON Schema declares
        # ``"type": "integer"``. Reject it explicitly.
        if expected_type is int and isinstance(value, bool):
            raise SchemaValidationError(
                schema_name,
                f"field {field} must be int, got bool",
                path=f"{path}.{field}",
            )
        if not isinstance(value, expected_type):
            expected_name = getattr(expected_type, "__name__", expected_type)
            actual_name = type(value).__name__
            raise SchemaValidationError(
                schema_name,
                f"field {field} must be {expected_name}, got {actual_name}",
                path=f"{path}.{field}",
            )
    return value


def _require_non_negative_int(
    payload: Mapping[str, Any],
    field: str,
    schema_name: str,
    *,
    path: str = "$",
) -> int:
    """Require *field* to be a non-negative integer.

    Mirrors the packaged JSON Schema's ``{"type": "integer", "minimum": 0}``
    for count-style fields, including the explicit ``bool`` rejection that
    ``isinstance(value, int)`` alone cannot provide.
    """
    value = _require_field(payload, field, schema_name, path=path, expected_type=int)
    if value < 0:
        raise SchemaValidationError(
            schema_name,
            f"field {field} must be >= 0",
            path=f"{path}.{field}",
        )
    return value


def _check_schema_identity(payload: Mapping[str, Any], schema_name: str) -> None:
    """Require the payload to declare *schema_name* at the published version.

    Both fields are mandatory in every packaged JSON Schema document, so a
    payload that omits them is malformed rather than merely untagged.
    """
    declared = _require_field(payload, "schema_name", schema_name, expected_type=str)
    if declared != schema_name:
        raise SchemaValidationError(
            schema_name,
            f"schema_name mismatch: expected {schema_name}, got {declared}",
        )
    version = _require_field(payload, "schema_version", schema_name, expected_type=int)
    expected_version = SCHEMA_VERSIONS[schema_name]
    if version != expected_version:
        raise SchemaValidationError(
            schema_name,
            f"unsupported schema_version {version} (expected {expected_version})",
        )


def _require_number_list(
    payload: Mapping[str, Any],
    field: str,
    schema_name: str,
    path: str,
) -> list[float]:
    values = _require_field(payload, field, schema_name, path=path, expected_type=list)
    if not all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in values):
        raise SchemaValidationError(schema_name, f"field {field} must contain numbers", path=path)
    return [float(v) for v in values]


def _validate_anim_curves(payload: Mapping[str, Any]) -> None:
    _check_schema_identity(payload, SCHEMA_ANIM_CURVES)
    curves = _require_field(payload, "curves", SCHEMA_ANIM_CURVES, expected_type=list)
    for index, curve in enumerate(curves):
        path = f"$.curves[{index}]"
        curve_map = _require_mapping(curve, SCHEMA_ANIM_CURVES, path)
        _require_field(curve_map, "target", SCHEMA_ANIM_CURVES, path=path, expected_type=str)
        key_count = _require_non_negative_int(curve_map, "key_count", SCHEMA_ANIM_CURVES, path=path)
        times = _require_number_list(curve_map, "times", SCHEMA_ANIM_CURVES, path)
        values = _require_number_list(curve_map, "values", SCHEMA_ANIM_CURVES, path)
        if key_count != len(times):
            raise SchemaValidationError(
                SCHEMA_ANIM_CURVES,
                f"key_count {key_count} does not match times length {len(times)}",
                path=path,
            )
        if len(values) != len(times):
            raise SchemaValidationError(
                SCHEMA_ANIM_CURVES,
                f"values length {len(values)} does not match times length {len(times)}",
                path=path,
            )
        for tangent_field in ("tangents_in", "tangents_out"):
            if tangent_field in curve_map and curve_map[tangent_field] is not None:
                tangents = curve_map[tangent_field]
                if not isinstance(tangents, list) or len(tangents) not in (0, len(times)):
                    raise SchemaValidationError(
                        SCHEMA_ANIM_CURVES,
                        f"{tangent_field} must have length 0 or {len(times)}",
                        path=path,
                    )


def _validate_rig_state(payload: Mapping[str, Any]) -> None:
    _check_schema_identity(payload, SCHEMA_RIG_STATE)
    joints = _require_field(payload, "joints", SCHEMA_RIG_STATE, expected_type=dict)
    _require_non_negative_int(joints, "count", SCHEMA_RIG_STATE, path="$.joints")
    hierarchy = joints.get("hierarchy")
    if hierarchy is not None:
        if not isinstance(hierarchy, list):
            raise SchemaValidationError(
                SCHEMA_RIG_STATE,
                "joints.hierarchy must be an array",
                path="$.joints.hierarchy",
            )
        for index, joint in enumerate(hierarchy):
            path = f"$.joints.hierarchy[{index}]"
            joint_map = _require_mapping(joint, SCHEMA_RIG_STATE, path)
            _require_field(joint_map, "name", SCHEMA_RIG_STATE, path=path, expected_type=str)
    constraints = payload.get("constraints")
    if constraints is not None:
        if not isinstance(constraints, list):
            raise SchemaValidationError(SCHEMA_RIG_STATE, "constraints must be an array", path="$.constraints")
        for index, constraint in enumerate(constraints):
            path = f"$.constraints[{index}]"
            constraint_map = _require_mapping(constraint, SCHEMA_RIG_STATE, path)
            _require_field(constraint_map, "target", SCHEMA_RIG_STATE, path=path, expected_type=str)
    skins = payload.get("skins")
    if skins is not None:
        if not isinstance(skins, list):
            raise SchemaValidationError(SCHEMA_RIG_STATE, "skins must be an array", path="$.skins")
        for index, skin in enumerate(skins):
            path = f"$.skins[{index}]"
            skin_map = _require_mapping(skin, SCHEMA_RIG_STATE, path)
            _require_field(skin_map, "mesh", SCHEMA_RIG_STATE, path=path, expected_type=str)
            _require_non_negative_int(skin_map, "influences", SCHEMA_RIG_STATE, path=path)
            _require_non_negative_int(skin_map, "unnormalized_vertices", SCHEMA_RIG_STATE, path=path)


def _validate_sim_status(payload: Mapping[str, Any]) -> None:
    _check_schema_identity(payload, SCHEMA_SIM_STATUS)
    _require_field(payload, "cache_exists", SCHEMA_SIM_STATUS, expected_type=bool)
    _require_non_negative_int(payload, "frame_count", SCHEMA_SIM_STATUS)
    per_frame = payload.get("per_frame_counts")
    if per_frame is not None:
        if not isinstance(per_frame, list):
            raise SchemaValidationError(
                SCHEMA_SIM_STATUS,
                "per_frame_counts must be an array",
                path="$.per_frame_counts",
            )
        for index, entry in enumerate(per_frame):
            path = f"$.per_frame_counts[{index}]"
            entry_map = _require_mapping(entry, SCHEMA_SIM_STATUS, path)
            _require_field(entry_map, "frame", SCHEMA_SIM_STATUS, path=path, expected_type=int)


def _validate_graph_state(payload: Mapping[str, Any]) -> None:
    _check_schema_identity(payload, SCHEMA_GRAPH_STATE)
    nodes = _require_field(payload, "nodes", SCHEMA_GRAPH_STATE, expected_type=list)
    for index, node in enumerate(nodes):
        path = f"$.nodes[{index}]"
        node_map = _require_mapping(node, SCHEMA_GRAPH_STATE, path)
        _require_field(node_map, "name", SCHEMA_GRAPH_STATE, path=path, expected_type=str)
    for collection, fields in (
        ("connections", ("from", "to")),
        ("params", ("node", "name")),
        ("outputs", ("node", "name")),
    ):
        entries = payload.get(collection)
        if entries is None:
            continue
        if not isinstance(entries, list):
            raise SchemaValidationError(SCHEMA_GRAPH_STATE, f"{collection} must be an array", path=f"$.{collection}")
        for index, entry in enumerate(entries):
            path = f"$.{collection}[{index}]"
            entry_map = _require_mapping(entry, SCHEMA_GRAPH_STATE, path)
            for field in fields:
                _require_field(entry_map, field, SCHEMA_GRAPH_STATE, path=path, expected_type=str)


_VALIDATORS = {
    SCHEMA_ANIM_CURVES: _validate_anim_curves,
    SCHEMA_RIG_STATE: _validate_rig_state,
    SCHEMA_SIM_STATUS: _validate_sim_status,
    SCHEMA_GRAPH_STATE: _validate_graph_state,
}


def validate_state_export(payload: Mapping[str, Any], schema_name: str) -> None:
    """Validate a state-export payload against its versioned schema.

    This is the dependency-free structural validator: it checks required
    fields, their types, and the cross-field invariants that plain JSON Schema
    cannot express (for example ``key_count == len(times)``). Harnesses that
    want a full JSON Schema pass can validate ``schema_document(schema_name)``
    with ``jsonschema``; both are intentionally kept in lockstep.

    Raises
    ------
    KeyError
        If *schema_name* is not a supported vocabulary schema.
    TypeError
        If *payload* is not a mapping.
    SchemaValidationError
        If the payload violates the schema.

    """
    validator = _VALIDATORS.get(schema_name)
    if validator is None:
        raise KeyError(f"Unknown verification schema: {schema_name}")
    if not isinstance(payload, Mapping):
        raise TypeError("state-export payload must be a mapping")
    validator(payload)


def _first_curve(payload: Mapping[str, Any], target: str | None) -> Mapping[str, Any]:
    curves = payload.get("curves")
    if not isinstance(curves, list) or not curves:
        raise SchemaValidationError(SCHEMA_ANIM_CURVES, "curves must be a non-empty array")
    if target is None:
        selected = curves[0]
    else:
        selected = next((c for c in curves if isinstance(c, Mapping) and c.get("target") == target), None)
        if selected is None:
            raise SchemaValidationError(SCHEMA_ANIM_CURVES, f"no curve named {target}")
    return _require_mapping(selected, SCHEMA_ANIM_CURVES, "$.curves")


def sample_curve(curve: Mapping[str, Any], t: float) -> float:
    """Linearly interpolate the value of a single anim-curve at time *t*.

    Sampling is deliberately linear: tangents are exported for downstream use
    but the core contract only guarantees times/values. Times before the first
    key and after the last key clamp to the nearest endpoint value (constant
    extrapolation), which is deterministic across adapters.

    Raises
    ------
    SchemaValidationError
        If *curve* is malformed (empty, non-monotonic, or length mismatch).

    """
    times = curve.get("times")
    values = curve.get("values")
    if not isinstance(times, list) or not isinstance(values, list):
        raise SchemaValidationError(SCHEMA_ANIM_CURVES, "curve must provide times and values arrays")
    if len(times) != len(values):
        raise SchemaValidationError(SCHEMA_ANIM_CURVES, "times and values lengths differ")
    if not times:
        raise SchemaValidationError(SCHEMA_ANIM_CURVES, "cannot sample an empty curve")
    if len(times) == 1:
        return float(values[0])
    if any(float(times[i + 1]) < float(times[i]) for i in range(len(times) - 1)):
        raise SchemaValidationError(SCHEMA_ANIM_CURVES, "curve times must be non-decreasing")
    if t <= float(times[0]):
        return float(values[0])
    if t >= float(times[-1]):
        return float(values[-1])
    for i in range(len(times) - 1):
        t0 = float(times[i])
        t1 = float(times[i + 1])
        if t0 <= t <= t1:
            if t1 == t0:
                return float(values[i])
            fraction = (t - t0) / (t1 - t0)
            return float(values[i]) + fraction * (float(values[i + 1]) - float(values[i]))
    return float(values[-1])  # pragma: no cover - defensive


def value_at(payload: Mapping[str, Any], t: float, target: str | None = None) -> float:
    """Return the sampled value of a state-export anim-curves payload at time *t*.

    *payload* may be a full ``dcc-mcp/anim-curves@1`` payload (``curves`` array)
    or a single curve mapping. When *target* is given, the curve whose
    ``target`` matches is selected; otherwise the first curve is used.

    Example::

        curves = call("get_anim_curves", targets=["rotor_main.rotateY"])
        assert abs(value_at(curves, t=24) - 360.0) < 1e-3
    """
    if "curves" in payload and isinstance(payload.get("curves"), list):
        return sample_curve(_first_curve(payload, target), t)
    return sample_curve(payload, t)


__all__ = [
    "INFINITY_KINDS",
    "SCHEMA_ANIM_CURVES",
    "SCHEMA_GRAPH_STATE",
    "SCHEMA_RIG_STATE",
    "SCHEMA_SIM_STATUS",
    "SCHEMA_VERSIONS",
    "SchemaValidationError",
    "sample_curve",
    "schema_document",
    "schema_names",
    "validate_state_export",
    "value_at",
]
