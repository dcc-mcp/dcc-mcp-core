"""Deterministic orchestration for the cross-DCC pipeline benchmark."""

from __future__ import annotations

import copy
from dataclasses import dataclass
import math
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import List
from typing import Mapping
from typing import Optional
from typing import Sequence

from dcc_mcp_core.verification.assertions import BehaviorReport

PIPELINE_BENCHMARK_SCHEMA_VERSION = 1
PIPELINE_BASELINE_ID = "rotorcraft-v1"

STAGE_PASS = "pass"
STAGE_FAILED = "failed"
STAGE_SKIPPED = "skipped"
STAGE_DEGRADED = "degraded"
STAGE_STATUSES = frozenset((STAGE_PASS, STAGE_FAILED, STAGE_SKIPPED, STAGE_DEGRADED))


@dataclass(frozen=True)
class PipelineStage:
    """One benchmark stage and the adapter families it exercises."""

    name: str
    adapters: tuple[str, ...]
    required_tools: tuple[str, ...]

    def to_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "adapters": list(self.adapters),
            "required_tools": list(self.required_tools),
        }


@dataclass(frozen=True)
class StageObservation:
    """Adapter-owned observation returned by a stage callback."""

    status: str
    calls: tuple[Mapping[str, Any], ...] = ()
    behavior_report: Optional[Any] = None
    reason: Optional[str] = None
    adapter_results: Optional[Mapping[str, Any]] = None

    @classmethod
    def from_value(cls, value: Any) -> StageObservation:
        if isinstance(value, cls):
            value = {
                "status": value.status,
                "calls": value.calls,
                "behavior_report": value.behavior_report,
                "reason": value.reason,
                "adapter_results": value.adapter_results,
            }
        if not isinstance(value, Mapping):
            raise TypeError("stage callback must return StageObservation or a mapping")
        status = value.get("status")
        if not isinstance(status, str) or status not in STAGE_STATUSES:
            raise ValueError("stage status must be one of pass, failed, skipped, degraded")
        raw_calls = value.get("calls", ())
        if not isinstance(raw_calls, Sequence) or isinstance(raw_calls, (str, bytes)):
            raise TypeError("stage calls must be a sequence of mappings")
        if any(not isinstance(call, Mapping) for call in raw_calls):
            raise TypeError("each stage call must be a mapping")
        for call in raw_calls:
            for field in ("read_only", "script"):
                if field in call and not isinstance(call[field], bool):
                    raise TypeError("stage call flags must be booleans")
        calls = tuple(raw_calls)
        reason = value.get("reason")
        if status in (STAGE_SKIPPED, STAGE_DEGRADED) and (not isinstance(reason, str) or not reason.strip()):
            raise ValueError("skipped and degraded stages require a reason")
        behavior = value.get("behavior_report")
        if behavior is not None and not isinstance(behavior, (Mapping, BehaviorReport)):
            raise TypeError("behavior_report must be a mapping")
        if isinstance(behavior, BehaviorReport) and behavior.total <= 0:
            raise ValueError("behavior assertion reports require at least one check")
        if isinstance(behavior, Mapping) and ("passed" in behavior or "total" in behavior):
            passed = behavior.get("passed")
            total = behavior.get("total")
            if (
                not isinstance(passed, int)
                or isinstance(passed, bool)
                or not isinstance(total, int)
                or isinstance(total, bool)
                or total <= 0
                or passed < 0
                or passed > total
            ):
                raise ValueError("behavior assertion counts must satisfy 0 <= passed <= total")
        adapter_results = value.get("adapter_results")
        if adapter_results is not None and not isinstance(adapter_results, Mapping):
            raise TypeError("adapter_results must be a mapping")
        return cls(status, calls, behavior, reason, adapter_results)


