# Project-Level State Persistence

Project persistence (issue [#576](https://github.com/loonghao/dcc-mcp-core/issues/576))
gives a DCC session a durable, file-based view of *what the artist is working on
right now*: which scene is open, which assets are loaded, which skills and tool
groups are active, and which jobs have checkpoints.  Core keeps the schema
DCC-agnostic; adapters add host-specific hints via `ProjectState.metadata`.

## When to use project persistence vs. job-level checkpoints

`dcc-mcp-core` exposes **two complementary** persistence layers.  They serve
different purposes and should be used together, not instead of each other:

| | **Job checkpoint** (`checkpoint.py`) | **Project state** (`project.py`) |
|-|-|-|
| **Unit of persistence** | A single long-running job | The whole DCC session |
| **Lifetime** | Cleared on job completion | Survives indefinitely across restarts |
| **Typical writer** | A skill script every N items | The adapter on scene load / skill activation |
| **Typical reader** | The same skill script on resume | Any agent asking "what is loaded?" |
| **On-disk location** | `<project_dir>/.dcc-mcp/checkpoints.json` (via `DccProject.checkpoints`) *or* a user-chosen path | `<project_dir>/.dcc-mcp/project.json` |
| **Resume semantics** | Skip already-processed items within one job | Reopen the scene, re-activate skills, rehydrate metadata |

Rule of thumb: if the state is meaningful only while a single tool call is
running, use a checkpoint.  If another agent in a later session would want to
see it, put it on the project.

## Directory layout

`DccProject.open(scene_path)` creates a sidecar directory next to the scene:

```
/my-dcc-project/
├── scene.ma
├── assets/
│   └── char_v001.ma
└── .dcc-mcp/
    ├── project.json        # ProjectState
    └── checkpoints.json    # CheckpointStore (via DccProject.checkpoints)
```

The same layout applies to Blender `.blend`, Houdini `.hip`, USD stages, and
PSD files: adapters pick any scene-like file and the core does the rest.

## Python usage

```python
from dcc_mcp_core.project import DccProject

# Open (or create) the project alongside the scene
project = DccProject.open("/show/shots/010/shot.ma")

# Mutate — every call auto-saves to project.json
project.add_asset("/show/assets/char_v001.ma")
project.activate_skill("maya-lookdev")
project.activate_tool_group("maya-lookdev-tools")  # grouping for #576
project.update_metadata(units="cm", up_axis="y")

# Job checkpoints live under the same project dir
project.checkpoints.save("job-abc", state={"processed": 42}, progress_hint="42/100")

# On a new process, load without creating
restored = DccProject.load("/show/shots/010/shot.ma")
print(restored.state.active_skills)  # → ['maya-lookdev']
```

`DccProject.load` returns a `DccProject` with an empty `ProjectState` if the
scene has never been saved — it **does not** write anything to disk.  Use
`DccProject.open` when you explicitly want to initialise a project.

## Registering MCP tools

Adapters expose project state to agents by calling `register_project_tools`
during server bootstrap.  It registers four tools under category `project`:

| Tool | Input | Output |
|-|-|-|
| `project.save`   | `scene_path`              | Full state dict after save  |
| `project.load`   | `scene_path` or `project_dir` | State dict, or `success: false` if no `project.json` exists |
| `project.resume` | `scene_path` or `project_dir` | `resume_session()` payload  |
| `project.status` | `scene_path` or `project_dir` | State dict for the given project |

```python
from dcc_mcp_core import register_project_tools

# Bootstrap: no default project, callers must pass scene_path / project_dir
register_project_tools(server, dcc_name="maya")

# OR: bind a default so agents can call project.status with no args
from dcc_mcp_core.project import DccProject
project = DccProject.open(current_scene_path())
register_project_tools(server, dcc_name="maya", project=project)
```

Handlers accept both JSON-encoded string params and plain dicts, matching the
`register_checkpoint_tools` contract.  Failures in `registry.register` or
`server.register_handler` are logged and non-fatal — a misconfigured server
will never crash from a missing tool.

## Integrating with DCC-specific adapters

Adapters typically:

1. Resolve the current scene path from the host (`cmds.file(q=True, sn=True)`
   in Maya, `bpy.data.filepath` in Blender, etc.).
2. Call `DccProject.open(scene_path)` once per session.
3. Populate `ProjectState.metadata` with host-specific hints that agents cannot
   guess otherwise: `units`, `up_axis`, frame range, render camera, and so on.
4. Call `register_project_tools(server, project=<bound project>)` so agents can
   always query `project.status` without a round-trip to the host.
5. Extend `active_tool_groups` at adapter startup to reflect which UI shelves
   / tool-palettes are currently mounted — this lets recipes and skills decide
   whether their prerequisites are available without a separate probe.

```python
# Inside MayaMcpServer bootstrap (simplified)
from dcc_mcp_core import register_project_tools
from dcc_mcp_core.project import DccProject

scene_path = cmds.file(q=True, sn=True) or None
if scene_path:
    project = DccProject.open(scene_path)
    project.update_metadata(
        units=cmds.currentUnit(q=True, linear=True),
        up_axis=cmds.upAxis(q=True, axis=True),
        fps=mel.eval("currentTimeUnitToFPS"),
    )
    project.activate_tool_group("maya-rigging-shelf")
    register_project_tools(server, dcc_name="maya", project=project)
```

## Best practices for state serialization

- **Keep `metadata` JSON-serialisable.**  `DccProject.save` uses the shared
  `json_dumps` helper; non-JSON values raise at save time, not mysteriously
  later on reload.
- **Prefer paths that make sense outside the current machine.**  Absolute
  paths with drive letters travel badly; where possible, store scene-relative
  paths (or resolvable tokens like `$SHOT_ROOT/char.ma`) and resolve at read
  time.
- **Don't store secrets.**  Project state sits next to the scene on disk and
  travels with it; treat it as world-readable within the production pipeline.
- **Mutate through the `DccProject` helpers** (`add_asset`, `activate_skill`,
  `activate_tool_group`, `update_metadata`).  They handle the auto-save and
  deduplication so you don't have to — and they keep `updated_at` coherent
  with the actual file content.
- **Respect backward compatibility of `project.json`.**  `ProjectState.from_dict`
  tolerates older payloads that predate fields like `active_tool_groups` or
  `created_at`.  If you add further fields in downstream adapters, apply the
  same pattern (`payload.get("field") or <default>`).

## See also

- `dcc_mcp_core.checkpoint` — job-scoped resume state.
  See [Job Persistence](./job-persistence.md).
- `workflows.resume` — workflow-level resume (issue
  [#565](https://github.com/loonghao/dcc-mcp-core/issues/565)).  A future
  enhancement may bridge `project.resume` into the workflow engine so a
  workflow can restore the DCC session before re-running steps.
- `AGENTS.md` at the repo root — how to discover and call these tools from an
  agent.
