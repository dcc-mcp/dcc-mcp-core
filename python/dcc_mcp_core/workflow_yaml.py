"""YAML declarative workflow definitions with task/step semantics (issue #439).

Inspired by AgentSPEX, this module provides a lightweight YAML format for
defining multi-step DCC workflows.  The key innovation is the **task vs step**
semantic distinction for context management:

- **task** — opens a new conversation / clean context; results are passed via
  variables only.  Use for isolated sub-tasks (import, render) where accumulated
  scene history is irrelevant.
- **step** — operates within the same conversation; accumulated history is
  available.  Use for multi-round reasoning (assign material + set up lighting).

YAML format (sibling file, referenced via ``metadata.dcc-mcp.workflows``)::

    name: model_to_render
    goal: "Import a model, clean topology, assign materials, light, and render"
    config:
      dcc: maya
    variables:
      model_path: ""
      output_dir: "/tmp/render"
    tasks:
      - name: import_and_clean
        kind: task
        tool: maya_geometry__import_fbx
        inputs:
          path: "{{model_path}}"
        outputs: [mesh_name]
      - name: material_and_light
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

Public API:

- :class:`WorkflowTask` — a single task/step definition
- :class:`WorkflowYaml` — a parsed workflow definition
- :func:`load_workflow_yaml` — load and validate a workflow YAML file
- :func:`get_workflow_path` — extract workflow file path from SkillMetadata
- :func:`register_workflow_yaml_tools` — register ``workflows.list`` and
  ``workflows.describe`` MCP tools

"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import json
import logging
from pathlib import Path
import re
from typing import Any

logger = logging.getLogger(__name__)

# ── Types ──────────────────────────────────────────────────────────────────

_TEMPLATE_RE = re.compile(r"\{\{(\w+)\}\}")


@dataclass
class WorkflowTask:
    """A single task or step in a WorkflowYaml definition.

    Parameters
    ----------
    name:
        Unique identifier within the workflow.
    kind:
        ``"task"`` (clean context) or ``"step"`` (accumulated context).
    tool:
        MCP tool name to invoke.
    inputs:
        Variable-interpolated input dict.
    outputs:
        List of output variable names produced by this task.
    on_failure:
        List of follow-up tools on failure (mirrors next-tools on-failure).
    description:
        Human-readable summary.

    """

    name: str
    kind: str = "step"
    tool: str = ""
    inputs: dict[str, Any] = field(default_factory=dict)
    outputs: list[str] = field(default_factory=list)
    on_failure: list[str] = field(default_factory=list)
    description: str = ""

    def __post_init__(self) -> None:
        if self.kind not in ("task", "step"):
            raise ValueError(f"WorkflowTask.kind must be 'task' or 'step', got '{self.kind}'")

    def interpolate_inputs(self, variables: dict[str, Any]) -> dict[str, Any]:
        """Return inputs with ``{{var}}`` templates replaced from *variables*.

        Missing variables are left as-is (no error raised at interpolation time).
        """
        result: dict[str, Any] = {}
        for k, v in self.inputs.items():
            if isinstance(v, str):
                result[k] = _TEMPLATE_RE.sub(lambda m: str(variables.get(m.group(1), m.group(0))), v)
            else:
                result[k] = v
        return result


@dataclass
class WorkflowYaml:
    """Parsed YAML workflow definition.

    Parameters
    ----------
    name:
        Unique workflow identifier.
    goal:
        Human-readable description of what the workflow achieves.
    config:
        Top-level configuration (``dcc``, ``tools``, etc.).
    variables:
        Default variable values; overridden at runtime.
    tasks:
        Ordered list of :class:`WorkflowTask` objects.
    source_path:
        Absolute path to the YAML file (set by :func:`load_workflow_yaml`).

    """

    name: str
    goal: str = ""
    config: dict[str, Any] = field(default_factory=dict)
    variables: dict[str, Any] = field(default_factory=dict)
    tasks: list[WorkflowTask] = field(default_factory=list)
    source_path: str | None = None

    def validate(self) -> list[str]:
        """Return a list of validation error strings (empty = valid)."""
        errors: list[str] = []
        if not self.name:
            errors.append("WorkflowYaml.name is required")
        seen_names: set[str] = set()
        for i, t in enumerate(self.tasks):
            if not t.name:
                errors.append(f"tasks[{i}].name is required")
            elif t.name in seen_names:
                errors.append(f"Duplicate task name '{t.name}'")
            else:
                seen_names.add(t.name)
            if not t.tool:
                errors.append(f"tasks[{i}] '{t.name}': tool is required")
        return errors

    def task_names(self) -> list[str]:
        """Return ordered list of task names."""
        return [t.name for t in self.tasks]

    def get_task(self, name: str) -> WorkflowTask | None:
        """Find a task by name."""
        for t in self.tasks:
            if t.name == name:
                return t
        return None

    def to_summary_dict(self) -> dict[str, Any]:
        """Return a concise summary for agent consumption."""
        return {
            "name": self.name,
            "goal": self.goal,
            "dcc": self.config.get("dcc"),
            "task_count": len(self.tasks),
            "tasks": [
                {
                    "name": t.name,
                    "kind": t.kind,
                    "tool": t.tool,
                    "description": t.description,
                }
                for t in self.tasks
            ],
        }


# ── YAML loading ───────────────────────────────────────────────────────────


def _parse_tasks(raw_tasks: list[Any]) -> list[WorkflowTask]:
    tasks: list[WorkflowTask] = []
    for raw in raw_tasks:
        if not isinstance(raw, dict):
            continue
        try:
            tasks.append(
                WorkflowTask(
                    name=str(raw.get("name", "")),
                    kind=str(raw.get("kind", "step")),
                    tool=str(raw.get("tool", "")),
                    inputs=dict(raw.get("inputs") or {}),
                    outputs=list(raw.get("outputs") or []),
                    on_failure=list(raw.get("on_failure") or raw.get("on-failure") or []),
                    description=str(raw.get("description") or ""),
                )
            )
        except ValueError as exc:
            logger.warning("_parse_tasks: skipping invalid task: %s", exc)
    return tasks


def load_workflow_yaml(path: str | Path) -> WorkflowYaml:
    """Load and parse a workflow YAML file.

    Parameters
    ----------
    path:
        Absolute path to the workflow YAML file.

    Returns
    -------
    WorkflowYaml
        Parsed workflow definition.

    Raises
    ------
    FileNotFoundError
        If the file does not exist.
    ValueError
        If the file fails to parse or validate.

    """
    try:
        import yaml  # type: ignore[import-untyped]
    except ImportError as exc:
        raise ImportError("PyYAML is required to load workflow YAML files: pip install pyyaml") from exc

    p = Path(path)
    if not p.is_file():
        raise FileNotFoundError(f"Workflow YAML file not found: {p}")

    try:
        raw = yaml.safe_load(p.read_text(encoding="utf-8")) or {}
    except Exception as exc:
        raise ValueError(f"Failed to parse YAML at {p}: {exc}") from exc

    if not isinstance(raw, dict):
        raise ValueError(f"Workflow YAML must be a mapping, got {type(raw).__name__}")

    wf = WorkflowYaml(
        name=str(raw.get("name") or ""),
        goal=str(raw.get("goal") or ""),
        config=dict(raw.get("config") or {}),
        variables=dict(raw.get("variables") or {}),
        tasks=_parse_tasks(list(raw.get("tasks") or [])),
        source_path=str(p.resolve()),
    )

    errors = wf.validate()
    if errors:
        raise ValueError(f"Workflow YAML validation errors: {'; '.join(errors)}")
    return wf


# ── Skill metadata integration ────────────────────────────────────────────


def get_workflow_path(metadata: Any, glob_match_first: bool = True) -> str | None:
    """Extract the workflow file path from a ``SkillMetadata`` object.

    Reads ``metadata.dcc-mcp.workflows`` (flat or nested form).  If the value
    is a glob pattern, returns the first matching file (when ``glob_match_first``
    is True) or ``None``.

    Parameters
    ----------
    metadata:
        A ``SkillMetadata`` instance or any object with a ``metadata`` dict
        and optionally a ``skill_path`` str attribute.
    glob_match_first:
        When True and the value looks like a glob (contains ``*`` or ``?``),
        return the first match; otherwise return the pattern as-is.

    Returns
    -------
    str | None
        Absolute or relative path to the first workflow YAML file, or ``None``.

    """
    meta_dict: dict[str, Any] = getattr(metadata, "metadata", {}) or {}

    wf_rel = meta_dict.get("dcc-mcp.workflows")
    if wf_rel is None:
        nested = meta_dict.get("dcc-mcp")
        if isinstance(nested, dict):
            wf_rel = nested.get("workflows")

    if not wf_rel:
        return None

    skill_path = getattr(metadata, "skill_path", None)

    if "*" in str(wf_rel) or "?" in str(wf_rel):
        if skill_path and glob_match_first:
            base = Path(skill_path)
            matches = sorted(base.glob(str(wf_rel)))
            return str(matches[0]) if matches else None
        return str(wf_rel)

    if skill_path and not Path(str(wf_rel)).is_absolute():
        return str(Path(skill_path) / str(wf_rel))
    return str(wf_rel)


# ── MCP tool registration ──────────────────────────────────────────────────

_WORKFLOWS_LIST_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {},
    "additionalProperties": False,
}

_WORKFLOWS_DESCRIBE_SCHEMA: dict[str, Any] = {
    "type": "object",
    "properties": {
        "name": {
            "type": "string",
            "description": "Workflow name (from workflows.list).",
        },
    },
    "required": ["name"],
    "additionalProperties": False,
}

_WORKFLOWS_LIST_DESCRIPTION = (
    "List YAML workflow definitions loaded from skill sibling files. "
    "When to use: to discover available multi-step DCC workflows before "
    "executing them manually. "
    "How to use: no parameters; use workflows.describe for details on a specific workflow."
)

_WORKFLOWS_DESCRIBE_DESCRIPTION = (
    "Describe a YAML workflow: goal, task list, kind (task vs step), and tools. "
    "When to use: before executing a workflow step-by-step — understand each tool call needed. "
    "How to use: pass the workflow name from workflows.list."
)


def register_workflow_yaml_tools(
    server: Any,
    *,
    workflows: list[WorkflowYaml] | None = None,
    skills: list[Any] | None = None,
    dcc_name: str = "dcc",
) -> None:
    """Register ``workflows.list`` and ``workflows.describe`` on *server*.

    Pass either pre-loaded *workflows* or a list of ``SkillMetadata`` *skills*
    (the function will auto-discover workflow files via :func:`get_workflow_path`).

    Parameters
    ----------
    server:
        An ``McpHttpServer`` compatible object.
    workflows:
        Pre-loaded :class:`WorkflowYaml` objects.
    skills:
        ``SkillMetadata`` objects; workflow paths discovered automatically.
    dcc_name:
        DCC name for tool metadata.

    """
    workflow_map: dict[str, WorkflowYaml] = {}

    if workflows:
        for wf in workflows:
            workflow_map[wf.name] = wf

    if skills:
        for skill_md in skills:
            wf_path = get_workflow_path(skill_md)
            if not wf_path:
                continue
            try:
                wf = load_workflow_yaml(wf_path)
                workflow_map[wf.name] = wf
            except Exception as exc:
                skill_name = getattr(skill_md, "name", "?")
                logger.warning(
                    "register_workflow_yaml_tools: failed to load %s (skill %s): %s",
                    wf_path,
                    skill_name,
                    exc,
                )

    def _handle_list(_params: Any) -> Any:
        summaries = [wf.to_summary_dict() for wf in workflow_map.values()]
        return {
            "success": True,
            "message": f"{len(summaries)} workflow(s) available.",
            "context": {"workflows": summaries, "count": len(summaries)},
        }

    def _handle_describe(params: Any) -> Any:
        args: dict[str, Any] = json.loads(params) if isinstance(params, str) else (params or {})
        name = args.get("name", "")
        wf = workflow_map.get(name)
        if wf is None:
            available = list(workflow_map.keys())
            return {
                "success": False,
                "message": f"Workflow '{name}' not found.",
                "context": {"available": available},
            }
        return {
            "success": True,
            "message": f"Workflow '{name}': {wf.goal}",
            "context": wf.to_summary_dict(),
        }

    try:
        registry = server.registry
    except Exception as exc:
        logger.warning("register_workflow_yaml_tools: server.registry unavailable: %s", exc)
        return

    for name, desc, schema, handler in [
        ("workflows.list", _WORKFLOWS_LIST_DESCRIPTION, _WORKFLOWS_LIST_SCHEMA, _handle_list),
        ("workflows.describe", _WORKFLOWS_DESCRIBE_DESCRIPTION, _WORKFLOWS_DESCRIBE_SCHEMA, _handle_describe),
    ]:
        try:
            registry.register(
                name=name,
                description=desc,
                input_schema=json.dumps(schema),
                dcc=dcc_name,
                category="workflows",
                version="1.0.0",
            )
        except Exception as exc:
            logger.warning("register_workflow_yaml_tools: register(%s) failed: %s", name, exc)
            continue
        try:
            server.register_handler(name, handler)
        except Exception as exc:
            logger.warning("register_workflow_yaml_tools: register_handler(%s) failed: %s", name, exc)


# ── Public API ─────────────────────────────────────────────────────────────

__all__ = [
    "WorkflowTask",
    "WorkflowYaml",
    "get_workflow_path",
    "load_workflow_yaml",
    "register_workflow_yaml_tools",
]
