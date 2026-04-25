"""Tests for YAML declarative workflow definitions (issue #439)."""

from __future__ import annotations

import json
from pathlib import Path
import textwrap
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from dcc_mcp_core.workflow_yaml import WorkflowTask
from dcc_mcp_core.workflow_yaml import WorkflowYaml
from dcc_mcp_core.workflow_yaml import get_workflow_path
from dcc_mcp_core.workflow_yaml import load_workflow_yaml
from dcc_mcp_core.workflow_yaml import register_workflow_yaml_tools

# ── YAML fixture ──────────────────────────────────────────────────────────

_WORKFLOW_YAML = textwrap.dedent(
    """\
    name: model_to_render
    goal: "Import a model, clean topology, assign materials, light, and render"
    config:
      dcc: maya
    variables:
      model_path: ""
      output_dir: "/tmp/render"
    tasks:
      - name: import_model
        kind: task
        tool: maya_geometry__import_fbx
        description: "Import the FBX file"
        inputs:
          path: "{{model_path}}"
        outputs: [mesh_name]
      - name: assign_material
        kind: step
        tool: maya_shading__assign_material
        inputs:
          mesh: "{{mesh_name}}"
        on_failure: [dcc_diagnostics__screenshot]
      - name: render
        kind: task
        tool: maya_render__render_frame
        inputs:
          output_dir: "{{output_dir}}"
    """
)


@pytest.fixture()
def workflow_yaml_file(tmp_path: Path) -> Path:
    p = tmp_path / "model_to_render.yaml"
    p.write_text(_WORKFLOW_YAML, encoding="utf-8")
    return p


# ── WorkflowTask ──────────────────────────────────────────────────────────


class TestWorkflowTask:
    def test_valid_task(self) -> None:
        t = WorkflowTask(name="foo", kind="task", tool="maya.create_sphere")
        assert t.kind == "task"

    def test_valid_step(self) -> None:
        t = WorkflowTask(name="bar", kind="step", tool="maya.bevel")
        assert t.kind == "step"

    def test_invalid_kind_raises(self) -> None:
        with pytest.raises(ValueError, match="kind"):
            WorkflowTask(name="x", kind="invalid", tool="t")

    def test_interpolate_inputs_replaces_template(self) -> None:
        t = WorkflowTask(name="t", tool="x", inputs={"path": "{{model_path}}/scene.fbx"})
        result = t.interpolate_inputs({"model_path": "/tmp/models"})
        assert result["path"] == "/tmp/models/scene.fbx"

    def test_interpolate_missing_var_left_as_is(self) -> None:
        t = WorkflowTask(name="t", tool="x", inputs={"v": "{{undefined}}"})
        result = t.interpolate_inputs({})
        assert result["v"] == "{{undefined}}"

    def test_interpolate_non_string_values_unchanged(self) -> None:
        t = WorkflowTask(name="t", tool="x", inputs={"count": 5})
        result = t.interpolate_inputs({"count": 99})
        assert result["count"] == 5


# ── WorkflowYaml ──────────────────────────────────────────────────────────


