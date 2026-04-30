# Recipes API

Recipes system for dcc-mcp-core skills (issues #428, #616). Formalizes the `metadata.dcc-mcp.recipes` sibling-file key per the #356 sibling-file pattern and supports both legacy Markdown anchors and structured YAML recipe packs.

**Exported symbols:** `RecipeDefinition`, `get_recipe_content`, `get_recipes_path`, `get_recipes_paths`, `parse_recipe_anchors`, `load_recipe_pack`, `list_recipe_entries`, `find_recipe_entry`, `validate_recipe_inputs`, `register_recipes_tools`

## get_recipes_path

```python
get_recipes_path(metadata: Any) -> str | None
```

Extract the recipes file path from a `SkillMetadata` object. Supports both flat (`"dcc-mcp.recipes"`) and nested (`"dcc-mcp": {"recipes": ...}`) forms. Returns absolute path resolved relative to skill's `skill_path`.

## get_recipes_paths

```python
get_recipes_paths(metadata: Any) -> list[str]
```

Extract all recipe sibling file paths. The metadata value may be a filename, glob string, or list of filenames/globs.

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

## Structured Recipe Packs

YAML recipe packs use a `recipes:` list:

```yaml
recipes:
  - name: build_pbr_material
    dcc: maya
    description: Build a PBR material network.
    inputs_schema:
      type: object
      required: [material_name]
      properties:
        material_name:
          type: string
    steps:
      - tool: maya_materials__create
        arguments:
          name: ${material_name}
    output_contract: material_graph
```

```python
load_recipe_pack(path) -> list[RecipeDefinition]
list_recipe_entries(skill_metadata) -> list[dict]
validate_recipe_inputs(recipe, inputs) -> list[str]
```

## register_recipes_tools

```python
register_recipes_tools(server, *, skills: list, dcc_name="dcc") -> None
```

Register `recipes__list`, `recipes__search`, `recipes__get`, `recipes__validate`, and `recipes__apply` MCP tools on `server`. Call **before** `server.start()`.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig, scan_and_load
from dcc_mcp_core.recipes import register_recipes_tools

loaded, _ = scan_and_load(dcc_name="maya")
server = create_skill_server("maya", McpHttpConfig(port=8765))
register_recipes_tools(server, skills=loaded, dcc_name="maya")
handle = server.start()
```
