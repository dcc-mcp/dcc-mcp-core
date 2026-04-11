# Skills API

`dcc_mcp_core.SkillCatalog`、`dcc_mcp_core.SkillScanner`、`dcc_mcp_core.SkillWatcher`、`dcc_mcp_core.SkillMetadata`、`dcc_mcp_core.SkillSummary`、`dcc_mcp_core.ToolDeclaration`、`dcc_mcp_core.parse_skill_md`、`dcc_mcp_core.scan_and_load`

`dcc_mcp_core.skill`（纯 Python）：`skill_entry`、`skill_success`、`skill_error`、`skill_warning`、`skill_exception`、`run_main`

## SkillCatalog

渐进式 Skill 发现与加载。线程安全（所有状态存储在 DashMap/DashSet 中）。

当通过 `with_dispatcher()` 附加了调度器时，加载 Skill 会自动为每个 Action 注册基于子进程的处理器 — 启用 Skills-First 工作流，Agent 无需手动注册处理器。

```python
from dcc_mcp_core import SkillScanner, SkillCatalog

scanner = SkillScanner()
catalog = SkillCatalog(scanner)
```

### 构造函数

```python
SkillCatalog(scanner: SkillScanner) -> SkillCatalog
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `scanner` | `SkillScanner` | 用于发现的扫描器实例 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `with_dispatcher(dispatcher)` | — | 附加 `ActionDispatcher`；启用 `load_skill()` 时的自动处理器注册 |
| `discover(extra_paths=None, dcc_name=None)` | `None` | 扫描并填充目录 |
| `load_skill(skill_name)` | `bool` | 加载 Skill；成功返回 `True`，已加载或未找到返回 `False` |
| `unload_skill(skill_name)` | `bool` | 卸载 Skill；成功返回 `True`，未加载返回 `False` |
| `find_skills(query=None, tags=None, dcc=None)` | `List[SkillSummary]` | 按 name/tags/dcc 搜索（所有过滤器 AND 组合）|
| `list_skills(status=None)` | `List[SkillSummary]` | 列出 Skill。status：`"loaded"` 或 `"unloaded"`，`None` 为全部 |
| `get_skill_info(skill_name)` | `SkillMetadata \| None` | 返回完整元数据，未找到返回 `None` |
| `is_loaded(skill_name)` | `bool` | 指定 Skill 是否已加载 |
| `loaded_count()` | `int` | 已加载 Skill 的数量 |
| `__repr__()` | `str` | 字符串表示 |

### 示例

```python
import os
from dcc_mcp_core import SkillScanner, SkillCatalog, ActionRegistry, ActionDispatcher

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

scanner = SkillScanner()
catalog = SkillCatalog(scanner)

# 发现 Skill
catalog.discover(extra_paths=["/extra/skills"], dcc_name="maya")

# 列出所有已发现的 Skill
for skill in catalog.list_skills():
    status = "loaded" if skill.loaded else "unloaded"
    print(f"  [{status}] {skill.name} v{skill.version}: {skill.description}")

# 搜索
results = catalog.find_skills(query="geometry", tags=["create"])
for s in results:
    print(f"  {s.name}: {s.tool_count} tools → {s.tool_names}")

# 附加调度器 — 启用 Skills-First 自动处理器注册
registry = ActionRegistry()
dispatcher = ActionDispatcher(registry)
catalog.with_dispatcher(dispatcher)

# 加载 Skill（附加调度器后 Action 自动注册）
ok = catalog.load_skill("maya-geometry")
print(f"已加载: {ok}")

# 获取完整元数据
meta = catalog.get_skill_info("maya-geometry")
if meta:
    print(meta.name, meta.tools)

# 查看已加载数量
print(catalog.loaded_count())

# 卸载
ok = catalog.unload_skill("maya-geometry")
print(f"已卸载: {ok}")
```

---

## SkillSummary

`SkillCatalog.find_skills()` 和 `list_skills()` 返回的轻量级摘要对象。

### 属性（只读）

| 属性 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | Skill 名称 |
| `description` | `str` | 简短描述 |
| `tags` | `List[str]` | Skill 标签 |
| `dcc` | `str` | 目标 DCC（如 `"maya"`）|
| `version` | `str` | Skill 版本 |
| `tool_count` | `int` | 声明的工具数量 |
| `tool_names` | `List[str]` | 声明的工具名称列表 |
| `loaded` | `bool` | 当前是否已加载 |

### 特殊方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `SkillSummary(name='...', loaded=True)` |

---

## ToolDeclaration

Skill 中的单个工具声明，从 SKILL.md frontmatter 的 `tools:` 列表解析。

```python
from dcc_mcp_core import ToolDeclaration

