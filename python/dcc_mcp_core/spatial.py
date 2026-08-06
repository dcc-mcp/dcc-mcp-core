"""Deterministic coordinate-system conversion for DCC interchange."""

from __future__ import annotations

from dataclasses import dataclass
import math
from typing import Any
from typing import Dict
from typing import List
from typing import Mapping
from typing import Optional
from typing import Sequence
from typing import Union

__all__ = ["SpatialConvention", "plan_spatial_conversion"]

_AXIS_VECTORS = {
    "+X": (1.0, 0.0, 0.0),
    "-X": (-1.0, 0.0, 0.0),
    "+Y": (0.0, 1.0, 0.0),
    "-Y": (0.0, -1.0, 0.0),
    "+Z": (0.0, 0.0, 1.0),
    "-Z": (0.0, 0.0, -1.0),
}


def _normalize_axis(value: str) -> str:
    if not isinstance(value, str):
        raise TypeError("axis values must be strings")
    axis = value.strip().upper().replace("\u2212", "-")
    if axis in ("X", "Y", "Z"):
        axis = "+" + axis
    if axis not in _AXIS_VECTORS:
        raise ValueError(f"invalid signed axis {value!r}; expected one of {sorted(_AXIS_VECTORS)}")
    return axis


@dataclass(frozen=True)
class SpatialConvention:
    """Explicit right/up/forward axes and physical scale for one coordinate space."""

    right: str
    up: str
    forward: str
    meters_per_unit: float
    name: Optional[str] = None

    def __post_init__(self) -> None:
        axes = tuple(_normalize_axis(value) for value in (self.right, self.up, self.forward))
        if len({axis[-1] for axis in axes}) != 3:
            raise ValueError("right, up, and forward must use three distinct axes")
        if isinstance(self.meters_per_unit, bool):
            raise TypeError("meters_per_unit must be a positive finite number")
        meters_per_unit = float(self.meters_per_unit)
        if not math.isfinite(meters_per_unit) or meters_per_unit <= 0:
            raise ValueError("meters_per_unit must be a positive finite number")
        if self.name is not None and not isinstance(self.name, str):
            raise TypeError("name must be a string or None")
        object.__setattr__(self, "right", axes[0])
        object.__setattr__(self, "up", axes[1])
        object.__setattr__(self, "forward", axes[2])
        object.__setattr__(self, "meters_per_unit", meters_per_unit)

    @classmethod
    def from_dict(cls, data: Mapping[str, Any]) -> SpatialConvention:
        """Build a convention from a JSON-compatible mapping."""
        if not isinstance(data, Mapping):
            raise TypeError("spatial convention must be a mapping")
        return cls(
            right=data["right"],
            up=data["up"],
            forward=data["forward"],
            meters_per_unit=data["meters_per_unit"],
            name=data.get("name"),
        )

    def to_dict(self) -> Dict[str, Any]:
        """Serialize this convention to a JSON-compatible mapping."""
        result = {
            "right": self.right,
            "up": self.up,
            "forward": self.forward,
            "meters_per_unit": self.meters_per_unit,
        }
        if self.name is not None:
            result["name"] = self.name
        return result

    def basis_matrix(self) -> List[List[float]]:
        """Return the row-major matrix whose columns are right, up, and forward."""
        columns = [_AXIS_VECTORS[self.right], _AXIS_VECTORS[self.up], _AXIS_VECTORS[self.forward]]
        return [[columns[column][row] for column in range(3)] for row in range(3)]


def _transpose(matrix: Sequence[Sequence[float]]) -> List[List[float]]:
    return [[matrix[column][row] for column in range(3)] for row in range(3)]


def _multiply(left: Sequence[Sequence[float]], right: Sequence[Sequence[float]]) -> List[List[float]]:
    return [[sum(left[row][k] * right[k][column] for k in range(3)) for column in range(3)] for row in range(3)]


def _determinant(matrix: Sequence[Sequence[float]]) -> float:
    return (
        matrix[0][0] * (matrix[1][1] * matrix[2][2] - matrix[1][2] * matrix[2][1])
        - matrix[0][1] * (matrix[1][0] * matrix[2][2] - matrix[1][2] * matrix[2][0])
        + matrix[0][2] * (matrix[1][0] * matrix[2][1] - matrix[1][1] * matrix[2][0])
    )


def _point(value: Sequence[float]) -> List[float]:
    if isinstance(value, (str, bytes)) or not isinstance(value, Sequence) or len(value) != 3:
        raise TypeError("sample_point must be a three-number sequence")
    point = [float(component) for component in value]
    if not all(math.isfinite(component) for component in point):
        raise ValueError("sample_point components must be finite")
    return point


def plan_spatial_conversion(
    source: Union[SpatialConvention, Mapping[str, Any]],
    target: Union[SpatialConvention, Mapping[str, Any]],
    sample_point: Optional[Sequence[float]] = None,
) -> Dict[str, Any]:
    """Plan a source-to-target axis and unit conversion without mutating a scene."""
    source_convention = source if isinstance(source, SpatialConvention) else SpatialConvention.from_dict(source)
    target_convention = target if isinstance(target, SpatialConvention) else SpatialConvention.from_dict(target)
    axis_matrix = _multiply(target_convention.basis_matrix(), _transpose(source_convention.basis_matrix()))
    position_scale = source_convention.meters_per_unit / target_convention.meters_per_unit
    linear = [[component * position_scale for component in row] for row in axis_matrix]
    determinant = _determinant(axis_matrix)
    orientation_reversing = determinant < 0
    result = {
        "source": source_convention.to_dict(),
        "target": target_convention.to_dict(),
        "axis_matrix": axis_matrix,
        "position_scale": position_scale,
        "position_matrix": [
            [linear[0][0], linear[0][1], linear[0][2], 0.0],
            [linear[1][0], linear[1][1], linear[1][2], 0.0],
            [linear[2][0], linear[2][1], linear[2][2], 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        "determinant": determinant,
        "orientation_reversing": orientation_reversing,
        "mesh_winding": "reverse" if orientation_reversing else "preserve",
        "requires_tangent_handedness_flip": orientation_reversing,
        "matrix_convention": "row-major values; apply as M @ column_vector",
    }
    if sample_point is not None:
        point = _point(sample_point)
        result["converted_sample_point"] = [
            position_scale * sum(axis_matrix[row][column] * point[column] for column in range(3)) for row in range(3)
        ]
    return result
