# Skills API

`dcc_mcp_core.SkillCatalog`、`dcc_mcp_core.SkillScanner`、`dcc_mcp_core.SkillWatcher`、`dcc_mcp_core.SkillMetadata`、`dcc_mcp_core.SkillSummary`、`dcc_mcp_core.ToolDeclaration`、`dcc_mcp_core.parse_skill_md`、`dcc_mcp_core.scan_and_load`、`dcc_mcp_core.register_metadata_driven_tools`

`dcc_mcp_core.skill`（纯 Python）：`skill_entry`、`skill_success`、`skill_error`、`skill_warning`、`skill_exception`、`run_main`

## SkillCatalog

渐进式 Skill 发现与加载。线程安全（所有状态存储在 DashMap/DashSet 中）。

当通过 `with_dispatcher()` 附加了调度器时，加载 Skill 会自动为每个 Action 注册基于子进程的处理器 — 启用 Skills-First 工作流，Agent 无需手动注册处理器。

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry

registry = ToolRegistry()
catalog = SkillCatalog(registry)
```

### 构造函数

```python
SkillCatalog(registry: ToolRegistry) -> SkillCatalog
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `registry` | `ToolRegistry` | 用于注册/注销工具的 Action 注册表 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `with_dispatcher(dispatcher)` | — | 附加 `ToolDispatcher`；启用 `load_skill()` 时的自动处理器注册 |
| `new_with_dispatcher(registry, dispatcher)` | — | 创建带 dispatcher 的 catalog（构造器式） |
| `discover(extra_paths=None, dcc_name=None)` | `int` | 扫描并填充目录；返回新发现的 skill 数量 |
| `load_skill(skill_name)` | `List[str]` | 加载 Skill；返回注册的 action 名称列表，未找到则报错 |
| `load_skills(skill_names)` | `dict` | 批量加载；返回 `{name: Ok(actions) or Err(msg)}` |
| `unload_skill(skill_name)` | `int` | 卸载 Skill；返回移除的 action 数量，未加载则报错 |
| `remove_skill(skill_name)` | `bool` | 从目录中完全移除（已加载则先卸载） |
| `clear()` | `None` | 清空所有 Skill（已加载的先卸载） |
| `search_skills(query=None, tags=None, dcc=None, scope=None, limit=None)` | `List[SkillSummary]` | 统一发现：支持 `scope`（`"repo" \| "user" \| "system" \| "admin"`）和 `limit`。空调用按 scope 优先级返回顶级 Skill（Admin > System > User > Repo）。 |
| `list_skills(status=None)` | `List[SkillSummary]` | 列出 Skill。status：`"loaded"`、`"unloaded"`、`"pending_deps"` 或 `"error"`，`None` 为全部 |
| `get_skill_info(skill_name)` | `SkillMetadata \| None` | 返回完整元数据，未找到返回 `None` |
| `is_loaded(skill_name)` | `bool` | 指定 Skill 是否已加载 |
| `loaded_count()` | `int` | 已加载 Skill 的数量 |
| `__repr__()` | `str` | 字符串表示 |

### 示例

```python
import os
from dcc_mcp_core import SkillCatalog, ToolRegistry, ToolDispatcher

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

registry = ToolRegistry()
dispatcher = ToolDispatcher(registry)
catalog = SkillCatalog.new_with_dispatcher(registry, dispatcher)

# 发现 Skill
catalog.discover(extra_paths=["/extra/skills"], dcc_name="maya")

# 列出所有已发现的 Skill
for skill in catalog.list_skills():
    status = "loaded" if skill.loaded else "unloaded"
    print(f"  [{status}] {skill.name} v{skill.version}: {skill.description}")

# 搜索
results = catalog.search_skills(query="geometry", tags=["create"])
for s in results:
    print(f"  {s.name}: {s.tool_count} tools → {s.tool_names}")

# 加载 Skill（附加调度器后 Action 自动注册）
actions = catalog.load_skill("maya-geometry")
print(f"已注册 actions: {actions}")

# 获取完整元数据
meta = catalog.get_skill_info("maya-geometry")
if meta:
    print(meta.name, meta.tools)

# 查看已加载数量
print(catalog.loaded_count())

# 卸载
removed = catalog.unload_skill("maya-geometry")
print(f"已移除 {removed} 个 action")
```

---

## SkillSummary

`SkillCatalog.search_skills()` 和 `list_skills()` 返回的轻量级摘要对象。

### 属性（只读）

