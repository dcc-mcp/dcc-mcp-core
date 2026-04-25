# Docs Resources API

`docs://` MCP resource provider for agent-facing format specs and usage guides (issue #435).

Instead of embedding full format specifications in tool descriptions (which consumes tokens on every `tools/list` call), tool descriptions contain a brief pointer like `"For the full output schema, fetch docs://output-format/call-action"`. Agents fetch only the specifications they actually need.

**Exported symbols:** `get_builtin_docs_uris`, `get_docs_content`, `register_docs_resource`, `register_docs_resources_from_dir`, `register_docs_server`

## Built-in docs:// Resources

| URI | Content |
|-----|---------|
| `docs://output-format/call-action` | `tools/call` return value schema |
| `docs://output-format/list-actions` | `tools/list` response structure |
| `docs://skill-authoring/tools-yaml` | `tools.yaml` schema + conventions |
| `docs://skill-authoring/annotations` | `ToolAnnotations` reference |
| `docs://skill-authoring/sibling-files` | SKILL.md sibling-file pattern (v0.15+) |
| `docs://skill-authoring/thin-harness` | thin-harness layer guide pointer |

## get_builtin_docs_uris

```python
get_builtin_docs_uris() -> list[str]
```

Return the list of built-in `docs://` resource URIs.

## get_docs_content

```python
get_docs_content(uri: str) -> dict | None
```

Return the content dict for a `docs://` URI (keys: `name`, `description`, `mime`, `content`), or `None` if unknown.

## register_docs_resource

```python
register_docs_resource(server, *, uri: str, name: str, description: str, content: str, mime: str = "text/markdown") -> None
```

Register a single `docs://` resource on `server`. URI must start with `docs://`.

## register_docs_resources_from_dir

```python
register_docs_resources_from_dir(server, *, directory: str | Path, uri_prefix: str = "docs://custom", glob: str = "**/*.md") -> list[str]
```

Register all Markdown files under `directory` as `docs://` resources. Returns URIs of successfully registered resources.

## register_docs_server

```python
register_docs_server(server) -> None
```

Register all built-in `docs://` resources on `server`. Call **before** `server.start()`.

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig
from dcc_mcp_core.docs_resources import register_docs_server

server = create_skill_server("maya", McpHttpConfig(port=8765))
register_docs_server(server)
handle = server.start()
# Agents can now: resources/read docs://output-format/call-action
```
