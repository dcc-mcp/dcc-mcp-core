"""Round-trip + validation tests for the WorkflowSpec skeleton (issue #348).

These tests are skipped when the extension was built without the
``workflow`` Cargo feature — e.g. a minimal wheel that does not ship the
Workflow primitive yet.
"""

from __future__ import annotations

import textwrap

import pytest

import dcc_mcp_core

pytestmark = pytest.mark.skipif(
    dcc_mcp_core.WorkflowSpec is None,
    reason="Built without the `workflow` Cargo feature",
)


VALID_YAML = textwrap.dedent(
    """
    name: vendor_intake
    description: "Import vendor Maya files, QC, export FBX, push to Unreal."
    inputs:
      date: { type: string, format: date }
    steps:
      - id: list
        tool: vendor_intake__list_sftp
      - id: per_file
        kind: foreach
        items: "$.list.files"
        as: file
        steps:
          - id: import
            tool: maya__import_scene
          - id: gate
            kind: branch
            on: "$.qc.passed"
            then:
              - id: export
                tool: maya__export_fbx
    """
).strip()


def test_parse_and_validate() -> None:
    spec = dcc_mcp_core.WorkflowSpec.from_yaml_str(VALID_YAML)
    assert spec.name == "vendor_intake"
    assert spec.step_count == 2
    spec.validate()


def test_round_trip_yaml() -> None:
    spec = dcc_mcp_core.WorkflowSpec.from_yaml_str(VALID_YAML)
    serialized = spec.to_yaml()
    again = dcc_mcp_core.WorkflowSpec.from_yaml_str(serialized)
    assert again.name == spec.name
    assert again.step_count == spec.step_count
    again.validate()


def test_duplicate_step_id_rejected() -> None:
    yaml = textwrap.dedent(
        """
        name: dup
        steps:
          - id: a
            tool: t1
          - id: a
            tool: t2
        """
    ).strip()
    spec = dcc_mcp_core.WorkflowSpec.from_yaml_str(yaml)
    with pytest.raises(ValueError, match="duplicate step id"):
        spec.validate()


def test_bad_tool_name_rejected() -> None:
    yaml = textwrap.dedent(
        """
        name: bad
        steps:
          - id: a
            tool: "bad/tool"
        """
    ).strip()
    spec = dcc_mcp_core.WorkflowSpec.from_yaml_str(yaml)
    with pytest.raises(ValueError, match="tool name"):
        spec.validate()


def test_invalid_yaml_raises_value_error() -> None:
    with pytest.raises(ValueError):
        dcc_mcp_core.WorkflowSpec.from_yaml_str("name: broken\nsteps: [not closed\n")


def test_workflow_status_terminal_flags() -> None:
    assert not dcc_mcp_core.WorkflowStatus("pending").is_terminal
    assert not dcc_mcp_core.WorkflowStatus("running").is_terminal
    for term in ("completed", "failed", "cancelled", "interrupted"):
        assert dcc_mcp_core.WorkflowStatus(term).is_terminal

    with pytest.raises(ValueError):
        dcc_mcp_core.WorkflowStatus("bogus")


def test_workflow_status_value_is_lowercase() -> None:
    s = dcc_mcp_core.WorkflowStatus("running")
    assert s.value == "running"
    assert str(s) == "running"
