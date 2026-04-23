# MCP Prompts 原语

> 为 dcc-mcp-core 实现 [MCP 2025-03-26 — Prompts](https://modelcontextprotocol.io/specification/2025-03-26/server/prompts)。
> Issues [#351](https://github.com/loonghao/dcc-mcp-core/issues/351)
> 和 [#355](https://github.com/loonghao/dcc-mcp-core/issues/355)。

**Prompts** 原语是 `McpHttpServer` 通告的第三个 MCP 面，
与 `tools` 和 `resources` 并列。它使 AI 客户端能够发现可重用的
**提示模板** — 由参数化的自然语言指令组成 — 技能作者可以
精心制作以从模型中引出正确的行为链。

与 `tools/call`（执行副作用）和 `resources/read`（返回不透明字节）
不同，`prompts/get` 返回一个**渲染后的消息数组**，客户端可以直接
拼接到对话中，保留技能作者的意图，而不需要模型猜测正确的措辞。

## 线协议

启用后，服务器在 `initialize` 中通告该原语：

```json
{
  "capabilities": {
    "prompts": { "listChanged": true }
  }
}
```

暴露三个 JSON-RPC 方法：

| 方法 | 目的 |
|------|------|
| `prompts/list` | 返回每个已注册的 prompt — 名称、描述、参数 schema。 |
| `prompts/get` | 通过名称渲染一个 prompt，使用调用者提供的参数。 |
| `notifications/prompts/list_changed` | 每当技能被加载 / 卸载时服务器推送的 SSE 事件。 |

## 来源: 兄弟 `prompts.yaml`

遵循项目范围的**兄弟文件规则** ([#356](https://github.com/loonghao/dcc-mcp-core/issues/356))，
提示模板从不内联到 `SKILL.md` 中。技能作者从 `metadata.dcc-mcp`
命名空间引用一个兄弟文件：

```yaml
---
name: maya-geometry
description: "Maya geometry primitives and editing."
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.prompts: prompts.yaml     # 单文件，或
  # dcc-mcp.prompts: prompts/*.prompt.yaml   # glob，每个 prompt 一个文件
---
```

`prompts.yaml` 包含两个顶层列表 — 都是可选的：

::: v-pre
```yaml
prompts:
  - name: bevel_all_edges
    description: "以一致的倒角宽度对每条选中的边进行倒角。"
    arguments:
      - name: chamfer_width
        description: "Maya 单位中的倒角宽度。"
        required: true
      - name: segments
        description: "倒角段数（默认 2）。"
        required: false
    template: |
      使用 `maya_geometry__select_edges` 捕获当前选择，
      然后调用 `maya_geometry__bevel_edges`，参数为
      width={{chamfer_width}} 和 segments={{segments}}。
      在保存前使用 `diagnostics__screenshot` 验证结果。

workflows:
  - file: workflows/bake_proxies.workflow.yaml
    prompt_name: bake_proxies_summary      # 可选重命名
```
:::

### 显式 prompts

`prompts:` 下的每个条目都是一个 `PromptSpec`：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | ✅ | 技能内唯一。作为 MCP 服务时使用完全限定名。 |
| `description` | string | ✅ | 客户端向用户展示的一行摘要。 |
| `arguments` | list[ArgumentSpec] | ❌ | 模板的类型化占位符。 |
| `template` | string | ✅ | <code v-pre>`{{name}}`</code> 占位符针对调用点参数解析。 |

`ArgumentSpec` 字段: `name`, `description`, `required`（默认 `false`）。

### 工作流派生 prompts

`workflows:` 列表为每个引用的工作流自动生成一个摘要 prompt。
这是一个最小的行为链提示 — "这是工作流按顺序运行的步骤" —
适合想要在执行前（或代替执行）叙述工作流的 agent：

```yaml
workflows:
  - file: workflows/bake_proxies.workflow.yaml
```

不需要 `template`；注册表将工作流的描述 + 步骤列表汇总为
一个 user-role 消息。使用 `prompt_name` 覆盖默认自动生成的名称。

### 单文件 vs glob

`metadata.dcc-mcp.prompts` 接受两种形式：

- `prompts.yaml` — 单文件，包含 `prompts:` + `workflows:` 列表。
- `prompts/*.prompt.yaml` — glob，每个 prompt 一个文件。每个文件的
  形状与 `prompts:` 中的单个条目相同。

解析是**惰性**的：路径在扫描 / 加载时记录；文件内容仅在服务器
处理 `prompts/list` 或 `prompts/get` 时才读取。

## 模板引擎

渲染引擎故意保持极简 — 只有一个 token：
::: v-pre
`{{placeholder}}`。
:::

- 大括号内的空白会被修剪：<code v-pre>`{{ foo }}`</code> == <code v-pre>`{{foo}}`</code>。
- 未声明的 required 参数会抛出
  `INVALID_PARAMS: missing required argument: <name>`。
- 不是裸标识符的大括号内容（<code v-pre>`{{ 1 + 1 }}`</code>）会原样保留 —
  引擎从不求值表达式。
- 没有匹配 `}}` 的未闭合 <code v-pre>`{{`</code> 会原样输出。

保持模板小而声明式。如果模板需要循环、条件或数据获取，
请将其编写为**工作流**（issue #348）并从 `workflows:` 列表中引用。

## 服务器配置

该原语**默认启用**。通过 `McpHttpConfig.enable_prompts = false`
全局禁用：

```python
from dcc_mcp_core import McpHttpConfig, create_skill_server

cfg = McpHttpConfig(port=8765)
cfg.enable_prompts = False     # opt out — capability 从 initialize 中消失
server = create_skill_server("maya", cfg)
server.start()
```

禁用时，服务器会省略 `prompts` 能力并以 `Method not found`
拒绝 `prompts/list` / `prompts/get`。

## `list_changed` 不变式

- `notifications/prompts/list_changed` 在已加载技能集发生变化时触发
  (`skills/load`, `skills/unload`, 热重载)。
- 注册表的内部缓存在发出通知的同一个临界区失效 — 客户端可以
  立即在其后调用 `prompts/list` 并观察到新集合。
- 通知扇出到每个活跃的 SSE 会话；prompts 没有每会话订阅模型。

## 技能作者指南

1. 保持 `template` 正文**在 50 行以内**。更长的指南应放在 `references/`
   中，并由工作流层拉取。
2. 在模板内部优先使用**完全限定工具名**
   (`maya_geometry__bevel_edges`) — 这样 agent 就不需要猜测。
3. 如果行为链超过 3-4 个工具调用，将其提取到 `workflow.yaml` 中
   并让服务器自动生成摘要 prompt。
4. 不要在模板中放入 secrets 或环境特定路径 — prompts 会原样
   暴露给模型。

## 相关 issues

- [#351](https://github.com/loonghao/dcc-mcp-core/issues/351) — MCP prompts 原语
- [#355](https://github.com/loonghao/dcc-mcp-core/issues/355) — 从 SKILL.md 示例 + 工作流派生的 prompts
- [#348](https://github.com/loonghao/dcc-mcp-core/issues/348) — 工作流 specs（自动派生 prompts 的来源）
- [#356](https://github.com/loonghao/dcc-mcp-core/issues/356) — 兄弟文件模式
- [#350](https://github.com/loonghao/dcc-mcp-core/issues/350) — MCP resources 原语（姐妹功能）