decl = ToolDeclaration(
    name="create_sphere",
    description="创建多边形球体",
    input_schema='{"type":"object","properties":{"radius":{"type":"number"}}}',
    read_only=False,
    destructive=False,
    idempotent=False,
    source_file="scripts/create_sphere.py",
)
```

### 构造函数

```python
ToolDeclaration(
    name: str,
    description: str = "",
    input_schema: str | None = None,    # JSON Schema 字符串
    output_schema: str | None = None,   # JSON Schema 字符串
    read_only: bool = False,
    destructive: bool = False,
    idempotent: bool = False,
    source_file: str = "",
) -> ToolDeclaration
```

### 字段（可读写）

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | 必填 | 工具名称（在 Skill 内唯一）|
| `description` | `str` | `""` | 人类可读的描述 |
| `read_only` | `bool` | `False` | 仅读取数据（无副作用）|
| `destructive` | `bool` | `False` | 可能导致破坏性更改 |
| `idempotent` | `bool` | `False` | 相同参数始终产生相同结果 |
| `source_file` | `str` | `""` | 脚本文件的显式路径 |

::: tip input_schema 和 output_schema
内部以 JSON 值存储，非字符串。从 Python 构造时传入 JSON 字符串，会自动解析。
:::

---

## SkillMetadata

从 Skill 的 `SKILL.md` frontmatter 解析。同时支持 Anthropic Skills、ClawHub/OpenClaw 和 dcc-mcp-core 扩展格式。

### 构造函数

```python
SkillMetadata(
    name: str,
    description: str = "",
    tools: List[str] | None = None,
    dcc: str = "python",
    tags: List[str] | None = None,
    scripts: List[str] | None = None,
    skill_path: str = "",
    version: str = "1.0.0",
    depends: List[str] | None = None,
    metadata_files: List[str] | None = None,
) -> SkillMetadata
```

### 字段（可读写）

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 唯一 Skill 名称 |
| `description` | `str` | 简短描述 |
| `tools` | `List[str]` | frontmatter 中的工具名称 |
| `dcc` | `str` | 目标 DCC 应用 |
| `tags` | `List[str]` | 分类标签 |
| `scripts` | `List[str]` | 发现的脚本文件路径 |
| `skill_path` | `str` | Skill 目录绝对路径 |
| `version` | `str` | Skill 版本 |
| `depends` | `List[str]` | 依赖的 Skill 名称 |
| `metadata_files` | `List[str]` | `metadata/` 目录中的 `.md` 文件路径 |

---

## SkillScanner

扫描目录以发现 Skill 技能包，缓存文件修改时间以支持高效的重复扫描。

```python
from dcc_mcp_core import SkillScanner

scanner = SkillScanner()
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `scan(extra_paths=None, dcc_name=None, force_refresh=False)` | `List[str]` | 扫描路径以查找 Skill 目录 |
| `clear_cache()` | — | 清除修改时间缓存和已发现列表 |

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `discovered_skills` | `List[str]` | 之前发现的 Skill 目录路径 |

---

## SkillWatcher

Skill 目录的热重载监控器。监控文件系统事件，当 `SKILL.md` 文件更改时自动重新加载技能元数据。

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/path/to/skills")
skills = watcher.skills()
```

### 构造函数

```python
SkillWatcher(debounce_ms: int = 300) -> SkillWatcher
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `watch(path)` | — | 开始递归监控 `path`。路径不存在则抛出 `RuntimeError` |
| `unwatch(path)` | `bool` | 停止监控 `path`。曾被监控返回 `True` |
| `skills()` | `List[SkillMetadata]` | 当前所有已加载技能的快照 |
| `skill_count()` | `int` | 当前已加载技能数量 |
| `watched_paths()` | `List[str]` | 当前正在监控的目录路径列表 |
| `reload()` | — | 手动触发完整重载 |

---

## 函数

### parse_skill_md

```python
parse_skill_md(skill_dir: str) -> SkillMetadata | None
```

从 Skill 目录解析 `SKILL.md`。文件缺失或无效时返回 `None`。

### scan_skill_paths

```python
scan_skill_paths(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> List[str]
```

便捷包装器：创建 `SkillScanner` 并返回发现的 Skill 目录路径。

### scan_and_load

```python
scan_and_load(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[List[SkillMetadata], List[str]]
```

完整流水线：扫描目录、加载所有 Skill、按依赖拓扑排序。

返回 `(ordered_skills, skipped_dirs)`。依赖缺失或有循环则抛出 `ValueError`。

### scan_and_load_lenient

```python
scan_and_load_lenient(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[List[SkillMetadata], List[str]]
```

