"""Deterministic offline evaluation records for modeling verification.

The first evaluation surface is deliberately small. A case owns a pure
grader callback, and the runner converts existing verification reports into a
stable, machine-readable result. It performs no DCC, network, or model work.
"""

from __future__ import annotations

from collections import Counter
from dataclasses import dataclass
import json
from pathlib import Path
import time
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import Mapping
from typing import Optional
from typing import Sequence
from typing import Tuple

from dcc_mcp_core.verification.assertions import BehaviorReport

EVAL_SET_SCHEMA_VERSION = 1
VERDICT_PASS = "pass"
VERDICT_FAIL = "fail"
VERDICT_INCONCLUSIVE = "inconclusive"
VERDICTS = (VERDICT_PASS, VERDICT_FAIL, VERDICT_INCONCLUSIVE)
_SCHEMA_FILES = {
    "set": "eval-set-v1.schema.json",
    "report": "eval-report-v1.schema.json",
}


def _json_safe(value: Any) -> Any:
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, Mapping):
        return {str(key): _json_safe(item) for key, item in value.items()}
    if isinstance(value, (list, tuple)):
        return [_json_safe(item) for item in value]
    return str(value)


def _checks_from_behavior_report(report: BehaviorReport) -> Tuple[Mapping[str, Any], ...]:
    return tuple(check.to_dict() for check in report.checks)


def _normalize_check(check: Mapping[str, Any]) -> Mapping[str, Any]:
    normalized = dict(_json_safe(check))
    if "name" not in normalized and "check" in normalized:
        normalized["name"] = normalized["check"]
    if "passed" not in normalized and "status" in normalized:
        normalized["passed"] = normalized["status"] == VERDICT_PASS
    if not isinstance(normalized.get("name"), str) or not normalized["name"].strip():
        raise ValueError("grader checks must contain a non-empty name")
    if not isinstance(normalized.get("passed"), bool):
        raise ValueError("grader checks must contain a boolean passed field")
    return normalized


def _normalize_grader_result(value: Any) -> Tuple[str, Tuple[Mapping[str, Any], ...], Optional[str]]:
    if isinstance(value, BehaviorReport):
        checks = _checks_from_behavior_report(value)
        if not checks:
            return VERDICT_INCONCLUSIVE, checks, "grader returned no checks"
        verdict = VERDICT_PASS if value.passed == value.total else VERDICT_FAIL
        return verdict, checks, None

    if isinstance(value, Mapping):
        raw_checks = value.get("checks", ())
        if not isinstance(raw_checks, Sequence) or isinstance(raw_checks, (str, bytes)):
            raise TypeError("grader checks must be a sequence of mappings")
        if any(not isinstance(check, Mapping) for check in raw_checks):
            raise TypeError("grader checks must contain mappings")
        checks = tuple(_normalize_check(check) for check in raw_checks)
        raw_verdict = value.get("verdict")
        if raw_verdict is None:
            if not checks:
                return VERDICT_INCONCLUSIVE, checks, "grader returned no checks"
            raw_verdict = VERDICT_PASS if all(bool(check.get("passed")) for check in checks) else VERDICT_FAIL
        if raw_verdict not in VERDICTS:
            raise ValueError(f"grader verdict must be one of {VERDICTS}")
        reason = value.get("reason")
        return raw_verdict, checks, str(reason) if reason is not None else None

    status = getattr(value, "status", None)
    raw_checks = getattr(value, "checks", None)
    if status in (VERDICT_PASS, VERDICT_FAIL, "unknown") and isinstance(raw_checks, Sequence):
        checks = tuple(_normalize_check(check) for check in raw_checks if isinstance(check, Mapping))
        verdict = VERDICT_INCONCLUSIVE if status == "unknown" else status
        reason = "verification returned unknown" if status == "unknown" else None
        return verdict, checks, reason

    raise TypeError("grader must return BehaviorReport, a result mapping, or a verification result")


@dataclass(frozen=True)
class EvalCase:
    """One offline evaluation case and its deterministic grader."""

    case_id: str
    title: str
    grader: Callable[[], Any]
    invocation: Optional[Mapping[str, Any]] = None
    tags: Tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if not self.case_id.strip():
            raise ValueError("case_id must not be blank")
        if not self.title.strip():
            raise ValueError("title must not be blank")
        if not callable(self.grader):
            raise TypeError("grader must be callable")

    def to_dict(self) -> Dict[str, Any]:
        return {
            "case_id": self.case_id,
            "title": self.title,
            "invocation": _json_safe(self.invocation or {}),
            "tags": list(self.tags),
        }


