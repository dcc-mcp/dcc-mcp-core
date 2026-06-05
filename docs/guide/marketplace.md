# Marketplace: Skill Package Catalog & Installer

The marketplace is a CLI-first discovery and installation system for official and
community skill packages. It resolves human-readable names from one or more
catalog sources, downloads or clones the matching package, and registers it so
the DCC adapter discovers it on the next restart or `reload_skill_paths` call.

## Architecture

```
┌──────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  CLI (CLAP)  │ ──▶ │ Application      │ ──▶ │ Domain           │
│ marketplace  │     │ marketplace.rs   │     │ marketplace.rs   │
│ subcommand   │     │ (business logic) │     │ (types/sources)  │
└──────────────┘     └──────────────────┘     └──────────────────┘
       │                        │                       │
       │                        ▼                       │
       │              ┌──────────────────┐              │
       │              │ dcc-mcp-catalog  │              │
       └──────────────│ (parse/search)   │──────────────┘
                      └──────────────────┘
                              │
                              ▼
                     ┌──────────────────┐
                     │  Gateway         │
                     │  gateway://catalog│
                     │  MCP resources   │
                     └──────────────────┘
```

The marketplace code lives in three layers:

1. **Domain** (`crates/dcc-mcp-cli/src/domain/marketplace.rs`) — types:
   `MarketplaceSource`, `MarketplaceHit`, `MarketplaceSearchResult`,
   `InstalledMarketplacePackage`, `OutdatedMarketplacePackage`, etc.

2. **Application** (`crates/dcc-mcp-cli/src/application/marketplace.rs`) —
   business logic: source management, search across sources, installation,
   uninstallation, update checks.

3. **Catalog** (`crates/dcc-mcp-catalog/`) — standalone package that parses
   `marketplace.json` / `catalog.yml` files, searches entries by keyword and
   DCC type, and inspects individual entries.

The gateway also exposes catalog data through MCP resources (`gateway://catalog`)
with a 5-minute cache (see [catalog.md](catalog.md)).

## Sources

A marketplace **source** is a named reference to a catalog file. Sources are
persisted in `~/.dcc-mcp/marketplace/sources.json`.

| Source type         | Example                                        |
|---------------------|------------------------------------------------|
| Official (built-in) | `dcc-mcp/marketplace`                          |
| GitHub slug         | `my-org/my-skills`                             |
| Raw JSON URL        | `https://example.com/catalog.json`             |
| Local file          | `/path/to/local-catalog.yml`                   |

### Source Precedence

1. Built-in official source (`dcc-mcp/marketplace`)
2. User-configured sources (persisted in `sources.json`)
3. Environment variable sources (`DCC_MCP_MARKETPLACE_SOURCES`)
4. Explicit `--source` CLI flag

Set `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES=1` to disable the built-in source.

## CLI Commands

| Command                                   | Description                                    |
|-------------------------------------------|------------------------------------------------|
| `marketplace add <source>`                | Register a marketplace source                  |
| `marketplace list`                        | List configured sources                        |
| `marketplace search --query <q>`          | Search entries across all sources              |
| `marketplace inspect <name>`              | Show full entry metadata                       |
| `marketplace install <name> --dcc <dcc>`  | Install a skill package                        |
| `marketplace list-installed --dcc <dcc>`  | List installed packages                        |
| `marketplace uninstall <name> --dcc <dcc>`| Remove an installed package                    |
| `marketplace outdated [name] --dcc <dcc>` | Check for newer versions                       |
| `marketplace update [name] --all`         | Upgrade installed packages                     |

