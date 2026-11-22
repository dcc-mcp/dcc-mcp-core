"""Tests for the cross-DCC pipeline benchmark contract."""

from __future__ import annotations

from dcc_mcp_core.verification import DEFAULT_PIPELINE_STAGES
from dcc_mcp_core.verification import STAGE_DEGRADED
from dcc_mcp_core.verification import STAGE_PASS
from dcc_mcp_core.verification import STAGE_SKIPPED
from dcc_mcp_core.verification import BehaviorReport
from dcc_mcp_core.verification import Check
from dcc_mcp_core.verification import run_pipeline_benchmark


def test_default_pipeline_has_seven_explicit_stages():
    assert [stage.name for stage in DEFAULT_PIPELINE_STAGES] == [
        "modeling",
        "rig",
        "animation",
        "texturing",
        "material",
        "render",
        "composite",
    ]


def test_report_tracks_calls_scripts_and_behavior_assertions():
    def execute(stage):
        return {
            "status": STAGE_PASS,
            "calls": [
                {"name": "typed_write", "read_only": False, "script": False},
                {"name": "raw_script", "read_only": False, "script": True},
                {"name": "readback", "read_only": True},
            ],
            "behavior_report": {"passed": 9, "total": 10},
        }

    report = run_pipeline_benchmark(execute)
    assert report.bare_script_share == 1 / 2
    assert report.behavior_assertion_pass_rate == 0.9
    assert report.gate_passed is False
    assert report.to_dict()["stages"][0]["calls"] == 3


def test_skips_and_degradations_are_explicit_and_fail_the_gate():
    def execute(stage):
        if stage.name == "texturing":
            return {"status": STAGE_DEGRADED, "reason": "Painter unavailable"}
        if stage.name == "material":
            return {"status": STAGE_SKIPPED, "reason": "Requires Designer adapter"}
        return {
            "status": STAGE_PASS,
            "calls": [{"name": "typed", "read_only": False, "script": False}],
            "behavior_report": {"passed": 1, "total": 1},
        }

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    assert report.degraded_stage_count == 1
    assert report.skipped_stage_count == 1
    assert report.gate_passed is False
    statuses = {result.stage.name: result.status for result in report.stages}
    assert statuses["texturing"] == STAGE_DEGRADED
    assert statuses["material"] == STAGE_SKIPPED


def test_callback_exception_becomes_a_failed_stage():
    def execute(stage):
        if stage.name == "render":
            raise RuntimeError("renderer unavailable")
        return {
            "status": STAGE_PASS,
            "calls": [{"name": "typed", "read_only": False}],
            "behavior_report": {"pass_rate": 1.0},
        }

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    render = next(result for result in report.stages if result.stage.name == "render")
    assert render.status == "failed"
    assert render.reason == "renderer unavailable"


def test_behavior_report_is_accepted_and_assertion_counts_are_weighted():
    checks = [
        Check("first", "exact", True, 1, 1),
        Check("second", "exact", False, 1, 2),
    ]

    def execute(stage):
        return {"status": STAGE_PASS, "behavior_report": BehaviorReport(checks=checks)}

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    assert report.behavior_assertion_pass_rate == 0.5


def test_malformed_call_records_fail_the_stage():
    def execute(stage):
        return {"status": STAGE_PASS, "calls": ["not-a-call-record"]}

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    assert all(result.status == "failed" for result in report.stages)


def test_non_boolean_call_flags_fail_the_stage():
    report = run_pipeline_benchmark(
        lambda stage: {
            "status": STAGE_PASS,
            "calls": [{"name": "typed", "read_only": "false"}],
        }
    )
    assert all(result.status == "failed" for result in report.stages)


def test_blank_skip_reason_fails_the_stage():
    report = run_pipeline_benchmark(lambda stage: {"status": STAGE_SKIPPED, "reason": "  \t"})
    assert all(result.status == "failed" for result in report.stages)


def test_mixed_behavior_report_formats_fail_closed():
    def execute(stage):
        if stage.name == "modeling":
            return {"status": STAGE_PASS, "behavior_report": {"passed": 1, "total": 1}}
        return {"status": STAGE_PASS, "behavior_report": {"pass_rate": 1.0}}

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    assert report.behavior_assertion_pass_rate == 0.0
    assert report.gate_passed is False


def test_missing_behavior_evidence_fails_even_with_zero_target():
    report = run_pipeline_benchmark(
        lambda stage: {"status": STAGE_PASS},
        behavior_assertion_target=0.0,
        bare_script_share_target=1.0,
    )
    assert report.behavior_assertion_evidence_valid is False
    assert report.gate_passed is False


def test_empty_behavior_report_fails_the_stage():
    report = run_pipeline_benchmark(lambda stage: {"status": STAGE_PASS, "behavior_report": BehaviorReport()})
    assert all(result.status == "failed" for result in report.stages)


def test_non_finite_rate_fails_the_stage():
    report = run_pipeline_benchmark(
        lambda stage: {"status": STAGE_PASS, "behavior_report": {"pass_rate": float("nan")}}
    )
    assert all(result.status == "failed" for result in report.stages)


def test_invalid_behavior_counts_fail_the_stage():
    def execute(stage):
        return {"status": STAGE_PASS, "behavior_report": {"passed": 2, "total": 1}}

    report = run_pipeline_benchmark(execute, bare_script_share_target=1.0)
    assert all(result.status == "failed" for result in report.stages)
