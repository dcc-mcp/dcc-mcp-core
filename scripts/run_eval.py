#!/usr/bin/env python3
"""Run, validate, or compare offline DCC-MCP evaluation documents."""

from __future__ import annotations

import argparse
import difflib
import importlib.util
import json
from pathlib import Path
import sys
from typing import Any
from typing import Iterable
from typing import Mapping

from dcc_mcp_core.verification import EvalCase
from dcc_mcp_core.verification import eval_report_json_schema
from dcc_mcp_core.verification import eval_set_json_schema
from dcc_mcp_core.verification import run_eval_set


def _load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read JSON document {path}: {exc}") from exc


def _basic_validate(document: Any, kind: str) -> None:
    if not isinstance(document, Mapping):
        raise ValueError("evaluation document must be a JSON object")
    if document.get("schema_version") != 1:
        raise ValueError("evaluation document must declare schema_version 1")
    if not isinstance(document.get("set_version"), str) or not document["set_version"].strip():
        raise ValueError("evaluation document must declare a non-empty set_version")
    if kind == "set":
        cases = document.get("cases")
        if not isinstance(cases, list) or not cases:
            raise ValueError("evaluation set must contain at least one case")
        for case in cases:
            _basic_validate_case(case)
        return
    if kind != "report":
        raise ValueError(f"unknown evaluation document kind: {kind!r}")
    if "generated_at" not in document or (
        document["generated_at"] is not None and not isinstance(document["generated_at"], str)
    ):
        raise ValueError("evaluation report must declare generated_at as a string or null")
    metrics = document.get("metrics")
    if not isinstance(metrics, Mapping):
        raise ValueError("evaluation report must contain metrics")
    required_metrics = (
        "case_count",
        "pass_count",
        "fail_count",
        "inconclusive_count",
        "pass_rate",
        "failure_histogram",
    )
    if any(name not in metrics for name in required_metrics):
        raise ValueError("evaluation report metrics are incomplete")
    for name in required_metrics[:4]:
        value = metrics[name]
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValueError(f"evaluation report metric {name} must be a non-negative integer")
    if (
        not isinstance(metrics["pass_rate"], (int, float))
        or isinstance(metrics["pass_rate"], bool)
        or not 0 <= metrics["pass_rate"] <= 1
    ):
        raise ValueError("evaluation report pass_rate must be between 0 and 1")
    histogram = metrics["failure_histogram"]
    if not isinstance(histogram, Mapping) or any(
        not isinstance(value, int) or isinstance(value, bool) or value < 1 for value in histogram.values()
    ):
        raise ValueError("evaluation report failure_histogram is invalid")
    runs = document.get("runs")
    if not isinstance(runs, list):
        raise ValueError("evaluation report must contain a runs array")
    for run in runs:
        if not isinstance(run, Mapping):
            raise ValueError("every evaluation run must be an object")
        required_run_fields = ("case", "verdict", "checks", "duration_ms", "artifacts", "reason")
        if any(name not in run for name in required_run_fields):
            raise ValueError("evaluation run is incomplete")
        _basic_validate_case(run["case"])
        if run["verdict"] not in ("pass", "fail", "inconclusive"):
            raise ValueError("every evaluation run must declare a valid verdict")
        if not isinstance(run["checks"], list):
            raise ValueError("evaluation run checks must be an array")
        for check in run["checks"]:
            if (
                not isinstance(check, Mapping)
                or not isinstance(check.get("name"), str)
                or not isinstance(check.get("passed"), bool)
            ):
                raise ValueError("every evaluation check must declare name and passed")
        if not isinstance(run["duration_ms"], int) or isinstance(run["duration_ms"], bool) or run["duration_ms"] < 0:
            raise ValueError("evaluation run duration_ms must be a non-negative integer")
        if not isinstance(run["artifacts"], list) or any(not isinstance(item, str) for item in run["artifacts"]):
            raise ValueError("evaluation run artifacts must be an array of strings")
        if run["reason"] is not None and not isinstance(run["reason"], str):
            raise ValueError("evaluation run reason must be a string or null")


