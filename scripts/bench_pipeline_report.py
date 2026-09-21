#!/usr/bin/env python3
"""Build the trend-friendly ``pipeline-bench.json`` report from raw stage metrics.

The Rust harness (``crates/dcc-mcp-telemetry/examples/pipeline_bench.rs``) records
every pipeline stage under a ``ToolRecorder`` and exports the aggregated
``ToolMetrics`` as a raw JSON report. This script turns that export into the
trend artifact consumed by dashboards and by the per-stage budget gate:

* it materialises ``success_rate`` next to ``p95_duration_ms`` for every stage;
* it attaches the per-stage budget declared in the benchmark scenario (optional);
* it exits non-zero when a stage breaches its budget.

Scenario input is optional on purpose: when the scenario file is absent, or
``PyYAML`` is unavailable, budgets degrade to "not applied" and the report is
still produced. Per-stage budgets can also be supplied on the command line,
which is how the gate is exercised without a scenario file.

Usage:
    python scripts/bench_pipeline_report.py \
        --metrics target/pipeline-bench-raw.json \
        --scenario scenarios/pipeline_full.yaml \
        --output pipeline-bench.json

Expected scenario shape (aliases accepted, see ``BUDGET_ALIASES``):

    budgets:
      serialize_request:
        p95_duration_ms: 0.5
        min_success_rate: 1.0

Exit codes: ``0`` pass or gate skipped, ``1`` budget breach, ``2`` input error.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
from typing import Any
from typing import NoReturn

REPORT_SCHEMA_VERSION = 1
DEFAULT_METRICS = Path("target") / "pipeline-bench-raw.json"
DEFAULT_OUTPUT = Path("pipeline-bench.json")

# Container keys that may hold per-stage budgets in a scenario file.
SCENARIO_KEYS = ("budgets", "stage_budgets", "stages")
# Accepted aliases for each budget field, in resolution order.
BUDGET_ALIASES: dict[str, tuple[str, ...]] = {
    "p95_duration_ms": ("p95_duration_ms", "max_p95_duration_ms", "p95_ms", "max_p95_ms"),
    "min_success_rate": ("min_success_rate", "min_success", "success_rate", "success_rate_min"),
}
# Stage-name aliases accepted in raw metric payloads.
STAGE_NAME_KEYS = ("stage", "action_name", "name")


def _fail(message: str) -> NoReturn:
    """Print an input error and exit with the usage exit code."""
    sys.stderr.write(f"bench_pipeline_report: {message}\n")
    raise SystemExit(2)


def _to_float(value: Any, default: float = 0.0) -> float:
    """Coerce ``value`` to ``float``, falling back to ``default``."""
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def _to_int(value: Any, default: int = 0) -> int:
    """Coerce ``value`` to ``int``, falling back to ``default``."""
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def load_metrics(path: Path) -> list[dict[str, Any]]:
    """Load the raw stage metrics exported by the Rust harness."""
    if not path.is_file():
        _fail(f"metrics file not found: {path}")
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except ValueError as exc:
        _fail(f"metrics file is not valid JSON ({path}): {exc}")

    stages = payload.get("stages", []) if isinstance(payload, dict) else payload
    if not isinstance(stages, list):
        _fail(f"metrics payload has no 'stages' list: {path}")
    return [stage for stage in stages if isinstance(stage, dict)]


def normalise_stage(raw: dict[str, Any]) -> dict[str, Any]:
    """Project one raw metric entry into the report shape.

    ``success_rate`` is recomputed from the counters when the producer did not
    emit it, so the report is self-contained regardless of the metrics source.
    """
    stage = ""
    for key in STAGE_NAME_KEYS:
        if raw.get(key):
            stage = str(raw[key])
            break
    invocations = _to_int(raw.get("invocation_count"))
    successes = _to_int(raw.get("success_count"))
    failures = _to_int(raw.get("failure_count"))
    if "success_rate" in raw:
        success_rate = _to_float(raw.get("success_rate"))
    elif invocations > 0:
        success_rate = successes / invocations
    else:
        success_rate = 0.0
    return {
        "stage": stage,
        "invocation_count": invocations,
        "success_count": successes,
        "failure_count": failures,
        "avg_duration_ms": _to_float(raw.get("avg_duration_ms")),
        "p95_duration_ms": _to_float(raw.get("p95_duration_ms")),
        "p99_duration_ms": _to_float(raw.get("p99_duration_ms")),
        "success_rate": success_rate,
    }


def _extract_budget(raw: Any) -> dict[str, float]:
    """Pick the recognised budget fields out of a scenario entry."""
    if not isinstance(raw, dict):
        return {}
    nested = raw.get("budget")
    source = nested if isinstance(nested, dict) else raw
    budget: dict[str, float] = {}
    for field, aliases in BUDGET_ALIASES.items():
        for alias in aliases:
            if alias in source:
                budget[field] = _to_float(source[alias])
                break
    return budget


def load_scenario_budgets(path: Path | None) -> tuple[dict[str, dict[str, float]], list[str]]:
    """Read per-stage budgets from a YAML or JSON scenario file.

    Returns the resolved budgets plus informational notes. A missing file, a
    missing ``PyYAML`` installation, or an unrecognised shape degrades to an
    empty budget map instead of failing the report.
    """
    notes: list[str] = []
    if path is None:
        return {}, notes
    if not path.is_file():
        notes.append(f"scenario file not found, budgets not applied: {path}")
        return {}, notes

    text = path.read_text(encoding="utf-8")
    if path.suffix.lower() == ".json":
        document = json.loads(text)
    else:
        try:
            import yaml
        except ImportError:
            notes.append(f"PyYAML is not installed, budgets not applied: {path}")
            return {}, notes
        document = yaml.safe_load(text) or {}

    if not isinstance(document, dict):
        notes.append(f"scenario root is not a mapping, budgets not applied: {path}")
        return {}, notes

    container: Any = None
    for key in SCENARIO_KEYS:
        if key in document:
            container = document[key]
            break
    if container is None:
        notes.append(f"scenario declares no budgets section: {path}")
        return {}, notes

    budgets: dict[str, dict[str, float]] = {}
    if isinstance(container, dict):
        for stage, raw in container.items():
            budget = _extract_budget(raw)
            if budget:
                budgets[str(stage)] = budget
    elif isinstance(container, list):
        for raw in container:
            if not isinstance(raw, dict):
                continue
            stage = ""
            for key in STAGE_NAME_KEYS:
                if raw.get(key):
                    stage = str(raw[key])
                    break
            budget = _extract_budget(raw)
            if stage and budget:
                budgets[stage] = budget
    else:
        notes.append(f"scenario section has an unsupported shape: {path}")
        return {}, notes

    if not budgets:
        notes.append(f"scenario declares no usable per-stage budgets: {path}")
    return budgets, notes


def parse_budget_overrides(values: list[str]) -> dict[str, dict[str, float]]:
    """Parse ``--budget stage=p95_ms[:min_success_rate]`` overrides."""
    budgets: dict[str, dict[str, float]] = {}
    for value in values:
        if "=" not in value:
            _fail(f"--budget expects stage=p95_ms[:min_success_rate], got: {value}")
        stage, _, spec = value.partition("=")
        stage = stage.strip()
        parts = [part.strip() for part in spec.split(":")]
        if not stage or not parts or not parts[0]:
            _fail(f"--budget expects stage=p95_ms[:min_success_rate], got: {value}")
        budget: dict[str, float] = {"p95_duration_ms": _to_float(parts[0])}
        if len(parts) > 1 and parts[1]:
            budget["min_success_rate"] = _to_float(parts[1], 1.0)
        budgets[stage] = budget
    return budgets


def evaluate(
    stages: list[dict[str, Any]],
    budgets: dict[str, dict[str, float]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[dict[str, Any]]]:
    """Attach budgets to measured stages and collect any breaches.

    Returns ``(rows, breaches, unmeasured)`` where ``unmeasured`` lists scenario
    stages that produced no metrics — reported, but never treated as a breach.
    """
    rows: list[dict[str, Any]] = []
    breaches: list[dict[str, Any]] = []
    seen = set()

    for stage in stages:
        row = dict(stage)
        seen.add(stage["stage"])
        budget = budgets.get(stage["stage"])
        if not budget:
            row["budget"] = None
            row["status"] = "unbudgeted"
            rows.append(row)
            continue

        reasons: list[str] = []
        limit = budget.get("p95_duration_ms")
        if limit is not None and stage["p95_duration_ms"] > limit:
            reasons.append(
                "p95_duration_ms {:.6f} ms exceeds budget {:.6f} ms".format(stage["p95_duration_ms"], limit)
            )
        floor = budget.get("min_success_rate")
        if floor is not None and stage["success_rate"] < floor - 1e-9:
            reasons.append(
                "success_rate {:.6f} below budget {:.6f}".format(stage["success_rate"], floor)
            )
        row["budget"] = dict(budget)
        row["status"] = "breach" if reasons else "pass"
        rows.append(row)
        if reasons:
            breaches.append({"stage": stage["stage"], "reasons": reasons})

    unmeasured = [
        {"stage": stage, "status": "no_metrics", "budget": dict(budget)}
        for stage, budget in sorted(budgets.items())
        if stage not in seen
    ]
    return rows, breaches, unmeasured


def build_report(
    metrics_path: Path,
    scenario_path: Path | None,
    rows: list[dict[str, Any]],
    breaches: list[dict[str, Any]],
    unmeasured: list[dict[str, Any]],
    gate_enabled: bool,
    notes: list[str],
) -> dict[str, Any]:
    """Assemble the final ``pipeline-bench.json`` payload."""
    return {
        "schema_version": REPORT_SCHEMA_VERSION,
        "source": {
            "metrics": str(metrics_path),
            "scenario": str(scenario_path) if scenario_path else None,
        },
        "gate": {
            "enabled": gate_enabled,
            "budgets_applied": sum(1 for row in rows if row["status"] != "unbudgeted"),
            "passed": not breaches,
            "breaches": breaches,
        },
        "stages": rows,
        "unmeasured_stages": unmeasured,
        "notes": notes,
    }


def print_summary(report: dict[str, Any]) -> None:
    """Print a human-readable table of the report."""
    gate = report["gate"]
    print(f"pipeline-bench: {len(report['stages'])} stage(s)")
    for row in report["stages"]:
        budget = row["budget"] or {}
        limit = budget.get("p95_duration_ms")
        floor = budget.get("min_success_rate")
        print(
            "  {:<20} p95={:>9.3f} ms  success_rate={:>5.3f}  budget_p95={:>9}  budget_success={:>5}  {}".format(
                row["stage"],
                row["p95_duration_ms"],
                row["success_rate"],
                f"{limit:.3f}" if limit is not None else "-",
                f"{floor:.3f}" if floor is not None else "-",
                row["status"],
            )
        )
    for note in report["notes"]:
        print(f"  note: {note}")
    for row in report["unmeasured_stages"]:
        print(f"  note: scenario declares '{row['stage']}' but no metrics were produced")
    if not gate["enabled"]:
        print("gate: skipped")
    elif gate["passed"]:
        print(f"gate: PASS ({gate['budgets_applied']} budget(s) applied)")
    else:
        print(f"gate: FAIL ({len(gate['breaches'])} breach(es))")
        for breach in gate["breaches"]:
            for reason in breach["reasons"]:
                print(f"  {breach['stage']}: {reason}")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="Build pipeline-bench.json from raw stage metrics and apply per-stage budgets.",
    )
    parser.add_argument(
        "--metrics",
        type=Path,
        default=DEFAULT_METRICS,
        help="raw metrics export from the Rust harness (default: %(default)s)",
    )
    parser.add_argument(
        "--scenario",
        type=Path,
        default=None,
        help="scenario file declaring per-stage budgets; optional (YAML or JSON)",
    )
    parser.add_argument(
        "--budget",
        action="append",
        default=[],
        metavar="STAGE=P95_MS[:MIN_SUCCESS_RATE]",
        help="per-stage budget override; repeatable and merged over the scenario",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help="path of the generated report (default: %(default)s)",
    )
    parser.add_argument(
        "--no-fail-on-breach",
        action="store_true",
        help="always exit 0; report breaches without failing the job",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Generate the report and apply the budget gate."""
    args = parse_args(argv)

    raw_stages = load_metrics(args.metrics)
    stages = [normalise_stage(raw) for raw in raw_stages]
    stages.sort(key=lambda stage: stage["stage"])
    if not stages:
        _fail(f"metrics payload contains no stages: {args.metrics}")

    budgets, notes = load_scenario_budgets(args.scenario)
    overrides = parse_budget_overrides(args.budget)
    if overrides:
        budgets.update(overrides)
        notes.append(f"applied {len(overrides)} command line budget override(s)")

    rows, breaches, unmeasured = evaluate(stages, budgets)
    report = build_report(
        metrics_path=args.metrics,
        scenario_path=args.scenario,
        rows=rows,
        breaches=breaches,
        unmeasured=unmeasured,
        gate_enabled=bool(budgets),
        notes=notes,
    )

    output: Path = args.output
    if output.parent != Path():
        output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=False) + "\n", encoding="utf-8")

    print_summary(report)
    print(f"wrote {output}")

    if breaches and not args.no_fail_on_breach:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
