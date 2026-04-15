# Skill 作用域与策略

> **[English](../../guide/skill-scopes-policies)**

Skills 拥有两种细粒度控制机制：**作用域**（信任层级）和**策略**（调用规则）。

## SkillScope：信任层级

Skills 按来源信任度分类：

```
Admin   （最高信任——企业管控）
  ↑
System  （随 dcc-mcp-core 捆绑——已验证）
  ↑
User    （~/.skills/ ——个人工作流）
  ↑
Repo    （.codex/skills/ ——项目本地，最低信任）
```

### 作用域的分配方式

作用域由 Skill 的**发现路径**决定：

```python
from dcc_mcp_core import SkillCatalog, ActionRegistry

catalog = SkillCatalog(ActionRegistry())

# 从此路径发现的 Skills 获得 scope="repo"
catalog.discover(extra_paths=[".codex/skills"])

# 在摘要中查看作用域
for skill in catalog.list_skills():
    print(f"{skill.name}: scope={skill.scope}")
```

### SkillSummary 中的作用域字段

发现后，`list_skills()` 返回 `SkillSummary` 对象，包含：

```python
summaries = catalog.list_skills()
for s in summaries:
    print(s.name)                 # "maya-geometry"
    print(s.scope)                # "repo" | "user" | "system" | "admin"
    print(s.implicit_invocation)  # True | False
```

### 为什么作用域很重要

- **企业场景**：Admin Skills 始终执行，无法被项目 Skills 覆盖
- **多项目**：System Skills 全局可用；Repo Skills 保持项目本地
- **安全性**：Repo Skills（来自克隆仓库的不可信代码）无法覆盖 System Skills

## SkillPolicy：调用控制

在 `SKILL.md` 前置元数据中声明 AI 智能体的调用方式：

```yaml
---
name: maya-cleanup
dcc: maya
policy:
  allow_implicit_invocation: false   # 需要先显式调用 load_skill
  products: ["maya", "houdini"]      # 仅在这些 DCC 中可见
---
```

### allow_implicit_invocation

控制 AI 是否可以直接从 `tools/list` 调用该技能。

| 值 | 行为 |
|----|------|
| `true`（默认） | 工具出现在 `tools/list` 中，可直接调用 |
| `false` | 工具**隐藏**，直到客户端显式调用 `load_skill(name)` |

**适合设置为 `false` 的场景：**
- 破坏性操作（`delete_all_nodes`、`reset_scene`）
- 高成本工具（完整渲染、模拟烘焙）
- 需要用户确认的工具

```python
from dcc_mcp_core import SkillMetadata
import json

md = SkillMetadata("secure-tool")
md.policy = json.dumps({"allow_implicit_invocation": False})

# 检查：
md.is_implicit_invocation_allowed()  # → False
```

### products：产品过滤器

将技能可见性限制到特定 DCC 应用：

```yaml
policy:
  products: ["maya"]             # 仅在 Maya 会话中
  # products: ["maya", "houdini"]  # Maya 和 Houdini 都有
  # products: []                   # 所有 DCC（策略缺失时的默认行为）
```

这可以防止 Maya MEL 脚本出现在 Blender 会话中。

```python
md = SkillMetadata("maya-mel-tool")
md.policy = json.dumps({"products": ["maya"]})

md.matches_product("maya")    # → True
md.matches_product("blender") # → False
md.matches_product("houdini") # → False
```

**产品匹配不区分大小写：**
```python
md.matches_product("MAYA")    # → True
md.matches_product("Maya")    # → True
```

## SkillDependencies：外部依赖声明

声明技能执行前需要的外部资源：

```yaml
---
name: usd-validator
external_deps:
  tools:
    - type: mcp
      value: "pixar-usd"
      description: "USD 验证 MCP 服务器"
      transport: "ipc"
    - type: env_var
      value: "PYTHONPATH"
      description: "必须包含 USD site-packages"
    - type: bin
      value: "usdview"
      description: "USD 检查工具"
---
```

### 依赖类型

| 类型 | `value` 字段 | 用途 |
|------|------------|------|
| `mcp` | MCP 服务器名称 | 需要运行中的 MCP 服务 |
| `env_var` | 变量名称 | 需要设置环境变量 |
| `bin` | 可执行文件名 | 需要 PATH 中存在该二进制文件 |

### Python API

```python
from dcc_mcp_core import SkillMetadata
import json

md = SkillMetadata("usd-validator")

# 通过 JSON 设置依赖
deps = {
    "tools": [
        {"type": "env_var", "value": "PYTHONPATH"},
        {"type": "bin", "value": "usdview"},
    ]
}
md.external_deps = json.dumps(deps)

# 读取回来
print(md.external_deps)  # JSON 字符串或 None
```

## 完整示例

```yaml
---
name: maya-scene-publisher
version: "2.0.0"
description: "包含验证的生产场景发布工具"
dcc: maya
scope: repo           # 项目本地技能

policy:
  allow_implicit_invocation: false   # 用户必须显式加载
  products: ["maya"]                  # 仅限 Maya

external_deps:
  tools:
    - type: env_var
      value: "PIPELINE_ROOT"
      description: "Pipeline 根目录"
    - type: mcp
      value: "asset-tracker"
      description: "资产追踪 MCP 服务"
    - type: bin
      value: "mayapy"
      description: "Maya Python 解释器"
---

# Maya 场景发布器

验证并将场景发布到生产管线。
```

加载此技能时：

1. 🔒 **作用域**：`repo` — 可被 User/System 技能覆盖
2. 🔐 **策略**：`allow_implicit_invocation: false` — 需要显式调用 `load_skill`
3. 🎯 **产品**：仅在 Maya 会话中可见；在 Blender/Houdini 中隐藏
4. 📋 **依赖**：首次调用前验证 `PIPELINE_ROOT`、`asset-tracker`、`mayapy`
