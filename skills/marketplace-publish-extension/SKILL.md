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

## Workflow

1. Point the tool at a local extension directory containing `SKILL.md`.
2. The tool reads the SKILL.md frontmatter and any accompanying metadata.
3. Additional CLI-supplied fields (install url, ref, tags, maintainer, icon,
   etc.) are merged in.
4. A `CatalogEntry` is built and upserted into the target `marketplace.json`.
5. If the catalog source is a git repo and `--commit` is passed, the updated
   `marketplace.json` is committed and pushed.
