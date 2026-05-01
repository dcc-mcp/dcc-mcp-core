# 项目级状态持久化

项目级持久化（issue [#576](https://github.com/loonghao/dcc-mcp-core/issues/576)）
给 DCC 会话提供一份**持久的、基于文件的视图**：当前打开的是哪个场景、加载了
哪些资产、哪些技能和工具组处于激活状态、哪些任务留有检查点。核心保持 schema
与 DCC 无关；适配器通过 `ProjectState.metadata` 附加宿主相关的提示。

## 何时用项目持久化，何时用任务级检查点

`dcc-mcp-core` 提供**互补**的两层持久化，二者配合使用，而不是互相替代：

| | **任务检查点** (`checkpoint.py`) | **项目状态** (`project.py`) |
|-|-|-|
| **持久化单位** | 单个长任务 | 整个 DCC 会话 |
| **生命周期** | 任务完成即清理 | 跨进程重启长期保留 |
| **典型写入者** | 技能脚本每 N 项写一次 | 适配器在场景加载 / 技能激活时写 |
| **典型读取者** | 同一技能脚本的 resume 逻辑 | 后续会话中任何询问"当前加载了什么"的代理 |
| **磁盘位置** | `<project_dir>/.dcc-mcp/checkpoints.json`（经 `DccProject.checkpoints`）或用户自定义路径 | `<project_dir>/.dcc-mcp/project.json` |
| **恢复语义** | 单任务内跳过已处理项 | 重开场景、重激活技能、恢复 metadata |

经验法则：如果状态只在单次工具调用期间有意义，用检查点；如果后续会话的另一个
代理会想看到它，就放进项目状态。

## 目录结构

`DccProject.open(scene_path)` 会在场景旁创建边车目录：

```
/my-dcc-project/
├── scene.ma
├── assets/
│   └── char_v001.ma
└── .dcc-mcp/
    ├── project.json        # ProjectState
    └── checkpoints.json    # CheckpointStore（通过 DccProject.checkpoints）
```

Blender `.blend`、Houdini `.hip`、USD 舞台、PSD 文件都适用同一布局：适配器挑
一个"场景一类"的文件，其余由核心负责。

## Python 用法

```python
from dcc_mcp_core.project import DccProject

# 打开（或创建）场景旁的项目
project = DccProject.open("/show/shots/010/shot.ma")

# 变更 —— 每次调用都自动写回 project.json
project.add_asset("/show/assets/char_v001.ma")
project.activate_skill("maya-lookdev")
project.activate_tool_group("maya-lookdev-tools")  # #576 新增的分组
project.update_metadata(units="cm", up_axis="y")

# 任务检查点落在同一个项目目录下
project.checkpoints.save("job-abc", state={"processed": 42}, progress_hint="42/100")

# 新进程里只加载、不创建
restored = DccProject.load("/show/shots/010/shot.ma")
print(restored.state.active_skills)  # → ['maya-lookdev']
```

对于从未保存过的场景，`DccProject.load` 返回一个空 `ProjectState` —— **不会**
往磁盘写任何内容。想显式初始化项目请用 `DccProject.open`。

## 注册 MCP 工具

适配器在服务启动时调用 `register_project_tools` 把项目状态暴露给代理。它在
`project` 分类下注册 4 个工具：

| 工具 | 输入 | 输出 |
|-|-|-|
| `project.save`   | `scene_path`              | 保存后的完整状态 dict |
| `project.load`   | `scene_path` 或 `project_dir` | 状态 dict；不存在 `project.json` 时 `success: false` |
| `project.resume` | `scene_path` 或 `project_dir` | `resume_session()` 载荷 |
| `project.status` | `scene_path` 或 `project_dir` | 指定项目的状态 dict |

```python
from dcc_mcp_core import register_project_tools

# 无默认项目，调用方必须传 scene_path / project_dir
register_project_tools(server, dcc_name="maya")

# 或者绑定默认项目，代理就能无参数调用 project.status
from dcc_mcp_core.project import DccProject
project = DccProject.open(current_scene_path())
register_project_tools(server, dcc_name="maya", project=project)
```

处理函数同时接受 JSON 字符串和 dict 作为参数，与
`register_checkpoint_tools` 的契约一致。`registry.register` 或
`server.register_handler` 的失败都会被 log、不会导致崩溃 —— 错配置的服务器绝
不会因为某个工具缺失就挂掉。

## 与 DCC 适配器集成

适配器的典型流程：

1. 从宿主读当前场景路径（Maya 里 `cmds.file(q=True, sn=True)`，Blender 里
   `bpy.data.filepath` 等）。
2. 每次会话调用一次 `DccProject.open(scene_path)`。
3. 把代理猜不到的宿主信息写进 `ProjectState.metadata`：`units`、`up_axis`、
   帧范围、渲染相机等。
4. 调用 `register_project_tools(server, project=<绑定的项目>)`，让代理不必
   回调宿主就能查 `project.status`。
5. 在适配器启动时往 `active_tool_groups` 里追加当前挂载的 UI shelf / 工具
   面板 —— 这样 recipe 和 skill 就能直接判断自己的前置条件是否可用，不必额外
   探测。

```python
# MayaMcpServer 启动代码（简化）
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

## 状态序列化的最佳实践

- **`metadata` 保持可 JSON 序列化。** `DccProject.save` 用共享的
  `json_dumps` 辅助；非 JSON 值会在保存时报错，而不是在重新加载时神秘失败。
- **尽量用跨机器仍然有意义的路径。** 带盘符的绝对路径很难迁移；尽量存储
  相对场景路径（或可解析的 token，如 `$SHOT_ROOT/char.ma`），读的时候再解析。
- **不要存密钥。** 项目状态和场景一起落盘、一起流转；视其为生产流水线内
  世界可读即可。
- **通过 `DccProject` 辅助方法修改**（`add_asset`、`activate_skill`、
  `activate_tool_group`、`update_metadata`）。它们会处理自动保存和去重，还能
  让 `updated_at` 与实际文件内容保持一致。
- **保留 `project.json` 的向后兼容性。** `ProjectState.from_dict` 能容忍
  较旧的、缺少 `active_tool_groups` 或 `created_at` 等字段的载荷。下游适配器
  扩展字段时也应同样处理（`payload.get("field") or <默认值>`）。

## 相关链接

- `dcc_mcp_core.checkpoint` —— 任务级恢复状态。参见
  [任务持久化](./job-persistence.md)。
- `workflows.resume` —— 工作流级恢复（issue
  [#565](https://github.com/loonghao/dcc-mcp-core/issues/565)）。未来可能把
  `project.resume` 桥接到工作流引擎，让工作流在重跑步骤前先恢复 DCC 会话。
- 仓库根目录的 `AGENTS.md` —— 代理如何发现并调用这些工具。