| 属性 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | Skill 名称 |
| `description` | `str` | 简短描述 |
| `search_hint` | `str` | 发现关键词提示（来自 SKILL.md `search-hint:`；回退到 `description`） |
| `tags` | `List[str]` | Skill 标签 |
| `dcc` | `str` | 目标 DCC（如 `"maya"`）|
| `version` | `str` | Skill 版本 |
| `tool_count` | `int` | 声明的工具数量 |
| `tool_names` | `List[str]` | 声明的工具名称列表 |
| `loaded` | `bool` | 当前是否已加载 |
| `status` | `str` | 机器可读加载状态：`"discovered"`、`"pending_deps"`、`"loaded"` 或 `"error"` |
| `missing_dependencies` | `List[str]` | 当前 catalog 中尚未出现的依赖 Skill 名称 |

### 特殊方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `SkillSummary(name='...', loaded=True)` |

---

## ToolDeclaration

Skill 中的单个工具声明，从 `metadata.dcc-mcp.tools` 指向的同级 `tools.yaml` 解析；旧的 SKILL.md 顶层 `tools:` 不再是有效新格式。

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
    defer_loading: bool = False,
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
| `defer_loading` | `bool` | `False` | 解析 SKILL.md 中的 `defer-loading:` / `defer_loading:`，供发现型 UI 使用 |
| `source_file` | `str` | `""` | 脚本文件的显式路径 |

::: tip input_schema 和 output_schema
内部以 JSON 值存储，非字符串。从 Python 构造时传入 JSON 字符串，会自动解析。
:::

::: tip 渐进式加载信号
`tools/list` 返回的未加载 skill stub 现在会带 `annotations.deferredHint = true`。调用 `load_skill(...)` 后，真实工具会以 `deferredHint = false` 暴露。
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
    search_hint: str = "",
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
| `search_hint` | `str` | `search_skills` 的关键词提示（SKILL.md `search-hint:` 字段；回退到 `description`） |
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

与 `scan_and_load` 相同，但会保留缺失软依赖的 Skill，让它们仍可被发现（通过日志记录警告）。缺失依赖只会在排序时被忽略；已经存在的依赖仍会排在依赖方之前。仅循环依赖会抛出 `ValueError`。

### register_metadata_driven_tools

```python
register_metadata_driven_tools(
    server,
    *,
    skills: Sequence[Any] | None = None,
    skipped: Sequence[Any] | None = None,
    dcc_name: str = "dcc",
    extra_paths: Iterable[str] | None = None,
    registrations: Sequence[MetadataExtensionRegistration | Callable | tuple[str, Callable]] | None = None,
    scan: Callable | None = None,
    phase: str = "startup",
) -> MetadataRegistrationReport
```

注册由已加载 Skill metadata 派生的可选工具。未传入 `skills` 时，helper
会先调用一次 `scan_and_load_lenient(extra_paths=..., dcc_name=...)`，然后按
`callback(server, skills=loaded_skills, dcc_name=dcc_name)` 调用每个扩展回调。

默认注册项包括：

- `recipes` → `register_recipes_tools`
- `skill-reference-docs` → `register_skill_reference_docs_tools`

Adapter 可以传入自定义回调或懒加载 import 描述：

```python
from dcc_mcp_core import (
    imported_metadata_extension,
    register_metadata_driven_tools,
)

report = register_metadata_driven_tools(
    server,
    dcc_name="maya",
    extra_paths=[studio_skill_root],
    registrations=[
        imported_metadata_extension(
            "recipes",
            "dcc_mcp_core.recipes",
            "register_recipes_tools",
        ),
        imported_metadata_extension(
            "refs",
            "dcc_mcp_core.skill_reference_docs",
            "register_skill_reference_docs_tools",
        ),
    ],
)
logger.info("metadata tools: %s", report.to_dict())
```

返回的 report 会记录每个扩展的 `registered`、`failed`、`skipped` 状态。
一个可选扩展导入或注册失败，不会阻止后续扩展继续执行。

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

### create_skill_server

```python
create_skill_server(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer
```

**Skills-First 工作流的推荐入口**（v0.12.12+）。

一次调用即可为指定 DCC 应用创建完整配置的 `McpHttpServer`，自动完成：
1. 创建 `ToolRegistry` + `ToolDispatcher`
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
from dcc_mcp_core import create_skill_server, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

server = create_skill_server("maya", McpHttpConfig(port=8765))
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

## Rust 后端 Skill 辅助工具

`dcc_mcp_core.skills_helper` 是面向 Skill 脚本的 canonical import path。
当完整 `dcc-mcp-core` wheel 可用时，Skill 作者应优先从这里导入轻量辅助
API，而不是创建零散的 `utils` 模块，或为 JSON、YAML、file/path、LZ4 payload
compression、schema validation、结果 envelope、参数规范化、取消检查等小功能新增
Python runtime dependency。

