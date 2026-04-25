# Recipes API

Recipes system for dcc-mcp-core skills (issue #428). Formalizes the `metadata.dcc-mcp.recipes` sibling-file key per the #356 sibling-file pattern.

**Exported symbols:** `get_recipe_content`, `get_recipes_path`, `parse_recipe_anchors`, `register_recipes_tools`

## get_recipes_path

```python
get_recipes_path(metadata: Any) -> str | None
```

Extract the recipes file path from a `SkillMetadata` object. Supports both flat (`"dcc-mcp.recipes"`) and nested (`"dcc-mcp": {"recipes": ...}`) forms. Returns absolute path resolved relative to skill's `skill_path`.

## parse_recipe_anchors

```python
parse_recipe_anchors(recipes_path: str) -> list[str]
```

Return the list of anchor names from a RECIPES.md file. Anchors are `##` headings.

## get_recipe_content

```python
get_recipe_content(recipes_path: str, anchor: str) -> str | None
```

Return the Markdown content of a specific anchor section (from `## <anchor>` up to the next `##` heading).

## register_recipes_tools

```python
register_recipes_tools(server, *, skills: list, dcc_name="dcc") -> None
```

Register `recipes__list` and `recipes__get` MCP tools on `server`. Call **before** `server.start()`.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig, scan_and_load
from dcc_mcp_core.recipes import register_recipes_tools

loaded, _ = scan_and_load(dcc_name="maya")
server = create_skill_server("maya", McpHttpConfig(port=8765))
register_recipes_tools(server, skills=loaded, dcc_name="maya")
handle = server.start()
```
