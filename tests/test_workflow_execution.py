"""Python-side coverage for the #348 workflow execution engine.

The Rust `WorkflowExecutor` / `WorkflowHost` have exhaustive unit tests
in ``crates/dcc-mcp-workflow/src/``. The Python surface currently
exposes the **spec + policy viewer** types (parse / validate /
introspect); a native ``WorkflowHost`` Python class is tracked as a
follow-up. These tests exercise every ``StepKind`` through the spec
viewer so that when the Python-facing run surface lands it can be
validated against the same YAML fixtures.
"""

from __future__ import annotations

import textwrap

import pytest

import dcc_mcp_core

pytestmark = pytest.mark.skipif(
    dcc_mcp_core.WorkflowSpec is None,
    reason="Built without the `workflow` Cargo feature",
)


FULL_YAML = textwrap.dedent(
    """
    name: full_exec_surface
    description: "Smoke test covering every StepKind for the #348 executor."
    inputs:
      date: { type: string, format: date }
    steps:
      - id: list_local
        tool: scene__list
        args:
          date: "{{inputs.date}}"
        timeout_secs: 30
        retry:
          max_attempts: 3
          backoff: exponential
          initial_delay_ms: 250
          max_delay_ms: 5000
          jitter: 0.2
      - id: list_remote
        kind: tool_remote
        dcc: unreal
        tool: unreal__fetch_latest
      - id: per_file
        kind: foreach
        items: "$.list_local.files"
        as: file
        steps:
          - id: import
            tool: maya__import_scene
          - id: qc
            tool: maya_qc__run_all
      - id: branches
        kind: parallel
        branches:
          - - id: export_fbx
              tool: maya__export_fbx
          - - id: export_usd
              tool: usd__export
      - id: human_gate
        kind: approve
        prompt: "Proceed with vendor drop?"
        timeout_secs: 120
      - id: final_gate
        kind: branch
        on: "$.qc.passed"
        then:
          - id: publish
            tool: pipeline__publish
        else:
          - id: notify
            tool: ops__notify_failure
    """
).strip()


@pytest.fixture(scope="module")
def spec():
    return dcc_mcp_core.WorkflowSpec.from_yaml_str(FULL_YAML)


def test_spec_parses_and_validates(spec):
    spec.validate()
    assert spec.name == "full_exec_surface"
    assert spec.step_count == 6


def test_every_step_kind_is_represented(spec):
    kinds = {s.kind for s in spec.steps}
    # `tool` variant is shorthand → still `kind == "tool"`. Remote, foreach,
    # parallel, approve, branch all appear exactly once.
    assert kinds == {"tool", "tool_remote", "foreach", "parallel", "approve", "branch"}


def test_policy_round_trips_retry_and_timeout(spec):
    first = spec.steps[0]
    assert first.policy.timeout_secs == 30
    assert first.policy.retry is not None
    assert first.policy.retry.max_attempts == 3
    assert first.policy.retry.backoff == dcc_mcp_core.BackoffKind.EXPONENTIAL
    # attempt=1 is the initial run → zero base delay; attempt=2 first retry.
    assert first.policy.retry.next_delay_ms(2) == 250


def test_terminal_statuses_mark_is_terminal():
    WorkflowStatus = dcc_mcp_core.WorkflowStatus
    for value in ("completed", "failed", "cancelled", "interrupted"):
        assert WorkflowStatus(value).is_terminal is True
    for value in ("pending", "running"):
        assert WorkflowStatus(value).is_terminal is False


def test_to_yaml_round_trip_preserves_kinds(spec):
    yaml_out = spec.to_yaml()
    spec2 = dcc_mcp_core.WorkflowSpec.from_yaml_str(yaml_out)
    spec2.validate()
    assert {s.kind for s in spec2.steps} == {s.kind for s in spec.steps}
