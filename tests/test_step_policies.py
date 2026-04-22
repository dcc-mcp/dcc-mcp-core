"""Integration tests for step-level retry / timeout / idempotency policies.

Covers issue #353 — Python bindings for ``StepPolicy`` / ``RetryPolicy`` /
``BackoffKind``, YAML fixture parsing, and the parse-time reference check
for ``idempotency_key`` templates.
"""

from __future__ import annotations

from pathlib import Path

import pytest

import dcc_mcp_core
from dcc_mcp_core import BackoffKind
from dcc_mcp_core import RetryPolicy
from dcc_mcp_core import StepPolicy
from dcc_mcp_core import WorkflowSpec

pytestmark = pytest.mark.skipif(
    dcc_mcp_core.WorkflowSpec is None,
    reason="Built without the `workflow` Cargo feature",
)

FIXTURES = Path(__file__).parent / "fixtures"


def _load_fixture() -> WorkflowSpec:
    spec = WorkflowSpec.from_yaml_str((FIXTURES / "workflow_step_policies.yaml").read_text())
    spec.validate()
    return spec


def test_step_policy_parses_full_block() -> None:
    spec = _load_fixture()
    step = spec.steps[0]
    assert step.id == "export_fbx"
    policy: StepPolicy = step.policy
    assert policy.is_empty is False
    assert policy.timeout_secs == 300
    assert policy.idempotency_key == "export_{{scene_id}}_{{frame_range}}"
    assert policy.idempotency_scope == "global"


def test_step_policy_retry_fields() -> None:
    spec = _load_fixture()
    retry: RetryPolicy | None = spec.steps[0].policy.retry
    assert retry is not None
    assert retry.max_attempts == 3
    assert retry.backoff == BackoffKind.EXPONENTIAL
    assert retry.initial_delay_ms == 500
    assert retry.max_delay_ms == 10_000
    assert abs(retry.jitter - 0.25) < 1e-6
    assert retry.retry_on == ["transient", "timeout"]


def test_retry_next_delay_exponential() -> None:
    retry = _load_fixture().steps[0].policy.retry
    assert retry is not None
    assert retry.next_delay_ms(1) == 0
    assert retry.next_delay_ms(2) == 500
    assert retry.next_delay_ms(3) == 1_000
    assert retry.next_delay_ms(4) == 2_000


def test_retry_is_retryable_filter() -> None:
    retry = _load_fixture().steps[0].policy.retry
    assert retry is not None
    assert retry.is_retryable("transient") is True
    assert retry.is_retryable("permission_denied") is False


def test_step_without_policy_block_defaults_to_empty() -> None:
    spec = _load_fixture()
    policy = spec.steps[1].policy
    assert policy.is_empty is True
    assert policy.timeout_secs is None
    assert policy.retry is None
    assert policy.idempotency_key is None
    assert policy.idempotency_scope == "workflow"


def test_backoff_kind_string_constants() -> None:
    assert BackoffKind.FIXED == "fixed"
    assert BackoffKind.LINEAR == "linear"
    assert BackoffKind.EXPONENTIAL == "exponential"
    assert set(BackoffKind.VALUES) == {"fixed", "linear", "exponential"}


def test_invalid_max_attempts_rejected() -> None:
    yaml = """
name: bad_retry
steps:
  - id: a
    tool: some_tool
    retry:
      max_attempts: 0
"""
    with pytest.raises(ValueError, match="max_attempts"):
        WorkflowSpec.from_yaml_str(yaml)


def test_inverted_delays_rejected() -> None:
    yaml = """
name: bad_delay
steps:
  - id: a
    tool: some_tool
    retry:
      max_attempts: 3
      initial_delay_ms: 5000
      max_delay_ms: 1000
"""
    with pytest.raises(ValueError, match="max_delay_ms"):
        WorkflowSpec.from_yaml_str(yaml)


def test_zero_timeout_rejected() -> None:
    yaml = """
name: bad_timeout
steps:
  - id: a
    tool: some_tool
    timeout_secs: 0
"""
    with pytest.raises(ValueError, match="timeout_secs"):
        WorkflowSpec.from_yaml_str(yaml)


def test_unknown_template_var_rejected_on_validate() -> None:
    yaml = """
name: bad_template
inputs:
  known_var: { type: string }
steps:
  - id: a
    tool: some_tool
    idempotency_key: "k_{{nope}}"
"""
    spec = WorkflowSpec.from_yaml_str(yaml)
    with pytest.raises(ValueError, match="unknown identifier"):
        spec.validate()


def test_template_references_prior_step_id() -> None:
    """Step ids are valid roots in idempotency-key templates."""
    yaml = """
name: step_ref
steps:
  - id: first
    tool: some_tool
  - id: second
    tool: other_tool
    idempotency_key: "k_{{first}}"
"""
    spec = WorkflowSpec.from_yaml_str(yaml)
    spec.validate()  # no raise


def test_jitter_out_of_range_is_clamped() -> None:
    yaml = """
name: jittery
steps:
  - id: a
    tool: some_tool
    retry:
      max_attempts: 2
      jitter: 2.0
"""
    spec = WorkflowSpec.from_yaml_str(yaml)
    retry = spec.steps[0].policy.retry
    assert retry is not None
    assert retry.jitter == 1.0


def test_policy_roundtrips_to_yaml() -> None:
    spec = _load_fixture()
    dumped = spec.to_yaml()
    reparsed = WorkflowSpec.from_yaml_str(dumped)
    reparsed.validate()
    step = reparsed.steps[0]
    assert step.policy.timeout_secs == 300
    assert step.policy.retry is not None
    assert step.policy.retry.max_attempts == 3
