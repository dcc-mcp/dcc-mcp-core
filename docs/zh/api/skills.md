# Skills API

`dcc_mcp_core.SkillCatalog`、`dcc_mcp_core.SkillScanner`、`dcc_mcp_core.SkillWatcher`、`dcc_mcp_core.SkillMetadata`、`dcc_mcp_core.SkillSummary`、`dcc_mcp_core.ToolDeclaration`、`dcc_mcp_core.parse_skill_md`、`dcc_mcp_core.scan_and_load`

## SkillCatalog

渐进式 Skill 发现与加载。管理 Skill 从发现到活跃工具注册的完整生命周期。

```python
from dcc_mcp_core import ActionRegistry, SkillCatalog

registry = ActionRegistry()
catalog = SkillCatalog(registry)
```

### 构造函数

```python
SkillCatalog(registry: ActionRegistry) -> SkillCatalog
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `discover(extra_paths=None, dcc_name=None)` | `int` | 发现 Skill；返回新发现的数量 |
| `load_skill(skill_name)` | `List[str]` | 加载 Skill；返回已注册的 Action 名称。未找到则抛出 `ValueError` |
| `unload_skill(skill_name)` | `int` | 卸载 Skill；返回移除的 Action 数量。未加载则抛出 `ValueError` |
| `find_skills(query=None, tags=[], dcc=None)` | `List[SkillSummary]` | 按 query/tags/dcc 搜索 Skill（所有过滤器 AND 组合）|
| `list_skills(status=None)` | `List[SkillSummary]` | 列出所有 Skill。status：`"loaded"`、`"discovered"`、`"error"` 或 `None`（全部）|
| `get_skill_info(skill_name)` | `dict \| None` | 以 dict 形式返回详细信息，未找到返回 `None` |
| `is_loaded(skill_name)` | `bool` | 指定 Skill 是否已加载 |
| `loaded_count()` | `int` | 已加载 Skill 的数量 |
| `__len__()` | `int` | 目录中 Skill 总数 |
| `__bool__()` | `bool` | 目录为空时返回 False |
| `__repr__()` | `str` | `SkillCatalog(total=N, loaded=N)` |

### 示例

```python
import os
from dcc_mcp_core import ActionRegistry, SkillCatalog

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

registry = ActionRegistry()
catalog = SkillCatalog(registry)

# 发现 Skill
count = catalog.discover(extra_paths=["/extra/skills"], dcc_name="maya")

# 列出所有已发现的 Skill
for skill in catalog.list_skills():
    status = "已加载" if skill.loaded else "未加载"
    print(f"  [{status}] {skill.name} v{skill.version}: {skill.description}")

# 搜索
results = catalog.find_skills(query="geometry", tags=["create"])
for s in results:
    print(f"  {s.name}: {s.tool_count} 个工具 → {s.tool_names}")

# 加载 Skill
actions = catalog.load_skill("maya-geometry")
print(f"已注册：{actions}")
# ['maya_geometry__create_sphere', 'maya_geometry__export_fbx']

# 检查已加载 Skill
print(catalog.loaded_count(), len(catalog))

# 卸载
n = catalog.unload_skill("maya-geometry")
print(f"已移除 {n} 个 Action")
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
2. `DCC_MCP_SKILL_PATHS` 环境变量
3. 平台特定 Skill 目录（DCC 特定，通过 `get_skills_dir(dcc_name)`）
4. 平台特定 Skill 目录（全局，通过 `get_skills_dir()`）

## 环境变量

| 变量 | 说明 |
|------|------|
| `DCC_MCP_SKILL_PATHS` | Skill 搜索路径（Windows 用 `;`，Unix 用 `:`）|

## Action 命名规则

`SkillCatalog.load_skill()` 注册工具时，Action 名称遵循以下格式：

```
{skill名称（连字符转下划线）}__{工具名称}
```

示例：
- Skill `maya-geometry`，工具 `create_sphere` → `maya_geometry__create_sphere`
- Skill `blender-utils`，工具 `render-scene` → `blender_utils__render_scene`
