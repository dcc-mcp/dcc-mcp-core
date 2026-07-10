---
name: marketplace-publish-extension
description: >-
  Infrastructure skill — publish (register/update) an extension package to a
  marketplace catalog. Reads the extension's SKILL.md frontmatter, constructs
  a CatalogEntry, and upserts it into the target marketplace.json. Optionally
  commits and pushes when the catalog source is a git repository. Use after
  scaffolding an extension with marketplace-create-extension. Not for
  installing or searching extensions — use marketplace-install or
  marketplace-search for that.
license: MIT-0
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: python
    version: "0.18.9"  # x-release-please-version
    layer: infrastructure
    search-hint: >-
      publish extension, register extension, marketplace catalog, upsert
      catalog entry, marketplace.json, extension publishing, release to
      marketplace
    tags: "marketplace, publishing, catalog, infrastructure"
    tools: tools.yaml
  openclaw:
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/marketplace-publish-extension/SKILL.md
---

# Marketplace Publish Extension

Publish (register or update) a dcc-mcp extension package to a marketplace
catalog (`marketplace.json`).

Prefer the CLI for local and CI publishing:

```bash
dcc-mcp-cli marketplace pack ./my-extension --out dist/
dcc-mcp-cli marketplace publish ./my-extension \
  --catalog ./marketplace.json \
  --install-url https://github.com/<owner>/<repo>/releases/download/v0.1.0/my-extension.zip \
  --sha256 sha256:<digest> \
  --min-core-version 0.19.0 \
  --skill-root skill/my-extension
```

## Tools

### `marketplace_publish_extension__publish`
Scan an extension directory, build a `CatalogEntry`, and upsert it into the
target `marketplace.json` (local file or remote URL-backed file). When the
catalog source is a local git repository the tool can optionally commit and
push the change.

## Prerequisites

- dcc-mcp-core installed
- Write access to the target marketplace catalog path
- Git (if using commit+push mode)

## Release Gate — Python 3.7

- **Verify py37 wheels exist** and py37 CI is green before publishing.
- `py37-lite` fallback wheels do NOT satisfy the release gate. Native py37
  builds are an LTS requirement with no automatic calendar expiry.
- Confirm `requires-python = ">=3.7"` in `pyproject.toml` is unchanged.
- Removing native py37 requires an accepted superseding ADR, a major release,
  and at least 180 days of notice.

## Workflow

1. Point the tool at a local extension directory containing `SKILL.md`.
2. The tool reads the SKILL.md frontmatter and any accompanying metadata.
3. Additional CLI-supplied fields (install url, ref, skill roots, tags, maintainer, icon,
   etc.) are merged in.
4. A `CatalogEntry` is built and upserted into the target `marketplace.json`.
   New catalogs use the v1 `skills` layout and preserve v1-only fields when an
   existing entry is updated.
5. If the catalog source is a git repo and `--commit` is passed, the updated
   `marketplace.json` is committed and pushed.

## Official catalog requirements

- Pass `--min-core-version`; v1 entries require an explicit compatibility floor.
- Git sources in the official catalog must use a complete 40-character commit
  SHA in `--install-ref`, never a mutable branch name.
- Declare every installed skill directory with `--skill-root`. The installer
  only loads declared roots, so multi-skill repositories do not accidentally
  expose examples or development-only skills.
- Zip sources require a 64-character SHA-256 digest. When the archive URL
  changes, provide the new digest rather than reusing old metadata.
- The publisher preserves v1 curation fields such as `requires` and `policy`
  when updating an existing entry, but new official entries must provide the
  complete marketplace metadata required by the catalog schema.