与 `scan_and_load` 相同，但静默跳过依赖缺失的 Skill（通过日志记录警告）。仅循环依赖会抛出 `ValueError`。

### resolve_dependencies

```python
resolve_dependencies(skills: List[SkillMetadata]) -> List[SkillMetadata]
```

拓扑排序，每个 Skill 出现在其依赖之后。依赖缺失或有循环则抛出 `ValueError`。

### validate_dependencies

```python
validate_dependencies(skills: List[SkillMetadata]) -> List[str]
```

验证所有声明的依赖是否存在。返回错误消息列表（空列表表示无问题）。

### expand_transitive_dependencies

```python
expand_transitive_dependencies(
    skills: List[SkillMetadata],
    skill_name: str,
) -> List[str]
```

返回 `skill_name` 所有传递依赖的名称。依赖缺失或有循环则抛出 `ValueError`。

---

## 搜索路径优先级

1. `extra_paths` 参数（最高优先级）
2. `DCC_MCP_{APP}_SKILL_PATHS` 环境变量（应用专属，如 `DCC_MCP_MAYA_SKILL_PATHS`）
3. `DCC_MCP_SKILL_PATHS` 环境变量（全局兜底）
4. 平台特定 Skill 目录（DCC 特定，通过 `get_skills_dir(dcc_name)`）
5. 平台特定 Skill 目录（全局，通过 `get_skills_dir()`）

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_{APP}_SKILL_PATHS` | 应用专属 Skill 路径，如 `DCC_MCP_MAYA_SKILL_PATHS`（Windows 用 `;`，Unix 用 `:`）|
| `DCC_MCP_SKILL_PATHS` | 全局兜底 Skill 路径 |

### create_skill_manager

```python
create_skill_manager(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer
```

**Skills-First 工作流的推荐入口**（v0.12.12+）。

一次调用即可为指定 DCC 应用创建完整配置的 `McpHttpServer`，自动完成：
1. 创建 `ActionRegistry` + `ActionDispatcher`
2. 创建与 dispatcher 连接的 `SkillCatalog`
3. 从 `DCC_MCP_{APP}_SKILL_PATHS` 和 `DCC_MCP_SKILL_PATHS` 发现 Skills
4. 返回已配置好的 `McpHttpServer`

**参数：**

| 参数 | 类型 | 说明 |
|------|------|------|
| `app_name` | `str` | DCC 名称（如 `"maya"`、`"blender"`）— 用于推导环境变量名和 MCP 服务器名 |
| `config` | `McpHttpConfig \| None` | HTTP 服务器配置；默认端口 8765 |
| `extra_paths` | `list[str] \| None` | 除环境变量外的额外 Skill 目录 |
| `dcc_name` | `str \| None` | 覆盖扫描的 DCC 过滤条件（默认与 `app_name` 相同）|

**返回值：** `McpHttpServer` — 调用 `.start()` 开始服务。

**示例：**

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"服务地址：{handle.mcp_url()}")
```

### get_app_skill_paths_from_env

```python
get_app_skill_paths_from_env(app_name: str) -> list[str]
```

从 `DCC_MCP_{APP_NAME}_SKILL_PATHS` 环境变量中返回 Skill 路径列表。

查找时不区分大小写，实际环境变量键名自动转换为大写（如 `app_name="maya"` 对应 `DCC_MCP_MAYA_SKILL_PATHS`）。

若环境变量未设置，返回 `[]`。

## Action 命名规则

`SkillCatalog.load_skill()` 注册工具时，Action 名称遵循以下格式：

```
{skill名称（连字符转下划线）}__{工具名称}
```

示例：
- Skill `maya-geometry`，工具 `create_sphere` → `maya_geometry__create_sphere`
- Skill `blender-utils`，工具 `render-scene` → `blender_utils__render_scene`

---

## Skill Script 辅助工具（纯 Python）

`dcc_mcp_core.skill` 是**纯 Python** 子模块 — 无需编译扩展即可使用。
Skill 脚本作者可在 DCC 环境内直接导入，即便完整 wheel 包未安装也可正常运行。

```python
from dcc_mcp_core.skill import skill_entry, skill_success, skill_error
```

所有辅助函数返回普通 `dict`，与 `ActionResultModel` 完全兼容。
若 `dcc_mcp_core._core` 可用，可将 dict 传入 `validate_action_result()` 获得类型化的 `ActionResultModel` 对象。

---

### skill_success

```python
skill_success(
    message: str,
    *,
    prompt: str | None = None,
    **context,
) -> dict
```

返回成功结果 dict。

| 参数 | 类型 | 说明 |
|------|------|------|
| `message` | `str` | 人类可读的执行摘要 |
| `prompt` | `str \| None` | Agent 下一步操作的提示（可选）|
| `**context` | `Any` | 附加到 `context` 的任意键值对 |

```python
return skill_success(
    "时间线已设置为 1–120 帧",
    prompt="查看时间线滑块确认结果。",
    start_frame=1,
    end_frame=120,
)
```

---

### skill_error

```python
skill_error(
    message: str,
    error: str,
    *,
    prompt: str | None = None,
    possible_solutions: list[str] | None = None,
    **context,
) -> dict
```

返回失败结果 dict。

| 参数 | 类型 | 说明 |
|------|------|------|
| `message` | `str` | 面向用户的错误描述 |
| `error` | `str` | 技术错误字符串（异常 repr、错误码等）|
| `prompt` | `str \| None` | 恢复提示；默认为通用消息 |
| `possible_solutions` | `list[str] \| None` | 可操作建议，存储在 `context["possible_solutions"]` 中 |

```python
return skill_error(
    "Maya 环境不可用",
    "ImportError: No module named 'maya'",
    prompt="请确保 Maya 已启动再调用此 Skill。",
    possible_solutions=["启动 Maya", "检查 DCC_MCP_MAYA_SKILL_PATHS"],
)
```

---

### skill_warning

```python
skill_warning(
    message: str,
    *,
    warning: str = "",
    prompt: str | None = None,
    **context,
) -> dict
```

返回成功但带警告的结果（`success=True`，`context["warning"]` 被设置）。

```python
return skill_warning(
    "时间线已设置，end_frame 已截断为场景长度",
    warning="end_frame 9999 > 场景长度 240；已截断为 240",
    prompt="查看时间线滑块。",
    actual_end=240,
)
```

---

### skill_exception

```python
skill_exception(
    exc: BaseException,
    *,
    message: str | None = None,
    prompt: str | None = None,
    include_traceback: bool = True,
    possible_solutions: list[str] | None = None,
    **context,
) -> dict
```

从异常构建失败结果 dict。自动捕获 `error_type` 和完整堆栈跟踪（可选）存入 `context`。

```python
try:
    do_work()
except Exception as exc:
    return skill_exception(
        exc,
        possible_solutions=["请确认场景已打开"],
    )
```

---

### @skill_entry

```python
@skill_entry
def my_tool(param: str = "default", **kwargs) -> dict:
    ...
```

为 skill 函数添加标准错误处理的装饰器。

- 自动捕获 `ImportError`（DCC 模块缺失）、`Exception` 和 `BaseException`
- 每种异常自动转换为规范的错误 dict
- 直接运行脚本时（`__name__ == "__main__"`），将 JSON 结果打印到 stdout

**完整示例**（替代手动 try/except/`main()` 样板代码）：

```python
from dcc_mcp_core.skill import skill_entry, skill_success

@skill_entry
def set_timeline(start_frame: float = 1.0, end_frame: float = 120.0, **kwargs):
    """设置 Maya 播放时间线范围。"""
    import maya.cmds as cmds  # ImportError 由装饰器自动捕获

    min_frame = kwargs.get("min_frame", start_frame)
    max_frame = kwargs.get("max_frame", end_frame)

    cmds.playbackOptions(
        min=min_frame, max=max_frame,
        animationStartTime=start_frame, animationEndTime=end_frame,
    )
    return skill_success(
        f"时间线已设置为 {start_frame}–{end_frame}",
        prompt="查看时间线滑块确认结果。",
        start_frame=start_frame,
        end_frame=end_frame,
    )

def main(**kwargs):
    """入口点；委托给 set_timeline。"""
    return set_timeline(**kwargs)

if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
```

---

### run_main

```python
run_main(main_fn: Callable[..., dict], argv: list[str] | None = None) -> None
```

执行 `main_fn` 并将 JSON 结果打印到 stdout。成功时调用 `sys.exit(0)`，失败时调用 `sys.exit(1)`。

用于 `if __name__ == "__main__"` 块：

```python
if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
```

---

### 从 DCC 专用辅助函数迁移

如果之前使用 `dcc_mcp_maya` 的 `maya_success` / `maya_error` / `maya_from_exception`，通用版本直接对应：

| 旧（DCC 专用）| 新（通用）|
|--------------|----------|
| `maya_success(msg, prompt=..., **ctx)` | `skill_success(msg, prompt=..., **ctx)` |
| `maya_error(msg, error, prompt=..., **ctx)` | `skill_error(msg, error, prompt=..., **ctx)` |
| `maya_from_exception(exc_msg, ...)` | `skill_exception(exc, ...)` |

dict 结构完全相同 — 两者均与 `ActionResultModel` 兼容。
