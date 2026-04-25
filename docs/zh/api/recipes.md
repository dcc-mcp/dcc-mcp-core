# 配方 API

> **[English](../api/recipes.md)**

Skill 配方锚点查找工具。薄线束 skill 的 `references/RECIPES.md` 文件包含可搜索的代码片段锚点，此模块提供配方列表和内容获取的 MCP 工具注册。

**导出符号：** `get_recipes_path`, `parse_recipe_anchors`, `get_recipe_content`, `register_recipes_tools`

## 主要函数

- `get_recipes_path(metadata) -> str | None` — 从 SkillMetadata 中提取配方文件路径
- `parse_recipe_anchors(recipes_path) -> list[str]` — 列出 RECIPES.md 文件中的锚点名
- `get_recipe_content(recipes_path, anchor) -> str | None` — 获取指定锚点段的内容
- `register_recipes_tools(server, *, skills, dcc_name="dcc")` — 注册 `recipes__list` 和 `recipes__get` MCP 工具

详见 [English API 参考](../api/recipes.md)。
