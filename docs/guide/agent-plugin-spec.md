# DCCMCP Agent Plugin Specification

The DCCMCP agent plugin is a package contract for agent hosts that need to discover and operate DCC applications through the DCCMCP CLI and MCP endpoints. The contract packages an agentskills.io `SKILL.md` with machine-readable runtime metadata.

## Package layout

An agent plugin package contains the following files.

```text
dcc-mcp/
├── SKILL.md
├── agent-plugin.json
├── references/
└── scripts/
```

`SKILL.md` remains the agent-facing instruction entrypoint. Its frontmatter follows agentskills.io. DCCMCP-specific fields stay under `metadata.dcc-mcp.*` in that file. `agent-plugin.json` describes packaging and runtime compatibility.

The canonical schema is [`contracts/agent-plugin.schema.json`](../../contracts/agent-plugin.schema.json). The reference manifest is [`skills/dcc-mcp/agent-plugin.json`](../../skills/dcc-mcp/agent-plugin.json).

## Manifest fields

`schema_version` identifies the manifest contract and is currently `"1"`.

`id` is a lowercase package identifier using letters, digits, hyphens, and dots. `name` is the display name. `version` and `runtime.minimum_cli` use semantic versioning.

`runtime.kind` is `dccmcp-cli`. `runtime.transports` declares the protocol surfaces used by the plugin. `streamable-http` describes MCP calls through `/mcp`. `rest` describes DCCMCP gateway control routes such as `/v1/search`, `/v1/describe`, and `/v1/call`.

`entrypoints` declares the files an agent host can load. Version 1 requires a `skill` entrypoint whose path is `SKILL.md`.

`capabilities` is a unique list of product capabilities. Hosts can use it to decide whether the plugin can satisfy a request before loading the full skill.

`commands` declares the user-facing command and compatibility aliases. The DCCMCP command is `dccmcp`. `dcc-mcp-cli` remains a compatibility alias while existing installations migrate.

`permissions` declares the classes of access the plugin may need. Values are descriptive capability names that a host can map to its own approval model. Version 1 groups them into `network`, `filesystem`, and `process`.

## Runtime behavior

The plugin uses the shared `dcc-mcp-client` implementation for MCP Streamable HTTP. The client performs `initialize`, records `Mcp-Session-Id`, negotiates a supported protocol version, sends `notifications/initialized`, and follows pagination cursors for tools, resources, and prompts.

The CLI control plane provides DCC inventory, skill discovery, tool description, DCC-MCP calls, gateway profiles, marketplace operations, and scoped UI Control. The generic MCP namespace exposes `initialize`, `tools`, `resources`, and `prompts` for any compatible MCP endpoint.

The plugin manifest does not register an MCP server in a registry. MCP server registration, endpoint discovery, authentication, and host-specific policy remain responsibilities of the agent host or DCCMCP gateway deployment.

## Validation and packaging

Validate the checked-in package with the repository script.

```bash
python scripts/validate_agent_plugin.py skills/dcc-mcp/agent-plugin.json
```

Package the skill tree with the existing OpenClaw-compatible packager.

```bash
python scripts/package_openclaw_skill.py skills/dcc-mcp dist/skills
```

CI should validate the manifest, verify the `SKILL.md` entrypoint, and test both `dccmcp` and `dccmcp` command names. A package version must match the release version carried by the workspace and skill metadata.