@dataclass(frozen=True)
class StageResult:
    """Normalized, serializable result for one pipeline stage."""

    stage: PipelineStage
    status: str
    calls: int
    mutating_calls: int
    script_calls: int
    behavior_assertion_pass_rate: Optional[float]
    behavior_assertion_passed: Optional[int]
    behavior_assertion_total: Optional[int]
    reason: Optional[str]
    adapter_results: Optional[Mapping[str, Any]]

    @property
    def bare_script_share(self) -> float:
        if self.mutating_calls == 0:
            return 0.0
        return self.script_calls / float(self.mutating_calls)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "stage": self.stage.to_dict(),
            "status": self.status,
            "calls": self.calls,
            "mutating_calls": self.mutating_calls,
            "script_calls": self.script_calls,
            "bare_script_share": round(self.bare_script_share, 6),
            "behavior_assertion_pass_rate": self.behavior_assertion_pass_rate,
            "behavior_assertion_passed": self.behavior_assertion_passed,
            "behavior_assertion_total": self.behavior_assertion_total,
            "reason": self.reason,
            "adapter_results": self.adapter_results,
        }


@dataclass(frozen=True)
class PipelineBenchmarkReport:
    """Complete benchmark output and release-gate decision."""

    baseline_id: str
    stages: tuple[StageResult, ...]
    behavior_assertion_target: float = 0.9
    bare_script_share_target: float = 0.2

    @property
    def skipped_stage_count(self) -> int:
        return sum(result.status == STAGE_SKIPPED for result in self.stages)

    @property
    def degraded_stage_count(self) -> int:
        return sum(result.status == STAGE_DEGRADED for result in self.stages)

    @property
    def bare_script_share(self) -> float:
        mutating = sum(result.mutating_calls for result in self.stages)
        scripts = sum(result.script_calls for result in self.stages)
        return scripts / float(mutating) if mutating else 0.0

    @property
    def behavior_assertion_pass_rate(self) -> float:
        passed = sum(
            result.behavior_assertion_passed or 0
            for result in self.stages
            if result.behavior_assertion_total is not None
        )
        total = sum(
            result.behavior_assertion_total or 0
            for result in self.stages
            if result.behavior_assertion_total is not None
        )
        has_rate_only = any(
            result.behavior_assertion_total is None and result.behavior_assertion_pass_rate is not None
            for result in self.stages
        )
        if total and has_rate_only:
            return 0.0
        if total:
            return passed / float(total)
        reports = [
            result.behavior_assertion_pass_rate
            for result in self.stages
            if result.behavior_assertion_pass_rate is not None
        ]
        return sum(reports) / len(reports) if reports else 0.0

    @property
    def behavior_assertion_evidence_valid(self) -> bool:
        """Return whether every stage has one consistent evidence format."""
        if not self.stages or any(result.behavior_assertion_pass_rate is None for result in self.stages):
            return False
        has_counted = any(result.behavior_assertion_total is not None for result in self.stages)
        has_rate_only = any(result.behavior_assertion_total is None for result in self.stages)
        return has_counted != has_rate_only

    @property
    def gate_passed(self) -> bool:
        return (
            bool(self.stages)
            and all(result.status == STAGE_PASS for result in self.stages)
            and self.behavior_assertion_evidence_valid
            and (
                self.bare_script_share < self.bare_script_share_target
                and self.behavior_assertion_pass_rate >= self.behavior_assertion_target
            )
        )

    def to_dict(self) -> Dict[str, Any]:
        return {
            "schema_version": PIPELINE_BENCHMARK_SCHEMA_VERSION,
            "baseline_id": self.baseline_id,
            "gate_passed": self.gate_passed,
            "metrics": {
                "bare_script_share": round(self.bare_script_share, 6),
                "bare_script_share_target": self.bare_script_share_target,
                "behavior_assertion_pass_rate": round(self.behavior_assertion_pass_rate, 6),
                "behavior_assertion_target": self.behavior_assertion_target,
                "degraded_stage_count": self.degraded_stage_count,
                "skipped_stage_count": self.skipped_stage_count,
            },
            "stages": [result.to_dict() for result in self.stages],
        }


