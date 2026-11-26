# MCP Skills Extension

`dcc-mcp-core` exposes discovered Agent Skills through the official
`io.modelcontextprotocol/skills` extension. The extension is additive to the
existing `search_skills`, `list_skills`, `get_skill_info`, and `load_skill`
workflow. Reading a skill resource does not load its tools.

The extension uses the server's existing MCP Resources surface and is enabled
by default when Resources are enabled. Set
`McpHttpConfig.enable_skills_extension = false` to disable it.

## Capability declaration

Servers that expose the extension declare Resources and the extension settings
in the MCP capability response:

```json
{
  "resources": {"subscribe": true, "listChanged": true},
  "extensions": {
    "io.modelcontextprotocol/skills": {"directoryRead": true}
  }
}
```

The compatibility `server/discover` request is also supported and returns the
same extension declaration with the server's supported protocol versions.

## Methods

`skills/list` returns complete entries for publishable skills. Modern
`2026-07-28` responses include the standard `resultType`, `ttlMs`, and
`cacheScope` fields. Each entry
contains the `SKILL.md` URI, verbatim YAML frontmatter, and a manifest of every
file with its raw byte size and `sha256:` digest. Results include the standard
pagination cursor and keep each skill entry intact. Legacy initialize/session
clients receive the compatible result shape for their negotiated protocol.

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
