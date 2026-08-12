# Marketplace: Typed Package Catalog

The marketplace is a CLI-first discovery and installation system for official and
community packages. It resolves human-readable names from one or more
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
| `marketplace search --query <q>`           | Fuzzy-rank entries across all sources; current builds also accept positional query words |
| `marketplace inspect <name>`              | Show full entry metadata                       |
| `marketplace install <name> --dcc <dcc> --reload` | Install a plugin, bundle, or Skill and refresh running adapters |
| `marketplace install <name> --target <kind:id>` | Install a generic package such as a CUA Profile |
| `marketplace list-installed --dcc <dcc>`  | List installed packages                        |
| `marketplace list-installed --target <kind:id>` | List packages for an application target |
| `marketplace uninstall <name> [--dcc <dcc>] [--reload]`| Remove an installed package and optionally refresh the adapter |
| `marketplace outdated [name] --dcc <dcc>` | Check for newer versions                       |
| `marketplace update [name] --all`         | Upgrade installed packages                     |
| `marketplace add-repo <repo> [--dcc]`     | Install directly from a GitHub repo            |
| `marketplace pack <path> --out dist/`     | Build a zip package and SHA-256 digest         |
| `marketplace publish <path> --catalog <file>` | Upsert a catalog entry for a package       |

Full argument reference: [cli-reference.md](cli-reference.md#marketplace).

## Package Shapes

The install command accepts all existing single-Skill packages and bundles.
It also recognizes [Agent Plugins 1.0](https://agent-plugins.org/specification)
when the package has a root `plugin.json`. Agent Plugin Skills are discovered
only from immediate `skills/<name>/SKILL.md` children; the manifest schema,
plugin name, and resolved path containment are validated before install.

`package.format: agent-plugin` advertises that portable layout.
`package.format: skill-bundle` advertises the existing DCC-MCP layout with
multiple `source.skillRoots`. In both cases `package.skills` provides the
component names shown by the marketplace UI. DCC-MCP currently installs Skill
components and ignores optional Agent Plugin `mcp.json` because DCC adapters
already own the runtime MCP connection.

Catalog v2 also supports generic `targets` and typed `components`. Target kinds
are `dcc`, `application`, `game`, and `web`; package formats are `skill`,
`skill-bundle`, `agent-plugin`, `cua-profile`, and `composite`. A CUA Profile
entry has one `cua-profile` component with a package-relative root:

```json
{
  "name": "the-bazaar-profile",
  "description": "The Bazaar semantic profile",
  "targets": [{"kind": "game", "id": "the-bazaar"}],
  "package": {
    "format": "cua-profile",
    "components": [
      {"kind": "cua-profile", "id": "the-bazaar", "root": "."}
    ]
  }
}
```

The Marketplace delegates Profile validation, installation, and removal to
`dcc-cua profile validate|install|uninstall` with exact process arguments. Core
does not parse the Profile schema. `DCC_MCP_CUA_BINARY` may select an explicit
binary; otherwise the CLI checks its sibling directory and then `PATH`.

## Installation Types

Three install types are supported, controlled by the catalog entry's
`install.type` field. Adapter entries may also set
`install.instructions_url` to the raw adapter-maintained `install.md`; the
`install` command exposes that URL as a `read-install-instructions` next step so
agents follow the latest host-specific setup runbook instead of core-hardcoded
DCC instructions. Use `install.python_path` only when the catalog intentionally
pins a host Python interpreter; the older `mayapy_path` spelling remains a
backward-compatible input alias and should not be used for new entries.

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

## Release Packaging (`pack` / `publish`)

Use `pack` in package repositories to build a zip and digest, then upload the
zip with GitHub's release tooling and use `publish` to update a local
`marketplace.json` checkout:

```bash
dcc-mcp-cli marketplace pack . --out dist/
gh release upload v0.1.0 dist/my-skill.zip --clobber
dcc-mcp-cli marketplace publish . \
  --catalog ../marketplace/marketplace.json \
  --install-url https://github.com/<owner>/<repo>/releases/download/v0.1.0/my-skill.zip \
  --sha256 sha256:<digest>
```

Publish a Profile package with explicit typed metadata:

```bash
dcc-mcp-cli marketplace publish . \
  --catalog ../marketplace/marketplace.json \
  --install-url https://example.com/the-bazaar-profile.zip \
  --name the-bazaar-profile \
  --description "The Bazaar semantic profile" \
  --target game:the-bazaar \
  --format cua-profile \
  --component cua-profile:the-bazaar=.
```

This is the recommended path for stable packages. Development packages can
still use `install.type: git` entries.

Minimal GitHub Actions shape for package repositories:

```yaml
on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Pack marketplace package
        run: dcc-mcp-cli marketplace pack . --out dist/
      - name: Upload release asset
        run: gh release upload "${GITHUB_REF_NAME}" dist/*.zip --clobber
        env:
          GH_TOKEN: ${{ github.token }}
```

## Direct GitHub Install (`add-repo`)

Inspired by `npx skills`, the `marketplace add-repo` command installs a Skill or Agent Plugin
directly from a GitHub repository without requiring a `marketplace.json` catalog
entry. It clones the repo, validates a root `plugin.json` when present, and
installs either the complete plugin or the matching Skill.

### Usage

```bash
# Install from GitHub shorthand (owner/repo → https://github.com/owner/repo.git)
dcc-mcp-cli marketplace add-repo dcc-mcp/dcc-mcp-maya --dcc maya

# Install from full URL
dcc-mcp-cli marketplace add-repo https://github.com/dcc-mcp/dcc-mcp-maya --dcc maya

# List available skills in a repo without installing
dcc-mcp-cli marketplace add-repo dcc-mcp/dcc-mcp-maya --list

# Install a subpath within a repo (e.g., owner/repo@subdir)
dcc-mcp-cli marketplace add-repo my-org/skill-repo@maya-skills --dcc maya

# Force replace an existing installation
dcc-mcp-cli marketplace add-repo dcc-mcp/dcc-mcp-maya --dcc maya --force
```

### How It Works

1. **Clone**: runs `git clone --depth 1` of the target repo to a temporary
   staging directory.
2. **Discover**: validates root `plugin.json` and its immediate `skills/*`
   components when present; otherwise scans for `SKILL.md` files.
3. **Parse**: reads plugin metadata plus each Skill's `name`, `description`, and
   optional `metadata.dcc-mcp.dcc`.
4. **Select**: installs every valid Skill in an Agent Plugin as one package. A
   multi-Skill plugin needs `--dcc` unless all Skills declare the same DCC.
5. **Install**: copies each Skill to
   `~/.dcc-mcp/marketplace/<dcc>/<skill-name>/` and writes one package manifest
   under `.packages/<plugin-name>/`.

### SKILL.md Discovery

Skills are discovered by finding `SKILL.md` files — first checking the repo
root, then nested skill directories. The frontmatter must contain at least a `name` field.
The `dcc` field (under `metadata.dcc-mcp.dcc`) is optional but recommended; use
`--dcc` when it is absent.

### Repo Reference Formats

| Format                        | Example                                         |
|-------------------------------|-------------------------------------------------|
| GitHub shorthand              | `dcc-mcp/dcc-mcp-maya`                          |
| Full HTTPS URL                | `https://github.com/dcc-mcp/dcc-mcp-maya.git`   |
| SSH URL                       | `git@github.com:dcc-mcp/dcc-mcp-maya.git`       |
| With subpath                  | `dcc-mcp/dcc-mcp-maya@subdir`                   |

### Comparison with Catalog Install

| Aspect                  | `marketplace install`        | `marketplace add-repo`          |
|-------------------------|------------------------------|---------------------------------|
| Requires catalog entry  | Yes                          | No                              |
| Source resolution       | Via sources.json / --source  | Direct GitHub clone             |
| Version tracking        | Catalog version + git ref    | HEAD of cloned branch           |
| Update mechanism        | Catalog-driven update check  | Re-clone (future)               |
| SKILL.md discovery      | Via catalog entry metadata   | Filesystem scan                 |

## Directory Layout

Installed packages land under:

```
~/.dcc-mcp/marketplace/
├── sources.json              # registered source list
├── installed.json            # installed-package state
├── maya/
│   ├── dcc-mcp-maya-skills/  # installed git clone
│   ├── my-custom-skill/      # installed path copy
│   └── .packages/
│       └── rig-plugin/       # one manifest for unified bundle uninstall
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
- **Plugin containment**: Agent Plugin manifests and fixed Skill components
  must resolve inside the plugin root; unsupported manifest schemas are rejected.
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
  package:
    format: agent-plugin              # agent-plugin | skill-bundle
    skills: [maya-rig, rig-review]    # displayed package components
  maintainer: "team@example.com"     # optional contact
```

## Admin Workflow

The **Marketplace** panel in the Admin Dashboard provides the same capabilities
as the CLI `marketplace` subcommand through a graphical interface. It is
accessible from the left navigation in the Admin Dashboard.

### Accessing the Panel

Navigate to the Admin Dashboard (`/admin`) and select **Marketplace** from the
left navigation. The panel loads the catalog from all configured sources and
displays the Browse tab by default.

### Browsing and Installing

1. **Browse tab**: use the search bar to find packages by name, description, or
   tags. Use the DCC type Chip row to filter by application type. Search and
   DCC filter can be combined.
2. **Inspect**: click any package card to open the detail modal, which shows
   full metadata including version, included Skills, DCC type, tags, maintainer, project URL,
   source, install type, and `min_core_version` compatibility information.
3. **Install**: click the **Install** button on a card or in the detail modal.
   On success, an inline notice appears with a **View in Skills** deep link
   that jumps to the Skills panel and highlights the newly loaded skill.

### Managing Installed Packages

Switch to the **Installed** tab to see all currently installed packages. Each
card shows the package name, version, DCC type, and install type. Click
**Uninstall** to remove a package. Both install and uninstall operations
automatically refresh the skill index when the backend reports a
`reload_required` flag.

### Adding Sources from the UI

The Marketplace panel reflects the same source configuration as the CLI. Source
management is available through the Admin API (`POST /admin/api/marketplace/sources`)
and is also accessible from the panel's source management interface.

Note that `marketplace add-repo` (direct GitHub install) is a CLI-only feature
and does not require a catalog source entry. Admin UI support is planned for a
future phase.

### Relationship Between CLI and Admin UI

| Aspect | CLI | Admin UI |
|--------|-----|----------|
| Catalog search | `marketplace search --query <q>` | Browse tab with search + DCC filter |
| Install | `marketplace install <name> --dcc <dcc> --reload` | Install button on card or detail modal |
| Uninstall | `marketplace uninstall <name> [--dcc <dcc>] [--reload]` | Uninstall button in Installed tab |
| List installed | `marketplace list-installed --dcc <dcc>` | Installed tab |
| Add source | `marketplace add <source>` | Source management in panel |
| Direct GitHub install | `marketplace add-repo <repo> --dcc <dcc>` | Admin API (planned) |
| Update | `marketplace update [name] --all` | Admin API (`POST /admin/api/marketplace/update`) |
| Live adapter refresh | Bundled with install `--reload`; standalone after update/uninstall | Automatic when the backend reports `reload_required` |

Both interfaces share the same installed package state. For a known exact ID,
CLI users can install and refresh running adapters in one command with
`marketplace install <name> --dcc <dcc> --reload`; `--dcc` is optional for a
single-DCC package. The Admin UI triggers the same refresh automatically when
its backend reports `reload_required`. Updates and uninstalls still use the
standalone `reload-skills` command when a live refresh is needed, unless CLI
uninstall uses `--reload`. The CLI also performs a short read-only update check
on startup and reports available Skill updates; applying them remains
consent-gated through `marketplace update`.

## See Also

- [cli-reference.md](cli-reference.md) — CLI command reference with full flag
  documentation
- [catalog.md](catalog.md) — DCC-MCP public adapter catalog format
- [skills.md](skills.md) — how to author a skill pack
- [admin-ui.md](admin-ui.md) — marketplace panel in the web dashboard
