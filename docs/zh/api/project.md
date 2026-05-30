# 项目状态 API

DCC 会话的项目级状态持久化（issue #576）。这是作业范围检查点的持久伴侣：它在场景或文档旁边的 `.dcc-mcp/project.json` 中存储场景/会话上下文。

**导出的符号：** `DccProject`、`ProjectState`、`PROJECT_DIR_NAME`、`PROJECT_STATE_FILE`

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

`metadata` 是 DCC 特定的扩展空间，用于单位、上轴、渲染层、时间线范围、活动文档 ID 或适配器会话详细信息。

## DccProject

```python
from dcc_mcp_core import DccProject

project = DccProject.open("/project/shot_010/main.ma")
project.add_asset("/assets/char_v001.ma")
project.activate_skill("maya-lookdev")
project.add_checkpoint_id("job_abc123")
project.update_metadata(units="cm", up_axis="y")

payload = project_resume_session()
```

`DccProject.open(scene_path)` 在缺少时在场景目录中创建 `.dcc-mcp/project.json`。变异辅助工具会自动保存，`DccProject.load(scene_path_or_project_dir)` 为后续会话恢复状态。
