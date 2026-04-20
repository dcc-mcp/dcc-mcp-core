# 命名你的 Actions 和 Tools

> **状态**：强制规范。每个 DCC-MCP crate、Python wheel 和 skill 作者
> 必须选择能通过
> [`dcc_mcp_core::naming`](https://github.com/loonghao/dcc-mcp-core/tree/main/crates/dcc-mcp-naming)
> 中两个验证器的名称。
> 相关规范：[MCP `draft/server/tools#tool-names`](https://modelcontextprotocol.io/specification/draft/server/tools#tool-names)、
> [SEP-986](https://github.com/modelcontextprotocol/modelcontextprotocol/issues/986)。

生态系统中存在**两套**命名规则。在动手之前，先搞清楚你写的字符串适用哪一套。

| 概念 | 用途 | 谁看到 | 验证器 | 正则 |
|------|------|--------|--------|------|
| **Tool name** | MCP 线上可见字符串，发布在 `tools/list` 中 | LLM / MCP 客户端 | `validate_tool_name` | `^[A-Za-z0-9](?:[A-Za-z0-9_.\-]{0,47})$` |
| **Action id** | 内部稳定标识符，宿主用来路由 `tools/call` | Rust/Python 代码，手写注册 | `validate_action_id` | `^[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*$` |

## 为什么有两套规则？

MCP 规范对 tool name 很宽松：混合大小写、连字符、点号、最长 128 字符。线上传输没问题，但作为内部标识符很糟糕——连字符与 Python 属性名冲突，混合大小写助长拼写错误，128 字符太长不便阅读。

因此 `dcc-mcp-core` 保持两层：

1. **Tool names** 遵循规范，但上限 48 字符，为网关前缀（`{id8}/`、`{skill}.`）留出空间。
2. **Action ids** 更严格：点分、小写、snake_case 段。你手写这些 ID；库在发布时将它们转换为 tool names。

## 使用验证器

### Rust

```rust
use dcc_mcp_naming::{validate_tool_name, validate_action_id};

validate_tool_name("geometry.create_sphere")?;
validate_action_id("scene.get_info")?;
```

两个函数均为 `O(n)`、无内存分配，返回结构化的
[`NamingError`](https://docs.rs/dcc-mcp-naming)，指向第一个违规位置。

### Python

```python
from dcc_mcp_core import (
    TOOL_NAME_RE,
    ACTION_ID_RE,
    MAX_TOOL_NAME_LEN,
    validate_tool_name,
    validate_action_id,
)

validate_tool_name("hello-world.greet")        # 通过
validate_action_id("scene.get_info")           # 通过

validate_tool_name("bad/name")                 # 抛出 ValueError
validate_action_id("Scene.Get")                # 抛出 ValueError（大写）
```

正则常量（`TOOL_NAME_RE`、`ACTION_ID_RE`）导出供下游工具使用——schema 生成器、lint 规则、文档——它们需要引用模式而不调用 Rust。**验证器仍然是权威检查**：优先使用 `validate_tool_name()` 而不是在你的代码中重新实现正则。

## 速查表

### 合法的 tool names

```
create_sphere
geometry.create_sphere
scene.object.transform
hello-world.greet
CamelCaseTool          # MCP 允许混合大小写
0              # 单个 ASCII 字母数字也合法
```

### 非法的 tool names

| 输入 | 原因 |
|------|------|
| `""` | 空字符串 |
| `_leading` | 前导 `_` 不是 ASCII 字母数字 |
| `.tool` / `-tool` | 前导 `.` / `-` |
| `tool/call` | `/` 保留给网关前缀 |
| `tool name` / `tool,other` / `tool@host` / `tool+v2` | `[_.-]` 之外的标点 |
| `a * 49` | 超过 `MAX_TOOL_NAME_LEN = 48` |
| `工具` / `tôol` | 非 ASCII |

### 合法的 action ids

```
scene
create_sphere
scene.get_info
maya.geometry.create_sphere
v2.create
```

### 非法的 action ids

| 输入 | 原因 |
|------|------|
| `""` | 空字符串 |
| `Scene.get` / `scene.Get` | 大写 |
| `1scene.get` | 前导数字 |
| `scene..get` / `.scene` / `scene.` | 空的 `.` 分隔段 |
| `scene-get` | action id 中不允许 `-`（用 `_`） |
| `scene/get` | 不允许 `/` |

## 上限与理由

* **`MAX_TOOL_NAME_LEN = 48`** — MCP 规范允许 128，我们限制在 48，这样网关可以安全地前置 `{id8}/`（9 字符）或 skill 可以前置 `{skill}.` 而不会超出规范上限。
* **更严格的 action-id 语法** — 保持手写标识符与 Python 属性约定一致（小写、snake_case、点分命名空间），消除在审计日志、遥测和 IPC 负载中序列化 action id 时的歧义。

## 何时调用验证器

* **宿主作者** — 在**注册时**调用 `validate_action_id`，而不是在调度时。接受错误 id 的注册是 bug 温床。
* **服务器作者** — 在 `tools/list` 中发布 tool 之前调用 `validate_tool_name`，包括 skill 派生的 tool（其名称由 skill slug + tool slug 组成）。
* **Skill 作者** — 无需显式调用；库在加载 skill 时验证你的 tool name。无效名称会导致 skill 加载失败并显示可读的错误信息。

## 从自定义规则迁移

早期代码路径偶尔重新发明了这些规则（子串检查、临时正则）。当你修改此类代码时，替换为验证器：

```diff
- if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
-     return Err("bad tool name");
- }
+ dcc_mcp_naming::validate_tool_name(name)?;
```

目标是**一条规则，一个实现**——不要在随机文件中写 `name.len() > 100`，不要在 crate 之间出现"我觉得应该允许连字符"的分歧。
