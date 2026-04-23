# 能力与工作区根目录

> **Issue:** [#354](https://github.com/loonghao/dcc-mcp-core/issues/354) —
> 能力声明 + 类型化工作区路径握手
>
> **状态:** 自 v0.15 起可用

本文档涵盖两个松耦合的功能，它们使 DCC 工具更安全、
跨主机更可移植：

1. **能力声明** — 工具声明需要哪些 DCC 功能；适配器声明
   主机能提供什么。服务器会阻止未满足要求的工具调用。
2. **类型化工作区路径握手** — 工具可以使用 `workspace://` URI scheme，
   服务器会针对 MCP 客户端通告的文件系统根目录解析它。

---

## 1. 能力声明

### 为什么

并非每个 DCC 都暴露相同的功能面。Maya 有 USD；3ds Max 没有。
某些适配器在无头模式下运行，没有文件系统访问权限；其他则有
完整的写权限。声明能力让运行时在 Python 脚本运行之前**
拒绝工具调用并返回格式良好的 MCP 错误。

### 每个工具: `tools.yaml` 中的 `required_capabilities`

根据 **issue #356**，工具声明存放在从 `SKILL.md` 通过
`metadata.dcc-mcp.tools` 引用的兄弟 `tools.yaml` 文件中。
向任何需要非平凡主机功能的工具添加 `required_capabilities`：

```yaml
# tools.yaml
tools:
  - name: import_usd
    description: 将 USD stage 导入场景
    required_capabilities: [usd, scene.mutate, filesystem.read]

  - name: read_stage_metadata
    description: 从 USD stage 读取元数据而不修改场景
    required_capabilities: [usd, scene.read, filesystem.read]

  - name: ping
    description: 不需要任何能力
```

能力字符串是自由格式的 — 将其视为技能作者和适配器作者之间的
约定。捆绑技能使用的常见命名空间：

| 命名空间 | 含义 |
|----------|------|
| `usd` | USD stage / layer 操作可用 |
| `scene.read` | 读取当前 DCC 场景图 |
| `scene.mutate` | 修改当前 DCC 场景图 |
| `filesystem.read` | 从磁盘读取文件 |
| `filesystem.write` | 向磁盘写入文件 |
| `viewport` | 渲染 / 截图活动视口 |

### 每个技能: 通过 `SkillMetadata.required_capabilities()` 聚合

加载器会自动对技能上所有按工具的能力取并集：

```python
from dcc_mcp_core import SkillMetadata, scan_and_load

skills, _ = scan_and_load(dcc_name="maya")
for md in skills:
    print(md.name, md.required_capabilities)  # 排序去重并集
```

这对 `search_skills` 过滤以及通过 `SKILL.md` 概览向 AI agent
展示很有用。

### 主机端: `McpHttpConfig.declared_capabilities`

DCC 适配器在启动服务器时声明当前主机能提供什么：

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

cfg = McpHttpConfig(port=8765)
cfg.declared_capabilities = [
    "usd",
    "scene.read",
    "scene.mutate",
    "filesystem.read",
    # 为只读会话故意省略 filesystem.write
]
server = create_skill_server("maya", cfg)
handle = server.start()
```

### 运行时行为

**`tools/list`** — 每个工具都会被列出，无论能力如何，但未满足的工具
会携带一个 `_meta` 提示，以便 AI 客户端可以跳过它们：

```jsonc
{
  "name": "import_usd",
  "description": "...",
  "inputSchema": { "...": "..." },
  "_meta": {
    "dcc": {
      "required_capabilities": ["usd", "scene.mutate", "filesystem.read"],
      "missing_capabilities": ["filesystem.write"]  // 仅当非空时
    }
  }
}
```

**`tools/call`** — 服务器会以结构化的 JSON-RPC 错误拒绝调用：

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32001,
    "message": "capability_missing: tool 'import_usd' requires filesystem.write",
    "data": {
      "tool": "import_usd",
      "required": ["usd", "scene.mutate", "filesystem.write"],
      "missing": ["filesystem.write"],
      "declared": ["usd", "scene.read", "scene.mutate", "filesystem.read"]
    }
  }
}
```

错误码 `-32001` 是 dcc-mcp-core 的 `CAPABILITY_MISSING`。AI 客户端应将
此视为对当前会话**永久**失败，而不是重试。

---

## 2. 类型化工作区路径握手

### 为什么

MCP 客户端通过 `initialize` 请求的 `roots` 能力通告文件系统根目录
（`file:///home/user/project/...`）。历史上接受路径的工具必须：

- 信任 AI 传递绝对路径（有风险 — 会逃逸工作区），
- 或接受原始字符串并重新实现根解析（样板代码）。

`WorkspaceRoots` 辅助类集中处理此问题。工具接受 `workspace://`
URI scheme，服务器针对会话的第一个根目录解析它。

### 从工具使用 `WorkspaceRoots`

`WorkspaceRoots` 作为 Python 类暴露。当工具声明了 `filesystem.*`
能力时，服务器会向工具上下文注入一个 `_workspace_roots` 参数：

```python
def import_usd(path: str, _workspace_roots=None):
    if _workspace_roots is None:
        return error_result("import_usd", "no workspace roots advertised")
    try:
        resolved = _workspace_roots.resolve(path)
    except ValueError as e:
        return error_result("import_usd", str(e))
    # ...继续使用 `resolved` 作为绝对 PathBuf 等效物
```

### 解析规则

| 输入 | 行为 |
|------|------|
| `workspace://assets/hero.usd` | 与第一个通告的根目录拼接 |
| `/abs/path/scene.ma` | 原样返回 |
| `C:\Users\me\scene.max` | 原样返回（Windows 绝对路径） |
| `assets/hero.usd`（相对） | 如果可用，与第一个根拼接；否则原样返回 |
| `workspace://...` 但没有根 | 抛出 `no workspace roots`（MCP 错误 `-32602`） |

### 手动构造（用于测试）

```python
from dcc_mcp_core import WorkspaceRoots

roots = WorkspaceRoots(["/projects/hero"])
assert roots.resolve("workspace://char/bob.usd") == "/projects/hero/char/bob.usd"
assert roots.resolve("/tmp/abs").endswith("abs")
```

### Rust API

```rust
use dcc_mcp_http::{WorkspaceRoots, WorkspaceResolveError};

let roots = WorkspaceRoots::from_client_roots(&session.roots());
let path = roots.resolve("workspace://assets/hero.usd")?;
// path 是绝对 std::path::PathBuf
```

`WorkspaceResolveError::NoRoots` 映射到 JSON-RPC 错误码 `-32602`
(`NO_WORKSPACE_ROOTS`)。

---

## 参见

- [Skills guide](skills.md) — `tools.yaml` 兄弟文件模式 (#356)
- [`docs/guide/naming.md`](naming.md) — SEP-986 工具名验证
- MCP roots 规范: <https://modelcontextprotocol.io/specification/2025-03-26/client/roots>