Full argument reference: [cli-reference.md](cli-reference.md#marketplace).

## Installation Types

Three install types are supported, controlled by the catalog entry's
`install.type` field:

### Git (`install.type: git`)

Clones the repository on install, then uses `git fetch && git checkout <ref>`
on subsequent updates. Best for actively developed skill packages.

```yaml
- name: dcc-mcp-maya-skills
  install:
    type: git
    url: "https://github.com/example/dcc-mcp-maya-skills.git"
    ref: "v1.2.0"
```

### Zip (`install.type: zip`)

Downloads a ZIP archive (from URL or local path) and extracts it. Supports
`sha256` verification. The archive root must contain exactly one top-level
directory, which is flattened automatically.

```yaml
- name: dcc-asset-hunyuan-download
  install:
    type: zip
    url: "https://example.com/packages/hunyuan-v2.zip"
    sha256: "a1b2c3d4e5f6..."
```

### Path (`install.type: path`)

Copies files from a local directory. Useful for development or internal
tooling.

```yaml
- name: my-internal-skills
  install:
    type: path
    url: "/share/skills/my-internal-skills"
```

## Directory Layout

Installed packages land under:

```
~/.dcc-mcp/marketplace/
├── sources.json              # registered source list
├── installed.json            # installed-package state
├── maya/
│   ├── dcc-mcp-maya-skills/  # installed git clone
│   └── my-custom-skill/      # installed path copy
└── blender/
    └── dcc-blender-skills/
```

DCC adapters automatically include `~/.dcc-mcp/marketplace/<dcc>` in their
skill search paths (see `collect_skill_search_paths()` in `server_base.py`),
so installed skills appear on adapter startup or `reload_skill_paths`.

## Environment Variables

| Variable                                   | Default                                      | Description                           |
|--------------------------------------------|----------------------------------------------|---------------------------------------|
| `DCC_MCP_MARKETPLACE_SOURCES`              | unset                                        | Comma-separated extra sources         |
| `DCC_MCP_MARKETPLACE_SOURCES_FILE`         | `~/.dcc-mcp/marketplace/sources.json`        | Sources persistence path              |
| `DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES`   | unset                                        | Disable built-in official source      |
| `DCC_MCP_MARKETPLACE_INSTALL_ROOT`         | `~/.dcc-mcp/marketplace`                     | Install root directory override       |
| `DCC_MCP_MARKETPLACE_OFFLINE`              | unset                                        | Force local-only catalog mode         |
| `DCC_MCP_MARKETPLACE_CATALOG_URL`          | official marketplace URL                     | Override remote catalog URL           |

## Security

- **Path traversal protection**: `marketplace_path_component()` rejects empty
  components, `.`, `..`, leading dots, and non-ASCII alphanumeric characters.
- **SHA256 verification**: Zip installs verify `install.sha256` when present
  and reject mismatches without modifying existing packages.
- **Archive escape detection**: Zip extraction rejects entries that escape the
  install root directory.
- **Force mode**: `--force` re-attempts install on failure but preserves the
  existing package when the replacement itself fails.

## Gateway Integration

The gateway exposes catalog data through MCP resources:

```python
# Search all catalog entries
result = client.resources_read("gateway://catalog?query=physics")

# Single entry by exact name
result = client.resources_read("gateway://catalog/dcc-mcp-physics-sim")
```

The gateway fetches the remote `marketplace.json` on a 5-minute cache cycle,
falling back to the local `dcc-mcp-catalog.yml` when offline. Set
`DCC_MCP_MARKETPLACE_OFFLINE=1` to force local-only mode.

## Catalog Entry Format

```yaml
- name: dcc-mcp-maya-skills          # unique kebab-case identifier
  description: "Official Maya skill pack"
  dcc: [maya]                        # supported DCC types
  url: "https://github.com/..."      # project URL
  tags: [skills, maya, official]     # searchable tags
  version: "1.2.0"                   # current version
  min_core_version: ">=0.17.0"       # minimum dcc-mcp-core version
  install:
    type: git                        # git | zip | path
    url: "https://github.com/..."
    ref: "v1.2.0"                    # tag/branch/commit (git type)
    sha256: "a1b2c3..."              # content hash (zip type)
  maintainer: "team@example.com"     # optional contact
```

## See Also

- [cli-reference.md](cli-reference.md) — CLI command reference with full flag
  documentation
- [catalog.md](catalog.md) — DCC-MCP public adapter catalog format
- [skills.md](skills.md) — how to author a skill pack
- [admin-ui.md](admin-ui.md) — marketplace panel in the web dashboard
