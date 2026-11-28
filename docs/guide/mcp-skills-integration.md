# Skills over MCP

[中文版](../zh/guide/mcp-skills-integration.md)

**dcc-mcp-core supports Skills over MCP**, the official extension for discovering
and reading Agent Skills. Compatible clients can inspect workflow metadata
before reading `SKILL.md` and supporting files. [Official Skills extension](https://modelcontextprotocol.io/extensions/skills/overview)

## Choose an endpoint

| Task | Entry point |
| --- | --- |
| Discover and read an Agent Skill | Connect to the target DCC's `mcp_url` and check for `io.modelcontextprotocol/skills`. |
| Find and run a tool in a live DCC session | Use `dccmcp` with the `dcc-mcp` Skill and follow the search result's `next_step`. |
| Keep an older MCP integration working | Use the session lifecycle. Per-DCC servers negotiate `2025-11-25`, `2025-06-18`, or `2025-03-26`. |

If you start from the gateway, read `gateway://instances` and use the
`mcp_url` of the session that owns the scene or document. Extension availability
depends on the endpoint, build, and configuration. Native `dcc-mcp-core` Python
wheels enable MCP `2026-07-28`. Custom builds need the corresponding Cargo
feature. [MCP migration guide](mcp-2026-07-28-migration.md)

## Discover and read a Skill

The [README example](https://github.com/dcc-mcp/dcc-mcp-core/blob/main/README.md#serve-a-skill-from-python)
serves a Photoshop Skill without opening Photoshop. Start the server and
substitute the printed endpoint for the URL in this Bash example.

```bash
curl -sS http://127.0.0.1:8765/mcp \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: server/discover' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "server/discover",
    "params": {
      "_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {}
      }
    }
  }'
```

1. Check `supportedVersions` and
   `capabilities.extensions["io.modelcontextprotocol/skills"]` in the result.
2. Use `skills/list` to browse entries or `skills/get` to look up a Skill by
   URI. Each entry includes frontmatter and a file manifest.
3. Read a file with `resources/read` using its exact manifest URI and verify the
   returned bytes against the SHA-256 digest. When the extension declares
   `directoryRead`, use `resources/directory/read` to browse a directory.
4. To activate DCC tools, follow the existing `load_skill` flow. Reading a
   Skill leaves its activation state unchanged.

Send the modern headers and request metadata with every request. See the
[Skills over MCP API reference](../api/skills-extension.md) for method details
and file publication rules.

## Use the CLI with existing integrations

```bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
```

Follow the search hit's `next_step` to load, describe, or call the tool. The
CLI remains the default for shell agents and works with existing scripts through
gateway REST. Older installations may use the `dcc-mcp-cli` executable alias.
A legacy direct MCP client can use `search_skills` to find Skills and
`load_skill` to activate tools. [CLI reference](cli-reference.md)

For Skill authors, `SKILL.md` describes the workflow, `tools.yaml` declares
typed operations, and sibling scripts implement them. The
[Skills authoring guide](skills.md) covers packaging and validation.
