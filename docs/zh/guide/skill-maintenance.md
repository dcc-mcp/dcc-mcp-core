# 技能包维护（DCC 适配器 + 捆绑核心技能）

本指南是 dcc-mcp-core 和下游适配器（例如 dcc-mcp-maya）中 SKILL 包的**单一维护契约**。仓库内的**参考实现**位于：

- `python/dcc_mcp_core/skills/dcc-diagnostics/` — 基础设施技能：丰富的 frontmatter `description`、`search-hint`、`layer`，工具目的在 SKILL.md 正文中明确说明。
- `python/dcc_mcp_core/skills/media/` — 基础设施技能：类型化的 vx 管理的 FFmpeg/FFprobe 包装器，用于 DCC 渲染/播放blast 工件，而不暴露任意 shell 或 vx 执行。
- `python/dcc_mcp_core/skills/workflow/` — 编排技能：示例 JSON 链和明确的"何时不使用"边界。

在创作或审查任何新技能时，请使用这两个树。

## 实现前的所有权

在添加或更改捆绑适配器技能之前，请阅读 [`docs/POLICY_SKILL_OWNERSHIP.md`](../POLICY_SKILL_OWNERSHIP.md) 和相关适配器的 `SKILL_OWNERSHIP.yml`（如果存在）。

- 常见文件操作（`open`、`save`、`import`、`export`、`read_file`、`write_file`、路径探测）必须为每个适配器有一个主要拥有技能包。
- 不要仅仅为了提高可发现性而将文件操作工具复制到第二个技能中；相反，添加别名、搜索提示、配方或指向主要所有者的 `next-tools`。
- 如果重复不可避免，请在同一 PR 中的 `SKILL_OWNERSHIP.yml` 中记录理由和所有者。

## Frontmatter（SKILL.md）

- 保持 `description` 作为**主要代理面向契约**（MCP `get_skill_info` / 搜索摘要不提供 Markdown 正文）。
- 在 `metadata.dcc-mcp` 下：始终根据适配器策略设置 `tools`、`layer`、`dcc`、`version`、`search-hint`、`tags`。
- 可选但推荐用于长篇注释：
  - `recipes:` — 基于锚点的代码片段（当宿主注册 `recipes__*` 工具时）。
  - `skill-reference-docs:` — 相对于技能根目录的 **glob 列表**，以便 `skill_refs__list` / `skill_refs__read` 可以在 `references/`（或其他目录）下提供任意 Markdown/文本，而无需硬编码一个文件名。
- 旧版 `introspection:` 单路径仍被 `skill-reference-docs` 解析支持；对于新包，请优先使用 `skill-reference-docs`。

## tools.yaml

- 每个工具**必须**声明 `execution`、`affinity` 和现实的 `timeout_hint_secs`（当 `execution: async` 时）。
- **导入/导出/保存/路径**：描述应说明**绝对路径与工作区相对路径**、所需插件和常见故障后续步骤（例如父目录必须存在、使用 `file_exists`、导出前保存场景）。简短描述使网关 `describe_tool` 无用 — 目标是提供足够的文本，使代理无需猜测即可成功。

## Python（或其他）脚本

- 尽早验证输入；返回带有 `possible_solutions` 的 `skill_error` / `ToolResult` 风格信封，而不是让 Maya 打开模态对话框。
- 对于写操作：确保父目录存在或返回结构化错误。
- 导出后：在可行时验证输出文件存在且非零大小。

## 代码检查（dcc-mcp-maya）

从 Maya 适配器仓库运行：

```bash
python tools/lint_skills.py
```

规则包括 IO 描述长度提示和 `references/` 元数据覆盖率。当您添加新的横切关注点约定时，请扩展 `tools/lint_skills.py`。

## 网关面向代理

- 优先使用网关 MCP `search` → `describe`，然后使用 REST `/v1/call`（或每宿主 `load_skill` 然后使用类型化工具）。长篇散文应放在 `recipes` / `skill-reference-docs` 中，而不仅仅是 frontmatter 下的 SKILL.md 正文中。

## 适配器启动注册

公开可选元数据驱动工具的适配器应使用 `register_metadata_driven_tools(...)`，而不是复制扫描/导入/注册包装器。当未提供 `skills` 时，辅助工具使用 `scan_and_load_lenient(...)` 进行扫描，注册默认的 `recipes` 和 `skill-reference-docs` 扩展，并返回带有每个扩展 `registered`、`failed` 或 `skipped` 状态的报告。

```python
from dcc_mcp_core import register_metadata_driven_tools

report = register_metadata_driven_tools(
    server,
    dcc_name="maya",
    extra_paths=[studio_skill_root],
)
logger.info("metadata tools: %s", report.to_dict())
```

当适配器已为服务器启动扫描了技能根目录时，传递显式 `skills=loaded_skills`；这使发现保持单次通过，同时保留相同的可选扩展错误策略。
