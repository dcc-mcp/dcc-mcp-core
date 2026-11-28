# Skills over MCP API

`dcc-mcp-core` exposes discovered Agent Skills through the official
`io.modelcontextprotocol/skills` extension on supported per-DCC MCP endpoints.
The extension delivers Skill metadata and files. Tool activation uses the
existing `load_skill` workflow. Existing CLI integrations use `dccmcp` over
gateway REST, and direct legacy clients can keep using `search_skills`,
`list_skills`, and `get_skill_info`. [Integration guide](../guide/mcp-skills-integration.md)

Per-DCC servers enable the extension by default when Resources are enabled. Set
`McpHttpConfig.enable_skills_extension = false` to disable it.

## Capability declaration

Check the selected endpoint's `server/discover` response on MCP `2026-07-28`
before sending extension requests. Look for `resources` and the extension ID
under `capabilities`. The declaration includes these fields.

```json
{
  "resources": {"subscribe": false, "listChanged": false},
  "extensions": {
    "io.modelcontextprotocol/skills": {"directoryRead": true}
  }
}
```

The extension declaration is scoped to stateless discovery. Legacy session
clients use the built-in Skill tools listed above. See the
[modern request boundary](../guide/modern-request-boundary.md) for discovery
request headers and metadata.

## Methods

On MCP `2026-07-28`, `skills/list` returns complete entries for publishable
skills, with `resultType`, `ttlMs`, and `cacheScope` in the response. Each entry
contains the `SKILL.md` URI, verbatim YAML frontmatter, and a manifest of every
file with its raw byte size and `sha256:` digest. Results include the standard
pagination cursor and keep each skill entry intact.

`skills/get` accepts the URI of a skill's `SKILL.md` and returns the same
complete entry without requiring a previous listing.

`resources/read` serves every file in a published skill. UTF-8 files use the
`text` field. Other files use the base64-encoded `blob` field. Supporting files
are addressed relative to the skill root, for example
`skill://maya-geo/references/checklist.md`.

`resources/directory/read` lists direct children of a skill directory. Directory
entries use `mimeType: "inode/directory"`, and clients descend by issuing
another directory request.

## Publication rules

The server publishes only catalogued skills whose on-disk directory passes all
of these checks.

- `SKILL.md` exists and has valid YAML frontmatter with `name` and
  `description`.
- The frontmatter name matches the catalog skill name.
- The directory contains no more than 512 files.
- The directory contains no more than 16 MiB of file content.
- Symlinks that resolve outside the skill root are excluded.

Skills that fail these checks remain available through the existing skill tools
and are omitted from the extension manifest. Unknown skill URIs, invalid file
paths, and directory reads against files return JSON-RPC `-32602`.

## Security and verification

The manifest is built from the same bytes served by `resources/read`. Clients
can verify each resource's size and SHA-256 digest before using it. File paths
are checked lexically and after canonicalization, so `..`, Windows drive or
alternate-stream syntax, and escaping symlinks cannot leave the skill root.

Reading `SKILL.md` remains a content retrieval operation. Skill activation and
any associated approval are controlled by the host's skill-loading workflow.

## References

- [MCP Skills extension overview](https://modelcontextprotocol.io/extensions/skills/overview)
- [Skills Over MCP charter](https://modelcontextprotocol.io/community/working-groups/skills-over-mcp)
- [Agent Skills specification](https://agentskills.io/)