@dataclass(frozen=True)
class EvalRun:
    """The normalized result for one evaluation case."""

    case: EvalCase
    verdict: str
    checks: Tuple[Mapping[str, Any], ...]
    duration_ms: int
    artifacts: Tuple[str, ...] = ()
    reason: Optional[str] = None

    def __post_init__(self) -> None:
        if self.verdict not in VERDICTS:
            raise ValueError(f"verdict must be one of {VERDICTS}")
        if self.duration_ms < 0:
            raise ValueError("duration_ms must not be negative")

    def to_dict(self) -> Dict[str, Any]:
        return {
            "case": self.case.to_dict(),
            "verdict": self.verdict,
            "checks": [_json_safe(check) for check in self.checks],
            "duration_ms": self.duration_ms,
            "artifacts": list(self.artifacts),
            "reason": self.reason,
        }


@dataclass(frozen=True)
class EvalReport:
    """A complete, serializable report for one offline evaluation set."""

    set_version: str
    runs: Tuple[EvalRun, ...]
    generated_at: Optional[str] = None

    @property
    def pass_rate(self) -> float:
        if not self.runs:
            return 0.0
        return sum(run.verdict == VERDICT_PASS for run in self.runs) / float(len(self.runs))

    @property
    def failure_histogram(self) -> Dict[str, int]:
        histogram = Counter()
        for run in self.runs:
            if run.verdict == VERDICT_PASS:
                continue
            if run.reason:
                histogram[run.reason] += 1
            for check in run.checks:
                if not check.get("passed", False):
                    histogram[str(check.get("name", "unnamed"))] += 1
        return dict(sorted(histogram.items()))

    def to_dict(self) -> Dict[str, Any]:
        return {
            "schema_version": EVAL_SET_SCHEMA_VERSION,
            "set_version": self.set_version,
            "generated_at": self.generated_at,
            "metrics": {
                "case_count": len(self.runs),
                "pass_count": sum(run.verdict == VERDICT_PASS for run in self.runs),
                "fail_count": sum(run.verdict == VERDICT_FAIL for run in self.runs),
                "inconclusive_count": sum(run.verdict == VERDICT_INCONCLUSIVE for run in self.runs),
                "pass_rate": round(self.pass_rate, 6),
                "failure_histogram": self.failure_histogram,
            },
            "runs": [run.to_dict() for run in self.runs],
        }

    def dumps(self) -> str:
        """Serialize the report with stable key ordering and whitespace."""
        return json.dumps(self.to_dict(), ensure_ascii=False, sort_keys=True, indent=2) + "\n"


def run_eval_set(
    cases: Iterable[EvalCase],
    *,
    set_version: str = "eval-set-v1",
    generated_at: Optional[str] = None,
    clock: Optional[Callable[[], float]] = None,
) -> EvalReport:
    """Run cases in declaration order and fail closed on grader errors."""
    timer = clock or time.perf_counter
    runs = []
    for case in cases:
        started = timer()
        try:
            verdict, checks, reason = _normalize_grader_result(case.grader())
        except Exception as exc:
            verdict = VERDICT_INCONCLUSIVE
            checks = ({"name": "grader_error", "passed": False, "actual": str(exc)},)
            reason = str(exc)
        elapsed_ms = max(0, round((timer() - started) * 1000))
        runs.append(EvalRun(case, verdict, checks, elapsed_ms, reason=reason))
    return EvalReport(set_version=set_version, runs=tuple(runs), generated_at=generated_at)


def _schema_path(kind: str) -> Path:
    try:
        filename = _SCHEMA_FILES[kind]
    except KeyError:
        raise ValueError(f"unknown evaluation schema kind: {kind!r}") from None
    return Path(__file__).resolve().parent.parent / "schemas" / filename


def eval_set_json_schema() -> Dict[str, Any]:
    """Load the versioned JSON Schema for persisted evaluation cases."""
    return json.loads(_schema_path("set").read_text(encoding="utf-8"))


def eval_report_json_schema() -> Dict[str, Any]:
    """Load the versioned JSON Schema for persisted evaluation reports."""
    return json.loads(_schema_path("report").read_text(encoding="utf-8"))


__all__ = [
    "EVAL_SET_SCHEMA_VERSION",
    "VERDICTS",
    "VERDICT_FAIL",
    "VERDICT_INCONCLUSIVE",
    "VERDICT_PASS",
    "EvalCase",
    "EvalReport",
    "EvalRun",
    "eval_report_json_schema",
    "eval_set_json_schema",
    "run_eval_set",
]