class TestWorkflowYaml:
    def test_validate_valid(self) -> None:
        wf = WorkflowYaml(
            name="my-wf",
            tasks=[WorkflowTask(name="a", tool="t"), WorkflowTask(name="b", tool="u")],
        )
        assert wf.validate() == []

    def test_validate_missing_name(self) -> None:
        wf = WorkflowYaml(name="")
        errors = wf.validate()
        assert any("name" in e for e in errors)

    def test_validate_duplicate_task_names(self) -> None:
        wf = WorkflowYaml(
            name="x",
            tasks=[WorkflowTask(name="dup", tool="t"), WorkflowTask(name="dup", tool="u")],
        )
        errors = wf.validate()
        assert any("Duplicate" in e for e in errors)

    def test_validate_task_missing_tool(self) -> None:
        wf = WorkflowYaml(name="x", tasks=[WorkflowTask(name="t", tool="")])
        errors = wf.validate()
        assert any("tool" in e for e in errors)

    def test_task_names(self) -> None:
        wf = WorkflowYaml(
            name="x",
            tasks=[WorkflowTask(name="a", tool="t"), WorkflowTask(name="b", tool="u")],
        )
        assert wf.task_names() == ["a", "b"]

    def test_get_task_found(self) -> None:
        wf = WorkflowYaml(name="x", tasks=[WorkflowTask(name="a", tool="t")])
        assert wf.get_task("a") is not None

    def test_get_task_missing(self) -> None:
        wf = WorkflowYaml(name="x", tasks=[])
        assert wf.get_task("nope") is None

    def test_to_summary_dict_structure(self) -> None:
        wf = WorkflowYaml(
            name="x",
            goal="do stuff",
            config={"dcc": "maya"},
            tasks=[WorkflowTask(name="a", kind="task", tool="t.foo")],
        )
        summary = wf.to_summary_dict()
        assert summary["name"] == "x"
        assert summary["goal"] == "do stuff"
        assert summary["dcc"] == "maya"
        assert summary["task_count"] == 1
        assert summary["tasks"][0]["kind"] == "task"


# ── load_workflow_yaml ────────────────────────────────────────────────────


class TestLoadWorkflowYaml:
    def test_loads_valid_file(self, workflow_yaml_file: Path) -> None:
        wf = load_workflow_yaml(workflow_yaml_file)
        assert wf.name == "model_to_render"
        assert len(wf.tasks) == 3

    def test_task_kind_preserved(self, workflow_yaml_file: Path) -> None:
        wf = load_workflow_yaml(workflow_yaml_file)
        assert wf.tasks[0].kind == "task"
        assert wf.tasks[1].kind == "step"

    def test_on_failure_parsed(self, workflow_yaml_file: Path) -> None:
        wf = load_workflow_yaml(workflow_yaml_file)
        assert "dcc_diagnostics__screenshot" in wf.tasks[1].on_failure

    def test_variables_populated(self, workflow_yaml_file: Path) -> None:
        wf = load_workflow_yaml(workflow_yaml_file)
        assert "model_path" in wf.variables

    def test_source_path_set(self, workflow_yaml_file: Path) -> None:
        wf = load_workflow_yaml(workflow_yaml_file)
        assert wf.source_path is not None

    def test_missing_file_raises(self, tmp_path: Path) -> None:
        with pytest.raises(FileNotFoundError):
            load_workflow_yaml(tmp_path / "nonexistent.yaml")

    def test_invalid_yaml_raises(self, tmp_path: Path) -> None:
        p = tmp_path / "bad.yaml"
        p.write_text("{bad yaml: [}", encoding="utf-8")
        with pytest.raises(ValueError, match="parse"):
            load_workflow_yaml(p)

    def test_non_mapping_yaml_raises(self, tmp_path: Path) -> None:
        p = tmp_path / "list.yaml"
        p.write_text("- item1\n- item2\n", encoding="utf-8")
        with pytest.raises(ValueError, match="mapping"):
            load_workflow_yaml(p)

    def test_missing_name_raises(self, tmp_path: Path) -> None:
        p = tmp_path / "no_name.yaml"
        p.write_text("goal: something\ntasks:\n  - name: t\n    tool: x\n", encoding="utf-8")
        with pytest.raises(ValueError, match="validation"):
            load_workflow_yaml(p)


# ── get_workflow_path ─────────────────────────────────────────────────────


