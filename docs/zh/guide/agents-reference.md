# Agents 参考 — 详细规则与陷阱

**[English](../../guide/agents-reference.md)**

> 本文件是 `AGENTS.md` 的详细补充。
> `AGENTS.md` 是导航地图（≤150 行）；本文件包含
> 代理按需查阅的扩展规则、代码示例和陷阱。
> 请先阅读 `AGENTS.md`，需要细节时再按链接查阅此处。

---

## 陷阱 — 详细参考

以下是最常见的错误，每条检查不超过 10 秒。

**`scan_and_load` 返回 2-元组 — 始终解包：**
```python
# ✓
skills, skipped = scan_and_load(dcc_name="maya")
# ✗ 直接迭代得到 (list, list)，不是 skill 对象
```

**`success_result` / `error_result` — kwargs 进入 context，而不是 `context=` kwarg：**
```python
# ✓
result = success_result("done", prompt="hint", count=5)
# result.context == {"count": 5}
```

**`ToolDispatcher` — 只用 `.dispatch()`，永远不要用 `.call()`：**
```python
dispatcher = ToolDispatcher(registry)          # 仅一个参数
result = dispatcher.dispatch("name", json_str)   # 返回 dict
```

**异步 `tools/call` 分发 (#318) — opt-in，非阻塞：**
```python
# 以下任一条件会将调用路由到 JobManager 并立即返回
# {job_id, status: "pending"}：
#   1. 请求携带 _meta.dcc.async = true
#   2. 请求携带 _meta.progressToken
#   3. 工具的 ActionMeta 声明 execution: async 或 timeout_hint_secs > 0
# 否则分发给同步处理（与 #318 之前的行为字节一致）。
body = {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
    "name": "render_frames",
    "arguments": {"start": 1, "end": 250},
    "_meta": {"dcc": {"async": True, "parentJobId": "<uuid-or-null>"}},
}}
# → result.structuredContent = {"job_id": "<uuid>", "status": "pending",
#                               "parent_job_id": "<uuid>|null"}
# 通过 jobs.get_status (#319) 轮询；取消父任务会取消每个子任务
# 其 _meta.dcc.parentJobId 匹配（CancellationToken 子令牌级联）。
```

**`ToolRegistry.register()` — 仅关键字参数，不支持位置参数：**
```python
registry.register(name="my_tool", description="...", dcc="maya")
```

**工具注解位于同级 `tools.yaml` 中，绝不在 SKILL.md 顶层 (#344)：**
将 MCP `ToolAnnotations` 声明为每个工具条目下的嵌套 `annotations:` 映射
（或旧式简写的平面 `*_hint:` 键）。当两种形式同时存在时，嵌套映射优先。
`deferred_hint` 是 dcc-mcp-core 扩展，搭载在 `tools/list` 的
`_meta["dcc.deferred_hint"]` 中 — 绝不在 spec `annotations` 映射内。
完整指南：`docs/guide/skills.md#declaring-tool-annotations-issue-344`。

**SKILL.md 同级文件模式 — 每个新扩展的规则 (v0.15+ / #356)：**

**不要**向 `SKILL.md` 添加新的顶层 frontmatter 键。agentskills.io
1.0 仅允许 `name`、`description`、`license`、`compatibility`、
`metadata`、`allowed-tools` 在顶层。每个 dcc-mcp-core
扩展 — `tools`、`groups`、`workflows`、`prompts`、行为链、
注解、模板、示例包以及任何未来的扩展 — 必须表示为：

1. `metadata:` 下使用 `dcc-mcp.<feature>` 约定的**命名空间键**。
2. 该键的**值是一个 glob 或文件名**，指向携带实际载荷的同级文件（YAML 或 Markdown）。
3. 同级文件位于**skill 目录内部**，而非内联在 `SKILL.md` 中。

```yaml
---
name: maya-animation
description: >-
  Maya animation keyframes, timeline, curves. Use when the user asks to
  set/query keyframes, change timeline range, or bake simulations.
license: MIT
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.tools: "tools.yaml"              # ✓ 指向同级文件
  dcc-mcp.groups: "tools.yaml"             # ✓ 同一或单独文件
  dcc-mcp.workflows: "workflows/*.workflow.yaml"
  dcc-mcp.prompts: "prompts/*.prompt.yaml"
  dcc-mcp.examples: "references/EXAMPLES.md"
---
# body — 仅人类可读的说明
```

加载器可**互换**接受两种形式 — 平面点分键
（`dcc-mcp.dcc: maya`）和 `yaml.safe_dump` 及迁移工具生成的嵌套映射：

```yaml
metadata:
  dcc-mcp:
    dcc: maya
    tools: "tools.yaml"
    groups: "groups.yaml"
```

新 skill 优先使用嵌套形式；它能通过标准 YAML 工具正确往返，无需逐键引号。

```
maya-animation/
├── SKILL.md                    # metadata map + body
├── tools.yaml                  # tools + groups
├── workflows/
│   ├── vendor_intake.workflow.yaml
│   └── nightly_cleanup.workflow.yaml
├── prompts/
│   └── review_scene.prompt.yaml
└── references/
    └── EXAMPLES.md
```

此规则不可协商的原因：

- **`skills-ref validate` 通过** — 没有自定义顶层字段。
- **渐进式披露** — 代理仅为实际需要的同级文件消耗 token；一个 60 工具的 skill 保持低索引开销。
- **可差异比较** — 每个 workflow/prompt 文件一个 PR，而非埋在一个巨大的 SKILL.md 块中。
- **前向兼容** — 未来扩展只需添加一个新的 `metadata.dcc-mcp.<x>` 键和新的同级 schema，无需重新协商 frontmatter 规范。

设计涉及 SKILL.md 的新功能时，设计审查门控是："这能否作为 `metadata.dcc-mcp.<feature>` 指向同级文件？"如果答案是否定的，请在实现前提交提案（参见 `docs/proposals/`）。

**`ToolRegistry` 方法名仍使用 "action"（v0.13 兼容性）：**
```python
# Rust API 在 v0.13 中将 action→tool 重命名，但部分方法名
# 仍保留 "action" 以保持向后兼容：
registry.get_action("create_sphere")           # 仍是 "get_action"
registry.list_actions(dcc_name="maya")         # 仍是 "list_actions"
registry.search_actions(category="geometry")   # 仍是 "search_actions"
# 这些不是 bug — 它们是兼容别名。
```

**DccLink IPC — 主要 RPC 路径 (v0.14+, issue #251)：**
```python
from dcc_mcp_core import DccLinkFrame, IpcChannelAdapter
channel = IpcChannelAdapter.connect("dcc-mcp-maya-12345")  # Named Pipe / UDS
channel.send_frame(DccLinkFrame(msg_type="Call", seq=1, body=b"{...}"))
reply = channel.recv_frame()   # DccLinkFrame: msg_type, seq, body
# Legacy FramedChannel.call / connect_ipc 已在 v0.14 中移除 (#251)。
```

**多客户端 IPC 服务器：**
```python
from dcc_mcp_core import SocketServerAdapter
server = SocketServerAdapter("/tmp/maya.sock", max_connections=8,
                             connection_timeout_secs=30)
```

**`DeferredExecutor` — 不在公共 `__init__` 中：**
```python
from dcc_mcp_core._core import DeferredExecutor   # 需要直接导入
```

**`McpHttpServer` — 在 `.start()` 之前注册所有处理器。**
这包括 `register_diagnostic_mcp_tools(...)` 用于实例绑定的诊断工具 —
在调用 `server.start()` 之前注册，绝不在之后。

**`Capturer.new_auto()` vs `.new_window_auto()`：**
```python
# ✓ 全屏 / 显示捕获（Windows 上 DXGI，Linux 上 X11）
Capturer.new_auto().capture()

# ✓ 单窗口捕获（Windows 上 HWND PrintWindow；其他平台为 Mock）
Capturer.new_window_auto().capture_window(window_title="Maya 2024")
# ✗ .new_auto() 后调用 .capture_window() — 可能返回错误的后端
```

**工具组 — 非活跃组被隐藏，而非删除：**
```python
# default_active=false 的工具在 tools/list 中隐藏但仍在 ToolRegistry 中。
# 使用 registry.list_actions()（显示全部）vs registry.list_actions_enabled()（仅活跃）。
registry.activate_tool_group("maya-geometry", "rigging")   # 发出 tools/list_changed
```

**`skill_success()` vs `success_result()` — 不同类型，不同用例：**
```python
# 在 skill 脚本中（纯 Python，返回 dict 用于子进程捕获）：
return skill_success("done", count=5)       # → {"success": True, ...} dict

# 在服务器代码中（返回 ToolResult 用于验证/传输）：
return success_result("done", count=5)      # → ToolResult 实例
```

**`SkillScope` — 更高作用域覆盖同名低作用域 skill：**
```python
# 作用域层级：Repo < User < System < Admin
# System 作用域的 skill 会静默遮蔽 Repo 作用域的同名 skill。
# 这防止了项目本地 skill 劫持企业管理 skill。
# 注意：SkillScope/SkillPolicy 是 Rust 层类型，不导出到 Python。
# 通过 SkillMetadata 访问作用域信息：metadata.is_implicit_invocation_allowed()，
# metadata.matches_product(dcc_name)。通过 SKILL.md frontmatter 配置：
#   allow_implicit_invocation: false
#   products: ["maya", "blender"]
```

**`allow_implicit_invocation: false` ≠ `defer-loading: true`：**
```yaml
# allow_implicit_invocation: false → skill 必须被显式 load_skill() 加载
# defer-loading: true → 工具桩出现在 tools/list 中但需要 load_skill()
# 两者都延迟工具可用性，但前者是*策略*（安全），
# 后者是*提示*（渐进加载）。两者结合可实现最大控制。
```

**MCP 安全 — 为安全的 AI 交互设计工具：**
```python
# 使用 ToolAnnotations 向 AI 客户端信号安全属性：
from dcc_mcp_core import ToolAnnotations
annotations = ToolAnnotations(
    read_only_hint=True,       # 工具仅读取数据，无副作用
    destructive_hint=False,    # 工具可能造成不可逆变更
    idempotent_hint=True,      # 重复调用产生相同结果
    open_world_hint=False,     # 工具可能与外部系统交互
    deferred_hint=None,        # 完整 schema 延迟到 load_skill（由服务器设置，非用户）
)
# 围绕用户工作流设计工具，而非原始 API 调用。
# 通过 error_result("msg", "specific error") 返回人类可读的错误。
# 当工具集变更时使用 notifications/tools/list_changed。
```

**`skill_warning()` / `skill_exception()` — 额外的 skill 辅助函数：**
```python
from dcc_mcp_core import skill_warning, skill_exception
# skill_warning() — 带警告的部分成功（success=True 但有附加说明）
# skill_exception() — 将异常包装为错误 dict 格式
# 两者都是 python/dcc_mcp_core/skill.py 中的纯 Python 辅助函数
```

**`next-tools` — 位于同级 `tools.yaml` 中，绝不在顶层 SKILL.md (issue #342)：**
```yaml
# tools.yaml  (从 SKILL.md 中通过 metadata.dcc-mcp.tools: tools.yaml 引用)
tools:
  - name: create_sphere
    next-tools:
      on-success: [maya_geometry__bevel_edges]    # 成功后建议
      on-failure: [dcc_diagnostics__screenshot]   # 失败时调试
```
- `next-tools` 是 dcc-mcp-core 扩展（不在 agentskills.io 规范中）
- 位于 `tools.yaml` 中每个工具条目内。SKILL.md 顶层 `next-tools:` 是旧式写法，会发出弃用警告并使 `is_spec_compliant() → False`。
- 出现在 `CallToolResult._meta["dcc.next_tools"]` — 服务器在成功后附加 `on_success`，在错误后附加 `on_failure`；未声明时完全省略。
- 无效的工具名在加载时被丢弃并发出警告 — skill 仍会加载。
- `on-success` 和 `on-failure` 都接受全限定工具名列表。

**agentskills.io 字段 — `license`、`compatibility`、`allowed-tools`：**
```yaml
---
name: my-skill
description: "Does X. Use when user asks to Y."
license: MIT                          # 可选 — SPDX 标识符或文件引用
compatibility: "Maya 2024+, Python 3.7+"  # 可选 — 环境要求
allowed-tools: Bash(git:*) Read       # 可选 — 预批准工具（实验性）
---
```
- `license` 和 `compatibility` 被解析到 `SkillMetadata` 字段
- `allowed-tools` 在 agentskills.io 规范中为实验性 — 空格分隔的工具字符串
- 大多数 skill 不需要 `compatibility`；仅在存在硬性要求时包含

**`external_deps` — 声明外部依赖（MCP 服务器、环境变量、二进制）：**
```python
import json
from dcc_mcp_core import SkillMetadata
# external_deps 是 SkillMetadata 上的 JSON 字符串字段
md.external_deps = json.dumps({
    "tools": [
        {"type": "mcp", "value": "github-mcp-server"},
        {"type": "env_var", "value": "GITHUB_TOKEN"},
        {"type": "bin", "value": "ffmpeg"},
    ]
})
# 读取：
deps = json.loads(md.external_deps) if md.external_deps else None
```
- 在 SKILL.md frontmatter 中声明为 `external_deps:`（YAML 映射）
- 解析为 `SkillMetadata.external_deps` JSON 字符串
- 通过 `json.loads(metadata.external_deps)` 访问 — 未设置时返回 `None`
- 完整 schema 参见 [Skill 作用域与策略](/guide/skill-scopes-policies)

**`CompatibilityRouter` — 不是独立的 Python 类：**
```python
# CompatibilityRouter 由 VersionedRegistry.router() 返回
# 不可直接导入 — 通过以下方式访问：
from dcc_mcp_core import VersionedRegistry
vr = VersionedRegistry()
router = vr.router()  # -> CompatibilityRouter（借用注册表）
# 大多数用例直接使用 VersionedRegistry.resolve() 即可
result = vr.resolve("create_sphere", "maya", "^1.0.0")
```

**SEP-986 工具命名 — 注册前验证名称：**
```python
from dcc_mcp_core import validate_tool_name, validate_action_id, TOOL_NAME_RE
# 工具名称：点分隔小写（如 "scene.get_info"）
validate_tool_name("scene.get_info")     # ✓ 通过
validate_tool_name("Scene/GetInfo")      # ✗ 抛出 ValueError
# Action ID：点分隔小写标识符链
validate_action_id("maya-geometry.create_sphere")  # ✓
# 用于自定义验证的正则常量：
# TOOL_NAME_RE, ACTION_ID_RE, MAX_TOOL_NAME_LEN (48 字符)
```

**Workflow 步骤策略 — 重试 / 超时 / 幂等性 (#353)：**
```python
from dcc_mcp_core import WorkflowSpec, BackoffKind
spec = WorkflowSpec.from_yaml_str(yaml)
spec.validate()  # idempotency_key 模板引用在此检查，而非解析时
retry = spec.steps[0].policy.retry
# next_delay_ms 是 1-索引：1 = 初始尝试（返回 0），2 = 首次重试
assert retry.next_delay_ms(1) == 0
assert retry.next_delay_ms(2) == retry.initial_delay_ms
# 指数退避：尝试 n >= 2 → initial * 2^(n-2)，限制在 max 内
```
- `max_attempts == 1` 表示**无重试**（而非"重试一次"）
- `retry_on: None` = 所有错误可重试；`retry_on: []` = 无错误可重试
- `idempotency_scope` 默认为 `"workflow"`（每次调用），设为 `"global"` 可跨调用
- 模板根必须在 `inputs`/`steps`/`item`/`env`、顶层输入键或步骤 id 中 — 在 `validate()` 时静态检查

**`lazy_actions` — opt-in 元工具快速路径：**
```python
# 启用后，tools/list 仅显示 3 个元工具：
# list_actions, describe_action, call_action
# 而非一次性显示每个已注册工具。
config = McpHttpConfig(port=8765)
config.lazy_actions = True   # opt-in；默认为 False
```

**`bare_tool_names` — 冲突感知的裸操作名 (#307)：**
```python
# 默认 True。tools/list 在裸名称唯一时发出 "execute_python"
# 而非 "maya-scripting.execute_python"。
# 冲突时回退到完整的 "<skill>.<action>" 形式。
# tools/call 在一个发布周期内接受两种形式。
config = McpHttpConfig(port=8765)
config.bare_tool_names = True   # 默认

# 仅当下游客户端硬编码了前缀形式
# 且无法同步更新时选择退出：
config.bare_tool_names = False
```

**`ToolResult.to_json()` — JSON 序列化：**
```python
result = success_result("done", count=5)
json_str = result.to_json()    # JSON 字符串
# 另有：result.to_dict()       # Python dict
```

---

## Do 和 Don't — 完整参考

### Do ✅

- 使用 `create_skill_server("maya", McpHttpConfig(port=8765))` — v0.12.12 以来的 Skills-First 入口
- 使用 `success_result("msg", count=5)` — 额外 kwargs 变为 `context` dict
- 使用 `ToolAnnotations(read_only_hint=True, destructive_hint=False)` — 帮助 AI 客户端安全选择
- 在 SKILL.md 中使用 `next-tools: on-success/on-failure` — 引导 AI 代理到后续工具
- 在 SKILL.md 中使用 `search-hint:` — 改善 `search_skills` 关键词匹配
- 对高级用户功能使用 `default_active: false` 的工具组 — 保持 `tools/list` 精简
- **为每个 skill 标记 `metadata.dcc-mcp.layer`** — `infrastructure`、`domain` 或 `example`。参见 `skills/README.md#skill-layering`。
- **每个 skill `description` 以 layer 前缀开头**（`Infrastructure skill —` / `Domain skill —` / `Example skill —`）后跟"Not for X — use Y"否定路由句
- **保持 `search-hint` 在各层间不重叠** — infrastructure：机制导向；domain：意图导向；example：附加"authoring reference"
- **将每个 domain skill 工具的 `on-failure` 连接到** `[dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]`
- **在每个使用 `on-failure` 链的 domain skill 中声明 `depends: [dcc-diagnostics]`**
- 对每个新的 SKILL.md 扩展，使用 `metadata.dcc-mcp.<feature>` 键指向同级文件（参见陷阱中的"SKILL.md 同级文件模式"）。`tools`、`groups`、`workflows`、`prompts` 及任何未来扩展同理。
- 解包 `scan_and_load()`：`skills, skipped = scan_and_load(dcc_name="maya")`
- 在 `McpHttpServer.start()` **之前**注册所有处理器 — 服务器在启动时读取注册表
- 对 AI 驱动的工具执行使用 `SandboxPolicy` + `InputValidator`
- 使用 `DccServerBase` 作为 DCC 适配器的基类 — skill/lifecycle/gateway 已继承
- 在 `vx just test` 之前使用 `vx just dev` — 必须先编译 Rust 扩展
- 保持 `SKILL.md` body 在 500 行 / 5000 token 以下 — 细节移至 `references/`
- PR 标题使用 Conventional Commits — `feat:`、`fix:`、`docs:`、`refactor:`
- 使用 `registry.list_actions()`（显示全部）vs `registry.list_actions_enabled()`（仅活跃）
- 查找工具时从 `search_skills(query)` 开始 — 不要猜测工具名。`search_skills` 接受 `tags`、`dcc`、`scope` 和 `limit`；无参调用可按信任作用域浏览。
- 在多网关设置中使用 `init_file_logging(FileLoggingConfig(...))` 获取持久日志；调用 `flush_logs()` 立即将事件写入磁盘
- 在 `tools/call` 中使用裸工具名 — `execute_python` 和 `maya-scripting.execute_python` 在一个版本的宽限期内都有效

### Don't ❌

- 不要直接迭代 `scan_and_load()` 结果 — 它返回 `(list, list)`，不是 skill 对象
- 不要使用 `success_result("msg", context={"count": 5})` — kwargs 自动进入 context
- 不要调用 `ToolDispatcher.call()` — 方法是 `.dispatch(name, json_str)`
- 不要向 `ToolRegistry.register()` 传位置参数 — 仅限关键字参数
- 不要从 Python 导入 `SkillScope` 或 `SkillPolicy` — 它们是 Rust-only 类型
- 不要从公共 `__init__` 导入 `DeferredExecutor` — 使用 `from dcc_mcp_core._core import DeferredExecutor`
- 不要先调用 `.new_auto()` 再调用 `.capture_window()` — 单窗口捕获用 `.new_window_auto()`
- 不要使用旧式 API：`ActionManager`、`create_action_manager()`、`MiddlewareChain`、`Action` — 在 v0.12+ 中已移除
- 不要在新的 SKILL.md 顶层放置**任何** dcc-mcp-core 扩展 (v0.15+ / #356) — **此规则是架构性的，不是特定字段列表**。`tools`、`groups`、`workflows`、`prompts`、`next-tools` 行为链、`examples` 包以及任何未来扩展必须是 `metadata.dcc-mcp.<feature>` 键指向同级文件。完整理由参见"SKILL.md 同级文件模式"陷阱。旧式顶层 `dcc:`/`tags:`/`tools:`/`groups:`/`depends:`/`search-hint:` 仍可解析以保持向后兼容但会发出弃用警告并使 `is_spec_compliant()` 返回 `False`。参见 `docs/guide/skills.md#migrating-pre-015-skillmd`。
- 不要将大型载荷（workflow 规格、prompt 模板、示例对话、注解表）内联到 SKILL.md frontmatter 或 body 中，即使在 `metadata:` 下 — 使用同级文件。SKILL.md body 保持在 ≤500 行 / ≤5000 token。
- **不要创建没有 `metadata.dcc-mcp.layer` 的 skill** — 未标记的 skill 随目录增长会造成路由歧义
- **不要在 domain skill `description` 中省略"Not for X"句** — 代理需要显式反例以避免选择错误的 skill
- **不要在 infrastructure 和 domain skill 之间重叠 `search-hint` 关键词** — 重叠关键词使 `search_skills()` 返回歧义结果
- 不要使用已移除的传输 API：`FramedChannel`、`connect_ipc()`、`IpcListener`、`TransportManager`、`CircuitBreaker`、`ConnectionPool` — 在 v0.14 (#251) 中移除。改用 `IpcChannelAdapter` / `DccLinkFrame`
- 不要添加 Python 运行时依赖 — 项目设计为零依赖
- 不要手动更新版本号或编辑 `CHANGELOG.md` — Release Please 负责处理
- 不要硬编码 API 密钥、令牌或密码 — 使用环境变量
- 不要在分支名中使用 `docs/` 前缀 — 会导致 `refs/heads/docs/...` 冲突
- 不要在 `tools/call` 中硬编码旧式 `<skill>.<action>` 前缀形式 — 裸名自 v0.14.2 (#307) 起为默认
- 不要在 Python 中引用 `ActionMeta.enabled` — 使用 `ToolRegistry.set_tool_enabled()` 代替
- 不要对 `ToolResult` 使用 `json.dumps()` — 使用 `result.to_json()` 或 `serialize_result()`
- 不要猜测工具名 — 使用 `search_skills(query)` 发现正确的工具。

---

## 代码风格

### Python

- `from __future__ import annotations` — 每个模块的第一行
- 导入顺序：future → 标准库 → 第三方 → 本地（带段落注释）
- 格式化工具：`ruff format`（行长 120，双引号）
- 所有公共 API：类型注解 + Google 风格 docstring

### Rust

- Edition 2024，MSRV 1.85
- `tracing` 用于日志（不用 `println!`）
- `thiserror` 用于错误类型
- `parking_lot` 代替 `std::sync::Mutex`

---

## 工具描述编写 — 风格指南

每个内置 MCP 工具描述（参见 `build_core_tools_inner` 和
`build_lazy_action_tools` 在 `crates/dcc-mcp-http/src/handler.rs` 中）遵循
issue #341 中采用的三层行为结构：一句现在时"是什么"摘要、
一段 `When to use:` 对比同级工具的段落（让代理知道何时不选它），
以及一段 `How to use:` 列表涵盖前置条件、常见陷阱和后续工具。
整个字符串保持在 ≤500 字符（MCP 客户端会截断长文本）；
如果需要更多上下文，移至 `docs/api/http.md` 并从描述中引用锚点。
输入 schema 中每个参数的 `description` 字段为单个从句 ≤100 字符。
结构契约由 `tests/test_tool_descriptions.py` 强制执行。

---

## 添加新的公共符号 — 检查清单

当添加需要从 Python 调用的 Rust 类型/函数时：

1. 在 `crates/dcc-mcp-*/src/` 中实现
2. 在 crate 的 `python.rs` 中添加 `#[pyclass]` / `#[pymethods]` 绑定
3. 通过相应的 `register_*()` 函数在 `src/lib.rs` 中注册
4. 在 `python/dcc_mcp_core/__init__.py` 中重新导出（导入 + 添加到 `__all__`）
5. 在 `python/dcc_mcp_core/_core.pyi` 中添加类型桩
6. 在 `tests/test_<module>.py` 中添加测试
7. 运行 `vx just dev` 重新构建，然后 `vx just test`

---

## 开发环境提示

- **测试前构建**：始终在 `vx just test` 之前运行 `vx just dev` — 必须先编译 Rust 扩展。
- **PR 预检**：`vx just preflight` 运行 cargo check + clippy + fmt + test-rust — 尽早发现问题。
- **Lint 自动修复**：`vx just lint-fix` 自动修复 Rust（cargo fmt）和 Python（ruff + isort）问题。
- **版本从不手动**：Release Please 负责版本管理 — 绝不手动编辑 `CHANGELOG.md` 或版本字符串。
- **仅文档变更**：对 `docs/`、`*.md`、`llms*.txt` 的变更在 CI 中跳过 Rust 重建 — 快速周转。
- **分支命名**：避免 `docs/` 前缀（导致 `refs/heads/docs/...` 冲突）。使用平面名如 `feat-xxx` 或 `enhance-xxx`。

---

## 安全考虑

- **沙盒**：对 AI 驱动的工具执行使用 `SandboxPolicy` + `SandboxContext`。绝不暴露不受限的文件系统或进程访问。
- **输入验证**：始终在执行前使用 `ToolValidator.from_schema_json()` 验证 AI 提供的参数。
- **ToolAnnotations**：信号安全属性（`read_only_hint`、`destructive_hint`、`idempotent_hint`、`open_world_hint`、`deferred_hint`）使 AI 客户端做出知情选择。
- **SkillScope**：信任层级防止项目本地 skill 遮蔽企业管理 skill。
- **审计日志**：`AuditLog` / `AuditMiddleware` 为所有 AI 发起的工具调用提供可追溯性。
- **代码中无密钥**：绝不硬编码 API 密钥、令牌或密码。使用环境变量或仓库外的配置文件。

---

## PR 指南

- **标题格式**：使用 Conventional Commits：`feat:`、`fix:`、`docs:`、`refactor:`、`chore:`、`test:`
- **Scope 可选**：`feat(capture): add DXGI backend`
- **破坏性变更**：`feat!: rename action→tool` 并在 footer 中加 `BREAKING CHANGE: ...`
- **Squash 合并**：PR 采用 squash 合并 — 在 PR 标题中写最终提交消息。
- **CI 必须通过**：`vx just preflight` + `vx just test` + `vx just lint` 必须全部为绿色。
- **不手动更新版本**：Release Please 负责版本管理 — 绝不手动更新。

---

## 提交消息指南

- 使用 [Conventional Commits](https://www.conventionalcommits.org/)：`feat:`、`fix:`、`docs:`、`refactor:`、`chore:`、`test:`
- Scope 可选：`feat(capture): add DXGI backend`
- 破坏性变更：`feat!: rename action→tool` 并在 footer 中加 `BREAKING CHANGE: ...`
- 版本更新由 Release Please 处理 — 绝不手动编辑 `CHANGELOG.md` 或版本字符串

---

## CI 与发布

- PR 必须通过：`vx just preflight` + `vx just test` + `vx just lint`
- CI 矩阵：Python 3.7、3.9、3.11、3.13 on Linux / macOS / Windows
- 版本管理：Release Please（Conventional Commits）— 绝不手动更新
- PyPI：Trusted Publishing（无需令牌）
- 仅文档变更跳过 Rust 重建 → CI 快速通过
- PR 采用 Squash 合并约定