def _basic_validate_case(case: Any) -> None:
    """Validate the required fields shared by set and report case records."""
    if not isinstance(case, Mapping):
        raise ValueError("every evaluation case must be an object")
    required_fields = ("case_id", "title", "invocation", "tags")
    if any(name not in case for name in required_fields):
        raise ValueError("evaluation case is incomplete")
    if not isinstance(case["case_id"], str) or not case["case_id"].strip():
        raise ValueError("evaluation case_id must be a non-empty string")
    if not isinstance(case["title"], str) or not case["title"].strip():
        raise ValueError("evaluation title must be a non-empty string")
    if not isinstance(case["invocation"], Mapping):
        raise ValueError("evaluation invocation must be an object")
    if not isinstance(case["tags"], list) or any(not isinstance(tag, str) for tag in case["tags"]):
        raise ValueError("evaluation tags must be an array of strings")


def _validate(document: Any, kind: str) -> None:
    schema = eval_set_json_schema() if kind == "set" else eval_report_json_schema()
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        _basic_validate(document, kind)
        return
    errors = sorted(Draft202012Validator(schema).iter_errors(document), key=lambda error: list(error.path))
    if errors:
        location = ".".join(str(item) for item in errors[0].path) or "$"
        raise ValueError(f"{kind} document is invalid at {location}: {errors[0].message}")


def _load_case_module(path: Path) -> Iterable[EvalCase]:
    spec = importlib.util.spec_from_file_location("dcc_mcp_eval_cases", str(path))
    if spec is None or spec.loader is None:
        raise ValueError(f"cannot load case module {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    cases = getattr(module, "CASES", None)
    if cases is None and callable(getattr(module, "cases", None)):
        cases = module.cases()
    if not isinstance(cases, (list, tuple)) or any(not isinstance(case, EvalCase) for case in cases):
        raise ValueError("case module must expose CASES or cases() containing EvalCase objects")
    return cases


def _run(args: argparse.Namespace) -> int:
    cases = _load_case_module(args.module)
    report = run_eval_set(
        cases,
        set_version=args.set_version,
        generated_at=args.generated_at,
        clock=None if args.measure_duration else (lambda: 0.0),
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(report.dumps(), encoding="utf-8")
    print(f"wrote {args.output} ({len(report.runs)} cases, pass rate {report.pass_rate:.3f})")
    return 0


def _validate_command(args: argparse.Namespace) -> int:
    document = _load_json(args.document)
    kind = args.kind
    if kind == "auto":
        if not isinstance(document, Mapping):
            raise ValueError("evaluation document must be a JSON object")
        kind = "report" if "runs" in document else "set"
    _validate(document, kind)
    print(f"valid {kind} document: {args.document}")
    return 0


def _diff(args: argparse.Namespace) -> int:
    first_bytes = args.first.read_bytes()
    second_bytes = args.second.read_bytes()
    if first_bytes == second_bytes:
        print("evaluation reports are identical")
        return 0
    first = first_bytes.decode("utf-8").splitlines(keepends=True)
    second = second_bytes.decode("utf-8").splitlines(keepends=True)
    sys.stdout.writelines(difflib.unified_diff(first, second, str(args.first), str(args.second)))
    return 1


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    run = subparsers.add_parser("run", help="run a Python case module and write a report")
    run.add_argument("--module", type=Path, required=True)
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--set-version", default="eval-set-v1")
    run.add_argument("--generated-at")
    run.add_argument("--measure-duration", action="store_true")
    run.set_defaults(handler=_run)

    validate = subparsers.add_parser("validate", help="validate a persisted set or report")
    validate.add_argument("document", type=Path)
    validate.add_argument("--kind", choices=("auto", "set", "report"), default="auto")
    validate.set_defaults(handler=_validate_command)

    diff = subparsers.add_parser("diff", help="compare two persisted reports byte-for-byte")
    diff.add_argument("first", type=Path)
    diff.add_argument("second", type=Path)
    diff.set_defaults(handler=_diff)
    return parser


def main() -> int:
    """Run the evaluation command-line interface."""
    args = _parser().parse_args()
    try:
        return args.handler(args)
    except (OSError, ValueError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
