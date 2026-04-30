# Project State API

Project-level state persistence for DCC sessions (issue #576). This is a durable companion to job-scoped checkpoints: it stores scene/session context in `.dcc-mcp/project.json` next to a scene or document.

**Exported symbols:** `DccProject`, `ProjectState`, `PROJECT_DIR_NAME`, `PROJECT_STATE_FILE`

## ProjectState

```python
ProjectState(
    scene_path="",
    loaded_assets=[],
    active_skills=[],
    checkpoint_ids=[],
    metadata={},
)
```

`metadata` is DCC-specific extension space for units, up-axis, render layer, timeline range, active document ids, or adapter session details.

## DccProject

```python
from dcc_mcp_core import DccProject

project = DccProject.open("/project/shot_010/main.ma")
project.add_asset("/assets/char_v001.ma")
project.activate_skill("maya-lookdev")
project.add_checkpoint_id("job_abc123")
project.update_metadata(units="cm", up_axis="y")

payload = project.resume_session()
```

`DccProject.open(scene_path)` creates `.dcc-mcp/project.json` in the scene directory when missing. Mutating helpers auto-save, and `DccProject.load(scene_path_or_project_dir)` restores the state for a later session.