```python
from dcc_mcp_core.skills_helper import (
    check_cancelled,
    json_dumps,
    json_loads,
    normalize_tool_arguments,
    skill_success,
    yaml_dumps,
    yaml_loads,
)
```

JSON 和 YAML helper 由 Rust/PyO3 bridge 提供，并继续保留向后兼容的顶层导入：

```python
from dcc_mcp_core.skills_helper import json_dumps, json_loads, yaml_dumps, yaml_loads

payload = json_loads('{"name": "cube"}')
text = json_dumps(payload, ensure_ascii=False)
config = yaml_loads("enabled: true\n")
```

处理文件时，优先使用带 source context 的 helper，让 UTF-8、大小限制和解析
错误在不同 Skill 中保持一致：

```python
from dcc_mcp_core.skills_helper import (
    SkillCodecError,
    dump_json_file,
    load_json_file,
    load_yaml_file,
)

try:
    manifest = load_json_file("manifest.json", require_mapping=True, max_bytes=1_000_000)
    settings = load_yaml_file("settings.yaml", require_mapping=True)
except SkillCodecError as exc:
    return skill_error_from_exception(exc)

dump_json_file("out/report.json", manifest, ensure_ascii=False)
```

处理生成 artefact 和本地 hand-off payload 时，优先使用 Rust-backed file/path
helper，而不是在每个 Skill 里复制一次小工具：

```python
from dcc_mcp_core.skills_helper import (
    SkillFileError,
    atomic_write_text,
    compress_bytes,
    decompress_bytes,
    ensure_within_root,
    file_digest,
)

try:
    out_path = atomic_write_text(
        "reports/summary.json",
        json_dumps(summary, ensure_ascii=False),
        root=session_temp_dir,
        max_bytes=2_000_000,
    )
    sha256 = file_digest(out_path, root=session_temp_dir)
    packed = compress_bytes(out_path.read_bytes(), max_bytes=2_000_000)
    restored = decompress_bytes(packed, max_bytes=2_000_000)
except SkillFileError as exc:
    return skill_error_from_exception(exc)
```

`ensure_within_root(root, path)` 会把相对路径解析到可信 workspace/session root
下，canonicalize 已存在的祖先路径，并拒绝逃逸 root 的 traversal。
`atomic_write_text()` / `atomic_write_bytes()` 通过同目录临时文件完成写入。
`file_digest()` / `bytes_digest()` 当前支持 SHA-256；BLAKE3 会等 wheel 已因
其他能力引入相关依赖时再补。`compress_bytes()` / `decompress_bytes()` 复用
shared-memory 层现有 LZ4 frame 实现，并强制显式 byte limit。

如果文件需要交给另一个 tool，或需要通过 MCP resources 暴露，应继续使用
`FileRef`、`artefact_put_file()` 和 `artefact_get_bytes()`。`skills_helper`
里的 file helper 是单个 Skill 或 session root 内部的低层 building block，
不替代更高层的 artefact store contract。

有边界的 REST 调用应优先使用 Rust-backed HTTP helper，而不是为常见 JSON API
额外引入 `requests`：

```python
from dcc_mcp_core.skills_helper import (
    SkillHttpError,
    http_get_json,
    http_post_json,
    skill_error_from_exception,
)

try:
    info = http_get_json(
        "https://pipeline.example/api/asset",
        query={"name": asset_name},
        headers={"Authorization": f"Bearer {token}"},
        timeout_ms=5_000,
        max_bytes=1_000_000,
    )
    created = http_post_json(
        "https://pipeline.example/api/report",
        {"asset": asset_name, "info": info},
        timeout_ms=5_000,
    )
except SkillHttpError as exc:
    return skill_error_from_exception(exc)
```

`http_request()` 返回 `HttpResponse`，包含 `status`、`headers`、`bytes`、
`text`、`json()`、`url`、`elapsed_ms` 和 `truncated`。在错误或 audit metadata
里回显 headers 前，先调用 `redact_http_headers()`。只有在需要 session、
streaming protocol、multipart upload、自定义 auth/retry flow，或 API 专有行为时，
才保留领域专用 HTTP client dependency。

现有 `from dcc_mcp_core import json_dumps` 仍可使用，并 re-export 同一组
canonical functions。新的 Skill authoring helper 应放在 `skills_helper`，
不要放进含义模糊的 `utils` 命名空间。

生成的 Python Skill 脚本在完整 wheel 可用时，也应通过这个命名空间导入标准
runner 和结果 helper：

