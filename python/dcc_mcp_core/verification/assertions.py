"""Dual-layer assertion library for the behavior verification contract.

The contract's central insight (core#2269) is that *structural* quantities
must be asserted exactly while *numeric* quantities must be asserted within
tolerance bands — simulations and renders are not bit-deterministic, so
exact-only assertions would be brittle, and tolerance-only assertions would
let silent count regressions through.

This module provides two surfaces:

* **Raising helpers** (``assert_exact``, ``assert_within``, ``assert_in_band``,
  ``assert_exists``, ``assert_resolution``, ``assert_state_schema``) that raise
  :class:`AssertionFailure` (an ``AssertionError`` subclass) on the first
  failed check, matching the ``assert`` idiom from the issue's examples.
* **A collecting verifier** (:class:`BehaviorVerifier`) that records every
  check into a :class:`BehaviorReport` whose ``pass_rate`` is the measurable
  metric the full-pipeline benchmark consumes (target >= 90%).

The layer split is intentional:

* ``exact`` — structural quantities: curve counts, joint counts, frame counts,
  booleans, enums, membership. Compared with ``==``, no tolerance.
* ``tolerance`` / ``within_band`` — numeric quantities: sampled key values,
  mean luma, vertex counts across re-import. Compared with an absolute or
  relative tolerance.

Everything is pure Python and Python 3.7 compatible.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from pathlib import Path
from typing import Any

from dcc_mcp_core.verification.schemas import SchemaValidationError
from dcc_mcp_core.verification.schemas import validate_state_export

KIND_EXACT = "exact"
KIND_TOLERANCE = "tolerance"
KIND_EXISTENCE = "existence"
KIND_RESOLUTION = "resolution"
KIND_SCHEMA = "schema"

_ALL_KINDS = (KIND_EXACT, KIND_TOLERANCE, KIND_EXISTENCE, KIND_RESOLUTION, KIND_SCHEMA)


class AssertionFailure(AssertionError):
    """A behavior assertion failed, carrying expected/actual for reporting."""

    def __init__(
        self,
        message: str,
        *,
        name: str = "",
        kind: str = "",
        expected: Any = None,
        actual: Any = None,
    ) -> None:
        self.name = name
        self.kind = kind
        self.expected = expected
        self.actual = actual
        super().__init__(message)


@dataclass(frozen=True)
class Check:
    """One recorded behavior check."""

    name: str
    kind: str
    passed: bool
    expected: Any
    actual: Any
    message: str = ""

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-safe representation of this check."""
        return {
            "name": self.name,
            "kind": self.kind,
            "passed": self.passed,
            "expected": self.expected,
            "actual": self.actual,
            "message": self.message,
        }


@dataclass
class BehaviorReport:
    """Aggregate result of a behavior verification run."""

    checks: list[Check] = field(default_factory=list)

    @property
    def total(self) -> int:
        return len(self.checks)

    @property
    def passed(self) -> int:
        return sum(1 for check in self.checks if check.passed)

    @property
    def failed(self) -> int:
        return self.total - self.passed

    @property
    def pass_rate(self) -> float:
        """Fraction of checks that passed, in ``[0.0, 1.0]``.

        An empty report returns ``1.0`` (vacuously passing) so a run with no
        assertions never reports a misleading ``0.0``.
        """
        if not self.checks:
            return 1.0
        return self.passed / float(self.total)

    def to_dict(self) -> dict[str, Any]:
        """Return a JSON-safe summary plus the ordered check list."""
        return {
            "total": self.total,
            "passed": self.passed,
            "failed": self.failed,
            "pass_rate": self.pass_rate,
            "checks": [check.to_dict() for check in self.checks],
        }


