# Skills 技能包系统

Skills 系统允许你将任何脚本（Python、MEL、MaxScript、BAT、Shell 等）零代码注册为 MCP 可发现的工具，直接复用 [OpenClaw Skills](https://docs.openclaw.ai/tools) / Anthropic Skills 生态格式。

## 快速上手

### 1. 创建 Skill 目录

```
maya-geometry/
├── SKILL.md
└── scripts/
    ├── create_sphere.py
    ├── batch_rename.mel
    └── export_fbx.bat
```

### 2. 编写 SKILL.md

```yaml
---
name: maya-geometry
description: "Maya 几何体创建和修改工具"
version: "1.0.0"
dcc: maya
tags: ["geometry", "create"]
tools:
  - name: create_sphere
    description: "根据给定半径创建多边形球体"
    source_file: scripts/create_sphere.py
    read_only: false
  - name: export_fbx
    description: "将选中对象导出为 FBX"
    source_file: scripts/export_fbx.bat
---
# Maya Geometry Skill

使用这些工具在 Maya 中创建和修改几何体。
```

### 3. 设置环境变量

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\to\my-skills

# 多个路径（使用平台路径分隔符）
export DCC_MCP_SKILL_PATHS="/path/skills1:/path/skills2"
```

### 4. 发现并加载

推荐使用 `SkillCatalog` 进行完整的渐进式加载，也可使用低级扫描函数进行一次性操作：

```python
from dcc_mcp_core import SkillScanner, SkillCatalog, ActionRegistry, ActionDispatcher

# 创建扫描器和目录
scanner = SkillScanner()
catalog = SkillCatalog(scanner)

# 发现 DCC_MCP_SKILL_PATHS 中的所有 Skill
catalog.discover(dcc_name="maya")

# 可选：附加调度器以启用自动处理器注册
registry = ActionRegistry()
dispatcher = ActionDispatcher(registry)
catalog.with_dispatcher(dispatcher)

# 列出可用 Skill
for skill in catalog.list_skills():
    print(f"  {skill.name} v{skill.version}: {skill.description} (已加载={skill.loaded})")

# 加载 Skill — 附加调度器后工具自动注册
ok = catalog.load_skill("maya-geometry")
print(f"已加载: {ok}")
```

## Skill 目录（推荐 API）

`SkillCatalog` 管理完整生命周期：发现 → 渐进式加载 → 卸载。

```python
from dcc_mcp_core import SkillScanner, SkillCatalog, ActionRegistry, ActionDispatcher

scanner = SkillScanner()
catalog = SkillCatalog(scanner)

registry = ActionRegistry()
dispatcher = ActionDispatcher(registry)
catalog.with_dispatcher(dispatcher)

# 发现
catalog.discover(extra_paths=["/my/skills"], dcc_name="maya")

# 搜索
results = catalog.find_skills(query="geometry", tags=["create"], dcc="maya")
for s in results:
    print(f"{s.name}: {s.tool_count} 个工具 {s.tool_names}")

# 加载/卸载
ok = catalog.load_skill("maya-geometry")  # 返回 bool
catalog.is_loaded("maya-geometry")        # True
ok = catalog.unload_skill("maya-geometry")

# 状态查询
catalog.loaded_count()      # int
len(catalog)                # 目录中的 Skill 总数
catalog.list_skills()                  # 所有 Skill（SkillSummary 列表）
catalog.list_skills("loaded")          # 仅已加载的
catalog.list_skills("unloaded")        # 仅未加载的

# 详细信息
info = catalog.get_skill_info("maya-geometry")  # dict 或 None
```

### SkillSummary 字段

`find_skills()` 和 `list_skills()` 返回 `SkillSummary` 对象：

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | Skill 名称 |
| `description` | `str` | 简短描述 |
| `tags` | `List[str]` | Skill 标签 |
| `dcc` | `str` | 目标 DCC（如 `"maya"`）|
| `version` | `str` | Skill 版本 |
| `tool_count` | `int` | 声明的工具数量 |
| `tool_names` | `List[str]` | 声明的工具名称列表 |
| `loaded` | `bool` | 当前是否已加载 |

## ToolDeclaration

`ToolDeclaration` 描述 Skill 中的单个工具，从 SKILL.md frontmatter 的 `tools:` 列表中解析：

```yaml
tools:
  - name: create_sphere
    description: "创建多边形球体"
    input_schema: '{"type":"object","properties":{"radius":{"type":"number"}}}'
    read_only: false
    destructive: false
    idempotent: false
    defer-loading: true
    source_file: scripts/create_sphere.py
```

```python
from dcc_mcp_core import ToolDeclaration

decl = ToolDeclaration(
    name="create_sphere",
    description="创建多边形球体",
    input_schema='{"type":"object","properties":{"radius":{"type":"number"}}}',
    read_only=False,
    destructive=False,
    idempotent=False,
    defer_loading=True,
    source_file="scripts/create_sphere.py",
)
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | 必填 | 工具名称（在 Skill 内唯一）|
| `description` | `str` | `""` | 人类可读的描述 |
| `input_schema` | `str`（JSON）| `None` | 输入参数的 JSON Schema |
| `output_schema` | `str`（JSON）| `None` | 输出的 JSON Schema |
| `read_only` | `bool` | `False` | 仅读取数据（无副作用）|
| `destructive` | `bool` | `False` | 可能导致破坏性修改 |
| `idempotent` | `bool` | `False` | 相同参数始终产生相同结果 |
| `defer_loading` | `bool` | `False` | 接受 `defer-loading` / `defer_loading`，用于标记发现阶段的声明 |
| `source_file` | `str` | `""` | 脚本的显式路径（相对于 Skill 目录）|

`tools/list` 返回的未加载 skill stub 还会显式带上 `annotations.deferredHint = true`。调用 `load_skill(...)` 后，stub 会被真实工具替换，且这些工具返回 `deferredHint = false`。

## 脚本查找优先级

加载 Skill 时，目录按以下优先级解析每个 ToolDeclaration 对应的脚本：

1. `ToolDeclaration.source_file` — 显式路径优先
2. `scripts/` 中文件名（去扩展名）与工具名匹配的脚本
3. 如果 Skill 只有一个脚本，则该脚本服务所有工具
4. 无脚本可匹配 — 工具在注册表中可见但无法执行

## 低级技能函数

如需简单的一次性扫描（不使用渐进式加载）：

```python
import os
from dcc_mcp_core import (
    SkillScanner,
    SkillWatcher,
    SkillMetadata,
    parse_skill_md,
    scan_skill_paths,
    scan_and_load,
    scan_and_load_lenient,
)

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

# 一次性扫描 + 加载 + 依赖排序 → 返回 (skills, skipped_dirs)
skills, skipped = scan_and_load(extra_paths=["/my/skills"], dcc_name="maya")
skills_lenient, skipped = scan_and_load_lenient(dcc_name="maya")  # 跳过错误

# 扫描目录中的 SKILL.md 文件
scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")

# 解析单个 Skill 目录
metadata = parse_skill_md("/path/to/maya-geometry")

# 获取原始 Skill 目录路径列表
paths = scan_skill_paths(extra_paths=["/my/skills"], dcc_name="maya")
```

## 使用 SkillWatcher 实现热重载

`SkillWatcher` 监控文件系统，当 `SKILL.md` 文件更改时自动重新加载技能：

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/path/to/skills")

# 获取当前技能（快照）
skills = watcher.skills()          # List[SkillMetadata]
count = watcher.skill_count()      # int

# 手动重载
watcher.reload()

# 停止监控
watcher.unwatch("/path/to/skills")

# 查看已监控的路径
paths = watcher.watched_paths()    # List[str]
```

## 依赖管理

Skill 可通过 SKILL.md 中的 `depends:` 字段声明对其他 Skill 的依赖：

```yaml
---
name: maya-animation
depends: ["maya-geometry"]
---
```

```python
from dcc_mcp_core import (
    resolve_dependencies,
    validate_dependencies,
    expand_transitive_dependencies,
)

skills, _ = scan_and_load()

# 拓扑排序（每个 Skill 在其依赖项之后出现）
ordered = resolve_dependencies(skills)

# 验证 — 返回错误消息列表
errors = validate_dependencies(skills)

# 指定 Skill 的所有传递依赖
deps = expand_transitive_dependencies(skills, "maya-animation")
# ["maya-geometry"]
```

## SkillMetadata 字段

从 SKILL.md frontmatter 解析。同时支持 Anthropic Skills、ClawHub/OpenClaw 和 dcc-mcp-core 扩展格式。

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 唯一 Skill 名称 |
| `description` | `str` | 简短描述 |
| `tools` | `List[str]` | frontmatter 中列出的工具名称 |
| `dcc` | `str` | 目标 DCC 应用（默认：`"python"`）|
| `tags` | `List[str]` | 分类标签 |
| `scripts` | `List[str]` | 发现的脚本文件路径 |
| `skill_path` | `str` | Skill 目录的绝对路径 |
| `version` | `str` | Skill 版本（默认：`"1.0.0"`）|
| `depends` | `List[str]` | 依赖的 Skill 名称 |
| `metadata_files` | `List[str]` | `metadata/` 目录中的 `.md` 文件路径 |

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_{APP}_SKILL_PATHS` | 应用专属 Skill 路径，如 `DCC_MCP_MAYA_SKILL_PATHS`（Windows 用 `;`，Unix 用 `:`）|
| `DCC_MCP_SKILL_PATHS` | 全局兜底 Skill 路径（应用专属变量未设置时使用）|

::: tip 应用专属路径优先级更高
对于 `app_name="maya"`，`DCC_MCP_MAYA_SKILL_PATHS` 优先检查，`DCC_MCP_SKILL_PATHS` 作为全局兜底。
:::

## 一键 Skills-First 启动：`create_skill_manager`

使用 `create_skill_manager`（v0.12.12+）可以一键完成所有配置，将 `ActionRegistry`、`ActionDispatcher`、`SkillCatalog` 和 `McpHttpServer` 组合在一起：

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

# 设置应用专属 Skill 路径
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

# 一键：发现 Skills + 启动 MCP HTTP 服务器
server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP 服务器地址：{handle.mcp_url()}")
# AI 客户端连接到 http://127.0.0.1:8765/mcp
```

`create_skill_manager` 自动完成：
1. 创建 `ActionRegistry` 和 `ActionDispatcher`
2. 创建连接到 dispatcher 的 `SkillCatalog`
3. 从 `DCC_MCP_MAYA_SKILL_PATHS` 和 `DCC_MCP_SKILL_PATHS` 发现 Skills
4. 返回已配置好的 `McpHttpServer`

```python
def create_skill_manager(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer: ...
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `app_name` | `str` | DCC 应用名（`"maya"`、`"blender"` 等）— 用于推导环境变量名和服务器名 |
| `config` | `McpHttpConfig \| None` | HTTP 配置；默认端口 8765 |
| `extra_paths` | `list[str] \| None` | 额外扫描的 Skill 目录 |
| `dcc_name` | `str \| None` | 覆盖 Skill 扫描的 DCC 过滤条件（默认与 `app_name` 相同）|

## 支持的脚本类型

| 扩展名 | 类型 | 执行方式 |
|--------|------|----------|
| `.py` | Python | `python` 解释器 |
| `.sh`, `.bash` | Shell | `bash` |
| `.bat`, `.cmd` | Batch | `cmd /C` |
| `.mel` | MEL（Maya）| `python` 包装器 |
| `.ms` | MaxScript（3ds Max）| `python` 包装器 |
| `.lua`, `.hscript` | Lua / Houdini | `python` 包装器 |

::: tip Skills-First 架构
推荐使用 `create_skill_manager` 作为 v0.12.12+ 的首选入口。它将 `SkillCatalog` 自动脚本执行与 MCP HTTP 服务集成，Agent 无需任何手动处理器注册即可通过 `tools/call` 调用工具。
:::

::: warning 脚本执行
所有脚本作为子进程运行。输入参数通过 stdin 以 JSON 格式传入。脚本应将 JSON 结果写入 stdout，成功时退出码为 0。
:::