```python
from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success
```

适合使用 `skills_helper` 的场景：

- Skill 需要 dependency-free JSON 或 YAML parsing，而不是 `json`、PyYAML 或本地 wrapper；
- Skill 需要有边界的 atomic write、safe path containment、SHA-256 digest 或
  本地 session file 的 LZ4 compression；
- handler 需要 `skill_success`、`skill_error`、`success_result`、`error_result` 等标准结果 helper；
- 工具 wrapper 需要 `normalize_tool_arguments()` / `normalize_tool_meta()` 来遵循共享 MCP/REST call envelope contract；
- 长时间运行的脚本需要 `check_cancelled()` / `check_dcc_cancelled()`；
- 当前安装版本的 `skills_helper` 已覆盖某个有边界的 HTTP 或 file/path
  helper；只有在 `skills_helper` 不覆盖所需行为时，才继续使用 `requests`
  或领域专用 file library。

只有当某个 Python dependency 承载 `skills_helper` 不覆盖的真实领域行为时，
才应继续引入该 dependency。

暂不暴露 TOML helper。当前 adapter 与 bundled skill 的 TOML 使用主要是由
core loader 处理的 metadata，并不是 Skill runtime 脚本直接读写；同时为了
支持 Python 3.7，稳定 TOML read/write API 会引入额外 runtime dependency。
等 Skill runtime 里出现 dependency-free TOML 需求时再补充。

---

## Skill Script 辅助工具（纯 Python）

`dcc_mcp_core.skill` 是**纯 Python** 子模块 — 无需编译扩展即可使用。
Skill 脚本作者可在 DCC 环境内直接导入，即便完整 wheel 包未安装也可正常运行。

```python
from dcc_mcp_core.skill import skill_entry, skill_success, skill_error
```

所有辅助函数返回普通 `dict`，与 `ToolResult` 完全兼容。
若 `dcc_mcp_core._core` 可用，可将 dict 传入 `validate_action_result()` 获得类型化的 `ToolResult` 对象。

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

dict 结构完全相同 — 两者均与 `ToolResult` 兼容。

---

## 结果序列化 — `serialize_result` / `deserialize_result`

基于 Rust 实现的 `ToolResult` 序列化工具。格式通过 `SerializeFormat` 枚举切换：当前使用 JSON，未来可升级到 MessagePack——调用方代码无需修改。

```python
from dcc_mcp_core import (
    serialize_result, deserialize_result, SerializeFormat, success_result
)
```

---

### SerializeFormat

```python
class SerializeFormat:
    Json: SerializeFormat     # UTF-8 JSON 文本（默认）
    MsgPack: SerializeFormat  # 二进制 MessagePack（via rmp-serde）
```

---

### serialize_result

```python
serialize_result(
    result: ToolResult,
    format: SerializeFormat = SerializeFormat.Json,
) -> str | bytes
```

序列化 `ToolResult`。

| `format` | 返回类型 | 说明 |
|----------|---------|------|
| `SerializeFormat.Json` | `str` | UTF-8 JSON 字符串 |
| `SerializeFormat.MsgPack` | `bytes` | 二进制 MessagePack |

```python
arm = success_result("时间线已更新", start_frame=1, end_frame=120)

# JSON（默认）
json_str = serialize_result(arm)
assert isinstance(json_str, str)

# MessagePack
msgpack_bytes = serialize_result(arm, SerializeFormat.MsgPack)
assert isinstance(msgpack_bytes, bytes)
```

---

### deserialize_result

```python
deserialize_result(
    data: str | bytes,
    format: SerializeFormat = SerializeFormat.Json,
) -> ToolResult
```

将 `str`（JSON）或 `bytes`（MsgPack）反序列化为 `ToolResult`。*format* 必须与序列化时使用的格式一致。

```python
original = success_result("完成", frame_count=240)
roundtrip = deserialize_result(serialize_result(original))
assert roundtrip.success
assert roundtrip.message == "完成"
assert roundtrip.context["frame_count"] == 240
```

---

### `run_main` 的序列化流程

`run_main()` 在 `_core` 可用时自动使用 `serialize_result`，在纯 Python 环境中回退到 `json.dumps`：

```
result dict
    ↓ validate_action_result()  （类型安全验证）
ToolResult
    ↓ serialize_result(arm, SerializeFormat.Json)   （Rust JSON 写入器）
JSON 字符串 → stdout
```

未来切换到 MessagePack 时，只需修改 `skill.py` 中的 `_serialize_result()`——`serialize_result` / `deserialize_result` API 保持稳定。