class TestGetWorkflowPath:
    def _make_md(self, skill_path, wf_rel, *, nested=False):
        md = MagicMock()
        md.skill_path = skill_path
        if wf_rel is None:
            md.metadata = {}
        elif nested:
            md.metadata = {"dcc-mcp": {"workflows": wf_rel}}
        else:
            md.metadata = {"dcc-mcp.workflows": wf_rel}
        return md

    def test_flat_form(self, tmp_path: Path) -> None:
        md = self._make_md(str(tmp_path), "workflows/wf.yaml")
        result = get_workflow_path(md)
        assert result == str(tmp_path / "workflows/wf.yaml")

    def test_nested_form(self, tmp_path: Path) -> None:
        md = self._make_md(str(tmp_path), "wf.yaml", nested=True)
        result = get_workflow_path(md)
        assert result == str(tmp_path / "wf.yaml")

    def test_no_metadata_returns_none(self) -> None:
        md = MagicMock()
        md.metadata = {}
        md.skill_path = None
        assert get_workflow_path(md) is None

    def test_glob_pattern_matches_first(self, tmp_path: Path) -> None:
        (tmp_path / "wf1.yaml").write_text("", encoding="utf-8")
        (tmp_path / "wf2.yaml").write_text("", encoding="utf-8")
        md = self._make_md(str(tmp_path), "wf*.yaml")
        result = get_workflow_path(md)
        assert result is not None
        assert "wf1.yaml" in result or "wf2.yaml" in result

    def test_glob_no_match_returns_none(self, tmp_path: Path) -> None:
        md = self._make_md(str(tmp_path), "*.workflow.yaml")
        result = get_workflow_path(md)
        assert result is None


# ── register_workflow_yaml_tools ──────────────────────────────────────────


class TestRegisterWorkflowYamlTools:
    def _make_server(self) -> tuple[MagicMock, dict]:
        server = MagicMock()
        registry = MagicMock()
        server.registry = registry
        handlers: dict = {}
        server.register_handler.side_effect = lambda name, fn: handlers.__setitem__(name, fn)
        return server, handlers

    def _make_wf(self) -> WorkflowYaml:
        return WorkflowYaml(
            name="test-wf",
            goal="A test workflow",
            config={"dcc": "maya"},
            tasks=[
                WorkflowTask(name="step1", kind="task", tool="t.tool1"),
                WorkflowTask(name="step2", kind="step", tool="t.tool2"),
            ],
        )

    def test_registers_two_tools(self) -> None:
        server, _handlers = self._make_server()
        register_workflow_yaml_tools(server, workflows=[self._make_wf()])
        names = {c.kwargs["name"] for c in server.registry.register.call_args_list}
        assert "workflows.list" in names
        assert "workflows.describe" in names

    def test_list_returns_workflows(self) -> None:
        server, handlers = self._make_server()
        register_workflow_yaml_tools(server, workflows=[self._make_wf()])
        result = handlers["workflows.list"](None)
        assert result["success"] is True
        assert result["context"]["count"] == 1
        assert result["context"]["workflows"][0]["name"] == "test-wf"

    def test_describe_known_workflow(self) -> None:
        server, handlers = self._make_server()
        register_workflow_yaml_tools(server, workflows=[self._make_wf()])
        result = handlers["workflows.describe"](json.dumps({"name": "test-wf"}))
        assert result["success"] is True
        assert result["context"]["task_count"] == 2

    def test_describe_unknown_workflow(self) -> None:
        server, handlers = self._make_server()
        register_workflow_yaml_tools(server, workflows=[self._make_wf()])
        result = handlers["workflows.describe"](json.dumps({"name": "no-such-wf"}))
        assert result["success"] is False
        assert "available" in result["context"]

    def test_no_registry_logs_warning(self) -> None:
        import logging

        class _BadServer:
            @property
            def registry(self):
                raise AttributeError("no registry")

        with patch.object(logging.getLogger("dcc_mcp_core.workflow_yaml"), "warning") as mock_warn:
            register_workflow_yaml_tools(_BadServer(), workflows=[self._make_wf()])
        mock_warn.assert_called_once()

    def test_skills_with_no_workflow_path_skipped(self, tmp_path: Path) -> None:
        md = MagicMock()
        md.metadata = {}
        md.skill_path = str(tmp_path)
        md.name = "no-wf-skill"
        server, handlers = self._make_server()
        register_workflow_yaml_tools(server, skills=[md])
        result = handlers["workflows.list"](None)
        assert result["context"]["count"] == 0
