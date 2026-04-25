# Workflow YAML API

YAML declarative workflow definitions with task/step semantics (issue #439).

The key innovation is the **task vs step** semantic distinction: **task** opens a new conversation/clean context; **step** operates within the same conversation with accumulated history.

**Exported symbols:** `WorkflowTask`, `WorkflowYaml`, `get_workflow_path`, `load_workflow_yaml`, `register_workflow_yaml_tools`

## WorkflowTask

A single task or step in a workflow definition.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | (required) | Unique identifier within the workflow |
| `kind` | `str` | `"step"` | `"task"` (clean context) or `"step"` (accumulated context) |
| `tool` | `str` | `""` | MCP tool name to invoke |
| `inputs` | `dict[str, Any]` | `{}` | Variable-interpolated input dict |
| `outputs` | `list[str]` | `[]` | Output variable names produced |
| `on_failure` | `list[str]` | `[]` | Follow-up tools on failure |
| `description` | `str` | `""` | Human-readable summary |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `interpolate_inputs(variables)` | `dict` | Return inputs with `{{var}}` templates replaced |

## WorkflowYaml

Parsed YAML workflow definition.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | (required) | Unique workflow identifier |
| `goal` | `str` | `""` | Human-readable description |
| `config` | `dict` | `{}` | Top-level configuration |
| `variables` | `dict` | `{}` | Default variable values |
| `tasks` | `list[WorkflowTask]` | `[]` | Ordered task list |
| `source_path` | `str \| None` | `None` | Absolute path to the YAML file |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `validate()` | `list[str]` | Return validation errors (empty = valid) |
| `task_names()` | `list[str]` | Return ordered list of task names |
| `get_task(name)` | `WorkflowTask \| None` | Find a task by name |
| `to_summary_dict()` | `dict` | Return concise summary for agent consumption |

## load_workflow_yaml

```python
load_workflow_yaml(path: str | Path) -> WorkflowYaml
```

Load and parse a workflow YAML file. Raises `FileNotFoundError` if not found, `ValueError` if parse/validation fails.

## get_workflow_path

```python
get_workflow_path(metadata: Any, glob_match_first: bool = True) -> str | None
```

Extract the workflow file path from a `SkillMetadata` object. If value is a glob pattern, returns the first matching file.

## register_workflow_yaml_tools

```python
register_workflow_yaml_tools(server, *, workflows=None, skills=None, dcc_name="dcc") -> None
```

Register `workflows.list` and `workflows.describe` MCP tools on `server`. Pass either pre-loaded `workflows` or a list of `SkillMetadata` `skills`.
