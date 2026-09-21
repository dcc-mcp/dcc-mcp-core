# MCP Skills Extension

`dcc-mcp` serves its skills through two parallel discovery tracks over the
`2026-07-28` stateless protocol:

| Track | Methods | Audience |
|-------|---------|----------|
| Tools (existing) | `search_skills`, `list_skills`, `get_skill_info` | every MCP client |
| Skills extension (new) | `skills/list`, `skills/get`, `resources/directory/read` | hosts implementing `io.modelcontextprotocol/skills` |

Both tracks read the **same catalog**, so a skill has the same `name` and the
same content on either path. The tools track is unchanged — adopting the
extension requires no migration.

The extension is specified against base protocol revision `2026-07-28` or
later; legacy `2025-x` sessions are unaffected.

## Specification

- [Skills over MCP overview](https://modelcontextprotocol.io/extensions/skills/overview)
- [ext-skills specification (stable)](https://github.com/modelcontextprotocol/ext-skills/blob/main/specification/stable/skills.mdx)
- [ADR-010 — dual-protocol migration](../adr/010-mcp-2026-07-28-dual-protocol-migration.md)

## Server configuration

The extension is **enabled by default** on the stateless path. Opt out with
`McpHttpConfig.enable_skills_extension = false`:

```python
from dcc_mcp_core import McpHttpConfig, create_skill_server

cfg = McpHttpConfig(port=8765)
cfg.enable_skills_extension = False
server = create_skill_server("maya", cfg)
server.start()
```

Because the extension serves its content through `resources/read`, declaring it
also requires the `resources` capability. Setting `enable_resources = false`
therefore suppresses the extension too, and the three methods answer
`Method not found`.

## Capability negotiation

`server/discover` declares both:

```json
{
  "capabilities": {
    "resources": { "subscribe": false, "listChanged": false },
    "extensions": {
      "io.modelcontextprotocol/skills": { "directoryRead": true }
    }
  }
}
```

`directoryRead: true` means `resources/directory/read` is implemented for every
directory inside a served skill.

## Resource URIs

Each file of a skill is an individually addressable resource:

```
skill://<skill-name>/SKILL.md           the skill's entry point
skill://<skill-name>/<relative-path>    any supporting file
```

The **first** path segment is the skill's `name` as declared in its `SKILL.md`
frontmatter, so the name is recoverable from the URI alone. The final segment
is the file being addressed (`SKILL.md` for the entry point, otherwise the
supporting file's own name), so it is not the skill name in general. A skill's
root directory is the entry-point URI with the `/SKILL.md` suffix removed and
no trailing slash — `skill://maya-geo`.

## `skills/list`

Returns one complete entry per skill: its `SKILL.md` URI, the frontmatter
verbatim, and a complete manifest of every file with its SHA-256 digest and
byte size.

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "resultType": "complete",
    "skills": [
      {
        "uri": "skill://maya-geo/SKILL.md",
        "frontmatter": {
          "name": "maya-geo",
          "description": "Polygon modelling helpers for Maya",
          "license": "MIT"
        },
        "resources": [
          {
            "uri": "skill://maya-geo/SKILL.md",
            "digest": "sha256:b95a384300adeea2d902f7d19cd7c04b378ef58e09759107b9c7db4dcacbaa25",
            "size": 190
          }
        ]
      }
    ],
    "ttlMs": 0,
    "cacheScope": "private"
  }
}
```

Notes:

- `resources` is complete — it lists `SKILL.md` and every supporting file, each
  exactly once. A host verifies each file it reads against that entry.
- `frontmatter` passes every field the skill author wrote through unchanged,
  including keys dcc-mcp itself does not model.
- Results are paginated with `cursor` / `nextCursor`. An entry is atomic: a
  skill's `resources` set is never split across pages.

## `skills/get`

Returns the entry for a single skill, named by the URI of its `SKILL.md`. It
does **not** depend on a prior `skills/list`, and it answers for any skill the
server serves, including one absent from a partial listing.

```json
{ "jsonrpc": "2.0", "id": 5, "method": "skills/get", "params": { "uri": "skill://maya-geo/SKILL.md" } }
```

An unknown URI returns `-32602` (Invalid params), the same code
`resources/read` uses for an unknown resource.

## `resources/read`

Skill files are read with the standard method; no skill-specific read semantics
are defined. The bytes returned are exactly the bytes the entry's digest and
size describe. `SKILL.md` is served as `text/markdown` with `text`
payloads; non-UTF-8 files come back as a base64 `blob`.

Relative references inside a skill resolve against that skill's root, so
`references/GUIDE.md` in `skill://maya-geo/SKILL.md` is
`skill://maya-geo/references/GUIDE.md`.

Reading a `SKILL.md` does not by itself activate the skill — activation is the
host's own decision, taken after verifying content and obtaining any approval it
requires.

## `resources/directory/read`

Lists the direct children of a skill directory. Non-recursive: descend by
calling the method again on a child directory. Subdirectories come back as
directory resources with `mimeType: "inode/directory"`.

```json
{ "jsonrpc": "2.0", "id": 7, "method": "resources/directory/read", "params": { "uri": "skill://maya-geo/templates" } }
```

A URI that does not exist, or exists but is not a directory, returns `-32602`.

## Limits

| Limit | Value |
|-------|-------|
| Files per skill | 512 |
| Total size per skill | 16 MiB (16,777,216 bytes) |

Both are the values every conforming host must accept. A skill on disk that
exceeds either limit is **not published** through the extension — it stays
available on the tools track, and a warning is logged at `WARN` level rather
than served with a manifest a host would reject.

## Errors

| Code | Cause |
|------|-------|
| `-32602` | Unknown skill URI, unknown skill file, or a directory read on a non-directory |
| `-32601` | Method not found — extension disabled, or `resources` switched off |

## Diagnostics

`skills/list` computes digests from the bytes on disk on every call, so a cache
entry can never go stale relative to what `resources/read` will return. The
trade-off is that listing walks every served skill; hosts should honour the
`ttlMs` hint and re-list only when it expires.