class BehaviorVerifier:
    """Collect behavior checks into a :class:`BehaviorReport` instead of raising.

    Use this when the harness wants the whole run's pass rate (the benchmark
    metric) rather than a fail-fast ``assert``::

        verify = BehaviorVerifier("rotor-rig")
        verify.exact("joint_count", rig["joints"]["count"], 12)
        verify.within("rotateY_at_24", value_at(curves, t=24), 360.0, tolerance=1e-3)
        report = verify.report()
        assert report.pass_rate >= 0.90
    """

    def __init__(self, name: str = "behavior") -> None:
        self.name = name
        self._checks: list[Check] = []

    # -- recording -----------------------------------------------------------
    def _record(self, name: str, kind: str, passed: bool, expected: Any, actual: Any, message: str = "") -> Check:
        check = Check(name=name, kind=kind, passed=passed, expected=expected, actual=actual, message=message)
        self._checks.append(check)
        return check

    # -- assertions ----------------------------------------------------------
    def exact(self, name: str, actual: Any, expected: Any) -> Check:
        """Assert a structural quantity equals *expected* exactly."""
        passed = actual == expected
        message = "" if passed else f"expected {name} == {expected} ({type(expected).__name__}), got {actual}"
        return self._record(name, KIND_EXACT, passed, expected, actual, message)

    def within(self, name: str, actual: Any, expected: Any, tolerance: float, *, relative: bool = False) -> Check:
        """Assert a numeric quantity is within *tolerance* of *expected*.

        When *relative* is true the tolerance is interpreted as a fraction of
        ``abs(expected)`` (for example ``0.05`` meaning +/-5%).
        """
        if tolerance < 0:
            raise ValueError("tolerance must be non-negative")
        a = float(actual)
        e = float(expected)
        allowed = tolerance if not relative else tolerance * abs(e)
        passed = abs(a - e) <= allowed
        message = "" if passed else f"expected {name} within {allowed:+g} of {e} (got {a})"
        return self._record(name, KIND_TOLERANCE, passed, {"expected": e, "tolerance": allowed}, a, message)

    def within_band(self, name: str, actual: Any, low: float, high: float) -> Check:
        """Assert a numeric quantity lies inside the inclusive band ``[low, high]``."""
        if low > high:
            raise ValueError("low must be <= high")
        a = float(actual)
        passed = low <= a <= high
        message = "" if passed else f"expected {name} in [{low}, {high}] (got {a})"
        return self._record(name, KIND_TOLERANCE, passed, {"low": low, "high": high}, a, message)

    def exists(self, name: str, path: Any) -> Check:
        """Assert a filesystem path exists (file or directory)."""
        passed = bool(path) and Path(str(path)).exists()
        message = "" if passed else f"expected {name} to exist: {path}"
        return self._record(name, KIND_EXISTENCE, passed, True, bool(path) and str(path), message)

    def resolution(self, name: str, width: Any, height: Any, *, min_width: int = 1, min_height: int = 1) -> Check:
        """Assert an image/video resolution meets minimums (existence/resolution check)."""
        w = int(width)
        h = int(height)
        passed = w >= min_width and h >= min_height
        message = "" if passed else f"expected {name} >= {min_width}x{min_height} (got {w}x{h})"
        return self._record(
            name,
            KIND_RESOLUTION,
            passed,
            {"min_width": min_width, "min_height": min_height},
            {"width": w, "height": h},
            message,
        )

    def schema(self, name: str, payload: Any, schema_name: str) -> Check:
        """Assert a state-export payload validates against its versioned schema."""
        try:
            validate_state_export(payload, schema_name)
        except (SchemaValidationError, TypeError, KeyError) as exc:
            return self._record(name, KIND_SCHEMA, False, schema_name, payload, str(exc))
        return self._record(name, KIND_SCHEMA, True, schema_name, payload)

    # -- result --------------------------------------------------------------
    def report(self) -> BehaviorReport:
        """Return the accumulated :class:`BehaviorReport`."""
        return BehaviorReport(checks=list(self._checks))

    def passed(self) -> bool:
        """Return True when every recorded check passed."""
        return all(check.passed for check in self._checks)


def _raise(name: str, kind: str, message: str, expected: Any, actual: Any) -> None:
    raise AssertionFailure(message, name=name, kind=kind, expected=expected, actual=actual)


def assert_exact(actual: Any, expected: Any, name: str = "") -> None:
    """Fail-fast structural assertion: ``actual == expected``."""
    if actual != expected:
        _raise(
            name,
            KIND_EXACT,
            f"expected {name or 'value'} == {expected} (got {actual})",
            expected,
            actual,
        )


def assert_within(actual: Any, expected: Any, tolerance: float, name: str = "", *, relative: bool = False) -> None:
    """Fail-fast numeric assertion: ``abs(actual - expected) <= tolerance``."""
    if tolerance < 0:
        raise ValueError("tolerance must be non-negative")
    a = float(actual)
    e = float(expected)
    allowed = tolerance if not relative else tolerance * abs(e)
    if abs(a - e) > allowed:
        _raise(
            name,
            KIND_TOLERANCE,
            f"expected {name or 'value'} within {allowed:+g} of {e} (got {a})",
            {"expected": e, "tolerance": allowed},
            a,
        )


def assert_in_band(actual: Any, low: float, high: float, name: str = "") -> None:
    """Fail-fast band assertion: ``low <= actual <= high``."""
    if low > high:
        raise ValueError("low must be <= high")
    a = float(actual)
    if not (low <= a <= high):
        _raise(
            name,
            KIND_TOLERANCE,
            f"expected {name or 'value'} in [{low}, {high}] (got {a})",
            {"low": low, "high": high},
            a,
        )


def assert_exists(path: Any, name: str = "") -> None:
    """Fail-fast existence assertion: the path exists on disk."""
    if not (bool(path) and Path(str(path)).exists()):
        _raise(name, KIND_EXISTENCE, f"expected {name or 'path'} to exist: {path}", True, path)


def assert_resolution(width: Any, height: Any, name: str = "", *, min_width: int = 1, min_height: int = 1) -> None:
    """Fail-fast resolution assertion: width/height meet minimums."""
    w = int(width)
    h = int(height)
    if not (w >= min_width and h >= min_height):
        _raise(
            name,
            KIND_RESOLUTION,
            f"expected {name or 'resolution'} >= {min_width}x{min_height} (got {w}x{h})",
            {"min_width": min_width, "min_height": min_height},
            {"width": w, "height": h},
        )


def assert_state_schema(payload: Any, schema_name: str, name: str = "") -> None:
    """Fail-fast schema assertion: validate *payload* against *schema_name*."""
    try:
        validate_state_export(payload, schema_name)
    except (SchemaValidationError, TypeError, KeyError) as exc:
        _raise(name, KIND_SCHEMA, str(exc), schema_name, payload)


__all__ = [
    "KIND_EXACT",
    "KIND_EXISTENCE",
    "KIND_RESOLUTION",
    "KIND_SCHEMA",
    "KIND_TOLERANCE",
    "AssertionFailure",
    "BehaviorReport",
    "BehaviorVerifier",
    "Check",
    "assert_exact",
    "assert_exists",
    "assert_in_band",
    "assert_resolution",
    "assert_state_schema",
    "assert_within",
]