DEFAULT_PIPELINE_STAGES = (
    PipelineStage("modeling", ("maya", "3dsmax", "houdini", "blender"), ("model", "validate_scene")),
    PipelineStage("rig", ("maya", "3dsmax"), ("rig", "export_rig_state")),
    PipelineStage("animation", ("maya", "3dsmax", "houdini", "blender"), ("animate", "get_anim_curves")),
    PipelineStage("texturing", ("painter",), ("texture", "inspect_project")),
    PipelineStage("material", ("designer",), ("material", "export_graph_state")),
    PipelineStage("render", ("maya", "3dsmax", "houdini", "blender"), ("render_frame", "image_stats")),
    PipelineStage("composite", ("nuke",), ("composite", "image_stats")),
)


def _behavior_metrics(value: Optional[Any]) -> tuple[Optional[float], Optional[int], Optional[int]]:
    if value is None:
        return None, None, None
    if isinstance(value, BehaviorReport):
        return value.pass_rate, value.passed, value.total
    raw = value.get("pass_rate")
    passed = value.get("passed")
    total = value.get("total")
    if (
        isinstance(passed, int)
        and not isinstance(passed, bool)
        and isinstance(total, int)
        and not isinstance(total, bool)
        and total > 0
        and 0 <= passed <= total
    ):
        return max(0.0, min(1.0, passed / float(total))), passed, total
    if isinstance(raw, (int, float)) and not isinstance(raw, bool):
        try:
            raw_float = float(raw)
        except (OverflowError, ValueError):
            raise ValueError("behavior assertion pass_rate must be numeric") from None
        if not math.isfinite(raw_float):
            raise ValueError("behavior assertion pass_rate must be finite")
        return max(0.0, min(1.0, raw_float)), None, None
    return None, None, None


def _result_for(stage: PipelineStage, observation: StageObservation) -> StageResult:
    mutating = sum(not bool(call.get("read_only")) for call in observation.calls)
    scripts = sum(bool(call.get("script")) for call in observation.calls if not bool(call.get("read_only")))
    behavior_rate, behavior_passed, behavior_total = _behavior_metrics(observation.behavior_report)
    return StageResult(
        stage=stage,
        status=observation.status,
        calls=len(observation.calls),
        mutating_calls=mutating,
        script_calls=scripts,
        behavior_assertion_pass_rate=behavior_rate,
        behavior_assertion_passed=behavior_passed,
        behavior_assertion_total=behavior_total,
        reason=observation.reason,
        adapter_results=copy.deepcopy(observation.adapter_results),
    )


def run_pipeline_benchmark(
    execute_stage: Callable[[PipelineStage], Any],
    *,
    stages: Iterable[PipelineStage] = DEFAULT_PIPELINE_STAGES,
    baseline_id: str = PIPELINE_BASELINE_ID,
    behavior_assertion_target: float = 0.9,
    bare_script_share_target: float = 0.2,
) -> PipelineBenchmarkReport:
    """Run every stage through one callback and return a bounded report."""
    stage_results: List[StageResult] = []
    for stage in stages:
        try:
            observation = StageObservation.from_value(execute_stage(stage))
            stage_results.append(_result_for(stage, observation))
        except Exception as exc:
            stage_results.append(
                _result_for(
                    stage,
                    StageObservation(status=STAGE_FAILED, reason=str(exc)),
                )
            )
    return PipelineBenchmarkReport(
        baseline_id=baseline_id,
        stages=tuple(stage_results),
        behavior_assertion_target=behavior_assertion_target,
        bare_script_share_target=bare_script_share_target,
    )


__all__ = [
    "DEFAULT_PIPELINE_STAGES",
    "PIPELINE_BASELINE_ID",
    "PIPELINE_BENCHMARK_SCHEMA_VERSION",
    "STAGE_DEGRADED",
    "STAGE_FAILED",
    "STAGE_PASS",
    "STAGE_SKIPPED",
    "STAGE_STATUSES",
    "PipelineBenchmarkReport",
    "PipelineStage",
    "StageObservation",
    "StageResult",
    "run_pipeline_benchmark",
]
