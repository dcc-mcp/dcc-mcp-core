# 薄线束 Skill 编写模式

**[English](../../guide/thin-harness.md)**

> **TL;DR** — 当没有 domain skill 覆盖用户的意图时，薄线束 skill
> 向代理提供一个原始脚本执行器加一本配方书。代理阅读配方，
> 编写原生 DCC 调用，然后提交 — 无需包装器。
> 架构原理参见 [ADR 003](/adr/003-thin-harness-skill-pattern)。

---

## 何时写包装器 vs. 薄线束

| 信号 | 使用方案 |
|------|----------|
| 操作是 2–5 个原生 API 调用，在训练数据中有良好文档 | **薄线束** — 提供 `execute_python` + 配方 |
| 操作需要多步骤流水线逻辑（渲染农场、镜头导出） | **Domain skill** — 显式 schema + 错误处理 |
| 操作需要在执行前进行安全验证 | **Domain skill** — `ToolValidator` + `SandboxPolicy` |
| 你在一对一包装 `maya.cmds`、`bpy.ops` 或 `hou.*` | **薄线束** — 代理已经知道这些 API |
| 你需要跨多个 DCC 状态变更的 `next-tools` 链式调用 | **Domain skill** — 显式声明链 |

**经验法则**：如果 LLM 训练语料中包含 10,000+ 个原生调用示例，
就写薄线束。如果操作是专有流水线逻辑，就写 domain skill。

---

## Skill 层级值

```yaml
# SKILL.md metadata
metadata:
  dcc-mcp:
    layer: thin-harness   # ← 新值，与 infrastructure / domain / example 并列
```

路由：代理在搜索 domain skill 之后将薄线束 skill 作为**兜底**加载。
如果 `search_skills(query)` 未返回 domain 匹配，代理加载 DCC 的薄线束
skill 并查阅 `references/RECIPES.md`。

---

## 薄线束 Skill 结构

```
my-dcc-scripting/
├── SKILL.md                      # 简短，layer: thin-harness
├── tools.yaml                    # execute_python + 可选组
├── scripts/
│   └── execute.py                # 原始脚本运行器
└── references/
    ├── RECIPES.md                # ~20 个可复制粘贴的代码片段
    └── INTROSPECTION.md          # 如何查询实时 DCC 命名空间
```

### SKILL.md

```yaml
---
name: maya-scripting
description: >-
  Thin-harness skill — raw Maya Python script execution with recipes.
  Use when no domain skill covers the operation and the agent knows the
  maya.cmds / OpenMaya API. Not for pipeline-level intent — use
  maya-pipeline domain skills for shot export, render farm, etc.
license: MIT
metadata:
  dcc-mcp:
    dcc: maya
    layer: thin-harness
    tools: tools.yaml
    recipes: references/RECIPES.md
    introspection: references/INTROSPECTION.md
---

Execute arbitrary Python inside the live Maya session.

## When to use this skill

- The user wants to call a specific `maya.cmds.*` function directly.
- No domain skill covers the operation.
- The user wants to inspect or iterate on raw DCC API calls.

## When NOT to use this skill

- Shot export → use `maya-pipeline__export_shot`
- Render farm submission → use `maya-render__submit`
- Any operation with multi-step error recovery → use a domain skill

## Checklist before calling execute_python

1. Check `references/RECIPES.md` for a working snippet.
2. If no recipe matches, call `dcc_introspect__search` to find the right symbol.
3. Submit the script. On error, read `_meta.dcc.raw_trace` for the failing call.
```

### tools.yaml

```yaml
tools:
  - name: execute_python
    description: >-
      Execute a Python script string inside the live DCC interpreter.
      When to use: when no domain skill covers the operation and you have
      a working maya.cmds / bpy / hou snippet. How to use: pass the full
      script as 'code'; check references/RECIPES.md first.
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: false
    next-tools:
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

### scripts/execute.py

```python
from __future__ import annotations

from dcc_mcp_core import skill_entry, skill_success, skill_error


