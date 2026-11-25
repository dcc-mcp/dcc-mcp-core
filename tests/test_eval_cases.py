"""Regression tests for the standard offline evaluation case set."""

from __future__ import annotations

from tests.eval.cases.core_cases import CASES

from dcc_mcp_core import run_eval_set


def test_core_case_set_has_twenty_deterministic_cases_and_five_named_defects() -> None:
    first = run_eval_set(CASES, set_version="eval-set-v1", generated_at="2026-01-01T00:00:00Z", clock=lambda: 0.0)
    second = run_eval_set(CASES, set_version="eval-set-v1", generated_at="2026-01-01T00:00:00Z", clock=lambda: 0.0)

    assert len(CASES) == 20
    assert len({case.case_id for case in CASES}) == 20
    assert first.dumps() == second.dumps()

    failures = [run for run in first.runs if run.verdict == "fail"]
    assert all(run.verdict == "pass" for run in first.runs if not run.case.case_id.startswith("defect-"))
    assert all(run.verdict != "inconclusive" for run in first.runs)
    assert len(failures) == 5
    assert {run.case.case_id for run in failures} == {
        "defect-missing-part",
        "defect-low-uv-coverage",
        "defect-non-manifold",
        "defect-blank-capture",
        "defect-low-similarity",
    }
    assert all(run.reason and run.reason.startswith("defect:") for run in failures)
    assert len(first.failure_histogram) >= 10
