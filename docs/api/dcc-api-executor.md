# DCC API Executor — Code-Orchestration for Huge APIs

> Source: [`python/dcc_mcp_core/dcc_api_executor.py`](https://github.com/loonghao/dcc-mcp-core/blob/main/python/dcc_mcp_core/dcc_api_executor.py) · Issue [#411](https://github.com/loonghao/dcc-mcp-core/issues/411)
>
> **[中文版](../zh/api/dcc-api-executor.md)**

Implements the "Cloudflare pattern" for enormous DCC APIs: expose just
**two tools** — `dcc_search` and `dcc_execute` — that together cover the
entire DCC Python API in ~500 tokens, instead of listing each of 1500+
commands individually.

**Why this matters for DCCs**

| DCC | Approximate API surface |
|-----|-------------------------|
| Maya | 2000+ MEL / Python commands |
| Houdini | 1500+ `hou` methods |
| Blender | 800+ `bpy` operators |
| 3ds Max | 1000+ `pymxs` commands |

Registering each as its own MCP tool bloats `tools/list` past agent
context budgets. Code orchestration keeps the surface constant: **2 tools,
any DCC, any size**.

> Reference: Anthropic — *"Building agents that reach production systems with
> MCP"* (Apr 22, 2026): "Design for code orchestration when your surface
> is large. Cloudflare's MCP server is the reference example — two tools
> (search and execute) cover ~2,500 endpoints in roughly 1K tokens."

## Imports

```python
from dcc_mcp_core import (
    DccApiCatalog,
    DccApiExecutor,
    register_dcc_api_executor,
)
```

## `DccApiCatalog(dcc_name, commands=None, catalog_text=None)`

Searchable catalog of DCC API command signatures and descriptions.

**Construction sources**

1. `commands` — explicit list of dicts: `[{"name": "polyCube", "signature": "polyCube(...)", "description": "Create a cube mesh."}, ...]`
2. `catalog_text` — plain-text catalog, one command per line as `name - description`. Blank lines and `#` comments are ignored.
3. `add_command(name, *, signature="", description="")` — append at runtime.

**Search** — `search(query: str, *, limit: int = 10) -> list[dict]`

- Tokenises on whitespace/punctuation
- Drops stopwords (`the`, `a`, `an`, `in`, `for`, `of`)
- Scores by token-overlap count across `name + description + signature`
- Returns top-`limit` by score, then alphabetical by name
- Never surfaces zero-score results

`len(catalog)` returns the number of commands.

## `DccApiExecutor(dcc_name, catalog=None, dispatcher=None)`

The two-tool wrapper.

| Arg | Type | Default | Notes |
|-----|------|---------|-------|
| `dcc_name` | `str` | — | DCC identifier |
| `catalog` | `DccApiCatalog \| None` | new empty catalog | Used by `dcc_search` |
| `dispatcher` | `ToolDispatcher \| None` | `None` | When provided, `dcc_execute` exposes `dispatch(name, args)` inside the script |

### `.search(query, *, limit=10) -> dict`

Handles `dcc_search` calls. Returns:

```json
{
  "success": true,
  "message": "Found 3 command(s) matching 'create sphere'.",
  "results": [
    {"name": "polySphere", "signature": "...", "description": "..."}
  ]
}
```

No hits → `"results": []` and a `"hint"` suggesting `search_skills`.

### `.execute(code, *, timeout_secs=30) -> dict`

Handles `dcc_execute` calls. Runs the snippet through
[`EvalContext`](./batch.md) with `sandbox=True`. When a `ToolDispatcher`
was passed to the constructor, `dispatch()` is available inside the
script.

Returns one of:

- `{"success": True, "output": <script return value>, "message": "..."}`
- `{"success": False, "error": "...", "message": "Script timed out ..."}`
- `{"success": False, "error": "...", "message": "Script failed ..."}`

`output` is the value returned by the script via a top-level `return`
statement (the last expression is not implicitly returned).

## `register_dcc_api_executor(server, executor, *, search_tool_name="dcc_search", execute_tool_name="dcc_execute") -> None`

Register both tools on a `McpHttpServer` *before* `server.start()`. Tool
names are overridable for disambiguation in multi-DCC gateways
(`maya_search`, `blender_search`, …).

## End-to-end example

```python
from dcc_mcp_core import (
    ToolRegistry, ToolDispatcher,
    McpHttpServer, McpHttpConfig,
    DccApiCatalog, DccApiExecutor, register_dcc_api_executor,
)

registry = ToolRegistry()
dispatcher = ToolDispatcher(registry)

catalog = DccApiCatalog(
    "maya",
    catalog_text="""
polyCube - Create a cube polygon mesh
polySphere - Create a sphere polygon mesh
select - Select nodes in the scene
render - Render the current frame
""",
)

executor = DccApiExecutor("maya", catalog=catalog, dispatcher=dispatcher)

server = McpHttpServer(registry, McpHttpConfig(port=8765))
register_dcc_api_executor(server, executor)
handle = server.start()
# tools/list now contains exactly 2 entries: dcc_search, dcc_execute
```

## Agent usage pattern

```text
User  : "Create 5 spheres arranged on a line"
Agent : dcc_search({"query": "create sphere"})         → finds polySphere
Agent : dcc_execute({"code": "..."} )
        for i in range(5):
            dispatch("polySphere", {"position": [i, 0, 0]})
        return {"created": 5}
Server: { "output": {"created": 5}, "success": true }
```

Only the final script return value reaches the model — the 5 intermediate
dispatches never consume tokens.

## Current status

Python helpers ship today. The Rust-level MCP built-in `dcc_search` /
`dcc_execute` tools are tracked in issue
[#411](https://github.com/loonghao/dcc-mcp-core/issues/411). Until they
land, register via `register_dcc_api_executor(server, executor)` — the
Python handlers behave identically.

## See also

- [`EvalContext` / `batch_dispatch`](./batch.md) — the sandboxed executor under the hood
- [Skills System](../guide/skills.md) — how `search-hint` populates catalog entries from SKILL.md
- [Remote Server guide](../guide/remote-server.md)