@skill_entry
def execute_python(code: str, timeout_secs: int = 30) -> dict:
    """Execute a Python script string in the live DCC interpreter.

    Args:
        code: Python source to execute.
        timeout_secs: Execution timeout. Default 30 s.

    Returns:
        skill_success with 'output' key on success, skill_error on failure.
    """
    import traceback

    local_ns: dict = {}
    try:
        exec(compile(code, "<execute_python>", "exec"), {}, local_ns)  # noqa: S102
        output = local_ns.get("result", None)
        return skill_success("Script executed", output=output)
    except Exception as exc:  # noqa: BLE001
        return skill_error(
            f"Script raised {type(exc).__name__}: {exc}",
            underlying_call=code[:200],
            traceback=traceback.format_exc(),
        )
```

---

## references/RECIPES.md 约定

一个带有 `##` 锚点段的扁平 Markdown 文件。每个段：
- 一句话描述何时使用该配方。
- 一个可直接运行的 Python 片段（≤15 行）。
- 无样板导入 — 假定 `import maya.cmds as cmds` 等已在作用域内。

```markdown
## create_polygon_cube

Create a named polygon cube at the origin.

\`\`\`python
cube = cmds.polyCube(name="myCube", w=1, h=1, d=1)[0]
\`\`\`

## set_world_translation

Set absolute world-space translation (not relative).

\`\`\`python
cmds.xform("myCube", translation=(1, 2, 3), worldSpace=True)
\`\`\`
```

配方锚点名将在 issue #428 落地后通过 `recipes__get(skill=..., anchor=...)` 可搜索。

---

## references/INTROSPECTION.md 约定

解释代理如何在不阅读供应商文档的情况下发现实时 DCC 命名空间。

```markdown
## List a module's public names

\`\`\`python
import maya.cmds as cmds
result = [n for n in dir(cmds) if not n.startswith("_")]
\`\`\`

## Get a command's flags

\`\`\`python
help(cmds.polyCube)
\`\`\`

## Use dcc_introspect__* tools (issue #426)

Once the dcc-introspect built-in skill is loaded:
- ``dcc_introspect__list_module(module="maya.cmds")``
- ``dcc_introspect__signature(qualname="maya.cmds.polyCube")``
- ``dcc_introspect__search(pattern="poly.*", module="maya.cmds")``
```

---

## AGENTS.md 中的路由

添加到 `AGENTS.md` Do 列表（另见 ADR 003）：

> **如果没有 domain skill 匹配用户的意图**，加载 DCC 的 `*-scripting`
> （薄线束）skill 并在发明调用之前阅读 `references/RECIPES.md`。
> 仅在没有配方匹配时才回退到原始 `execute_python`。

---

## 错误信封集成 (issue #427)

当薄线束 `execute_python` 调用抛出异常时，`_meta.dcc.raw_trace` 块
（当 `McpHttpConfig.enable_error_raw_trace = True` 时）为代理提供：

```jsonc
{
  "_meta": {
    "dcc.raw_trace": {
      "underlying_call": "cmds.polySphere(name='mySphere', radius=-1.0)",
      "traceback": "...",
      "recipe_hint": "references/RECIPES.md#create_sphere",
      "introspect_hint": "dcc_introspect__signature(qualname='maya.cmds.polySphere')"
    }
  }
}
```

代理读取跟踪，修正调用，然后重新提交 — 无需请求新的包装器工具。

---

## 相关

- [ADR 003](/adr/003-thin-harness-skill-pattern) — 架构决策
- [skills/templates/thin-harness/](https://github.com/loonghao/dcc-mcp-core/tree/main/skills/templates/thin-harness/) — 起始模板
- [skills/README.md#skill-layering](https://github.com/loonghao/dcc-mcp-core/blob/main/skills/README.md) — 层级定义
- Issue #426 — `dcc_introspect__*` 内置工具
- Issue #427 — `_meta.dcc.raw_trace` 错误信封
- Issue #428 — `metadata.dcc-mcp.recipes` 形式化
