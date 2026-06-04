# DCC-MCP Public Catalog

The DCC-MCP catalog is a community-maintained registry of adapters and skill packs for the DCC-MCP ecosystem (issue #774).

## Catalog File Format

`dcc-mcp-catalog.yml` in the repository root:

```yaml
version: "1"
entries:
  - name: dcc-mcp-maya-skills
    description: "Official Maya skill pack for DCC-MCP"
    dcc: [maya]
    url: "https://github.com/example/dcc-mcp-maya-skills"
    tags: [skills, maya, official]

  - name: dcc-mcp-blender-skills
    description: "Community Blender skill pack"
    dcc: [blender]
    url: "https://github.com/example/dcc-mcp-blender-skills"
    tags: [skills, blender, community]

  - name: dcc-mcp-houdini-adapter
    description: "Houdini DCC adapter for DCC-MCP"
    dcc: [houdini]
    url: "https://github.com/example/dcc-mcp-houdini"
    tags: [adapter, houdini, official]
```

## Entry Fields

| Field | Required | Type | Description |
|-------|----------|------|-------------|
| `name` | ✅ | string | Unique identifier (kebab-case recommended) |
| `description` | ✅ | string | One-sentence human-readable description |
| `dcc` | ✅ | list[string] | Supported DCC types (e.g. `[maya, blender]`) |
| `url` | ✅ | string | Repository or documentation URL |
| `tags` | ❌ | list[string] | Searchable tags (e.g. `skills`, `adapter`, `official`, `community`) |

## CLI Usage

```bash
# Search by keyword (matches name, description, DCC type, or tag)
dcc-mcp-server catalog search --query maya

# Search with no query → list all entries
dcc-mcp-server catalog search

# Describe a specific entry by exact name
dcc-mcp-server catalog describe --name dcc-mcp-maya-skills
```

Output is JSON-formatted for easy parsing.

## MCP Resource Usage

The gateway publishes the catalog as MCP **resources** (#813 phase 2). Read
them via `resources/read`:

```python
# Full index, optional ?query=... keyword filter
result = client.resources_read("gateway://catalog?query=blender")
# Returns: { "total": N, "query": "blender", "entries": [{"name": "...", "description": "...", "dcc": [...], "url": "...", "tags": [...]}] }

# Single entry by exact name
result = client.resources_read("gateway://catalog/dcc-mcp-blender-skills")
# Returns: single entry, or `-32002` error if not found
```

## Opt-in Documentation Connectors

Catalog entries can also point at read-only documentation MCP servers. These
entries are discovery hints only; they do not auto-enable remote connectors on
gateway startup.

Autodesk Product Help is modeled as a separate documentation backend, not as a
Maya, Houdini, Photoshop, or pipeline adapter:

```json
{
  "mcpServers": {
    "autodesk-product-help": {
      "url": "https://developer.api.autodesk.com/knowledge/public/v1/mcp"
    }
  }
}
```

Use `tags: [docs, autodesk, read-only, infrastructure]` for this connector.
Keep documentation lookups separate from `pipeline` / `shotgrid` searches so
production-tracking tools do not compete with product-help results.

Studio note: treat public documentation MCP servers as optional internet
dependencies. Autodesk Product Help is suitable for best-effort reference
lookup, but studios that require offline operation, pinned documentation, or
formal service guarantees should keep it disabled and route agents to approved
internal docs instead.

## Custom Catalog Path

Override the default `dcc-mcp-catalog.yml` location:

```bash
DCC_MCP_CATALOG_PATH=/path/to/my-catalog.yml dcc-mcp-server ...
```

Or set programmatically before calling the gateway tools.

## Search Behavior

- Case-insensitive substring match on `name`, `description`, `dcc`, and `tags`
- Empty query returns all entries
- `describe` requires an exact `name` match (case-sensitive)

## See also

- [skills.md](skills.md) — how to author a skill pack
- [mcp-skills-integration.md](mcp-skills-integration.md) — registering skills on the server
- [rez-skill-packages.md](rez-skill-packages.md) — Rez package layout for distributing skills
