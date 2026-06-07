# Marketplace Extension Authoring Guide

This guide walks through creating a publishable marketplace extension package
that follows the dcc-mcp skill contract.

## Directory Layout

```
my-extension/
├── SKILL.md       # Required: metadata frontmatter (MIT-0) + usage instructions
├── tools.yaml     # Required: tool declarations with schemas and annotations
├── scripts/       # Required: tool implementation scripts
│   └── <action>.py
└── references/    # Optional: recipes, examples, and long-form docs
```

## SKILL.md Frontmatter

Every extension must declare:

| Field | Required | Notes |
|-------|----------|-------|
| `name` | yes | kebab-case, <=64 chars, matches directory name |
| `description` | yes | Routing metadata: "Layer skill - scope. Use when..." |
| `license` | yes | Must be `MIT-0` for ClawHub compatibility |
| `compatibility` | yes | e.g. "dcc-mcp-core 0.17+, Python 3.7+" |
| `allowed-tools` | yes | Space-separated list (e.g. "Bash Read Write") |
| `metadata.dcc-mcp.dcc` | yes | Target DCC or "python" for infrastructure |
| `metadata.dcc-mcp.version` | yes | Semantic version string |
| `metadata.dcc-mcp.layer` | yes | infrastructure / domain / thin-harness / example |
| `metadata.dcc-mcp.tools` | yes | Path to tools.yaml (usually "tools.yaml") |
| `metadata.dcc-mcp.tags` | no | Comma-separated tags for gateway search |
| `metadata.dcc-mcp.search-hint` | no | Keywords for agent discovery |
| `metadata.openclaw.homepage` | no | GitHub URL for ClawHub publishing |

## tools.yaml Contract

Each tool declaration should include:

- `name`: snake_case local name (never dotted)
- `source_file`: path under `scripts/`
- `input_schema` / `output_schema`: JSON Schema objects with `type`, `properties`, `required`, `additionalProperties`
- `execution`: `sync` or `async`
- `affinity`: `main` for host API work, `any` for pure logic
- `enforce_thread_affinity`: `true` for host-bound tools
- `timeout_hint_secs`: realistic timeout in seconds
- `annotations`: `read_only_hint`, `destructive_hint`, `idempotent_hint`, `open_world_hint`, `deferred_hint`
- `next-tools.on-failure`: diagnostics tools for recovery

## Script Implementation

Scripts should prefer `dcc_mcp_core.skills_helper` for dependency-light helpers:

```python
from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success

@skill_entry
def main(param: str = "default", **params):
    return skill_success("Operation completed", param=param, **params)

if __name__ == "__main__":
    run_main(main)
```

Never import host APIs (`maya.cmds`, `bpy`, `pymxs`) at module level.
Keep them inside tool functions so metadata parsing works without the host.

## Catalog Entry

To publish your extension to a marketplace catalog, use
`marketplace-publish-extension` to upsert a `CatalogEntry` into the target
`marketplace.json`. The entry format:

```json
{
  "name": "my-extension",
  "description": "Extension description",
  "dcc": ["maya"],
  "version": "0.1.0",
  "maintainer": "Author Name",
  "tags": ["modeling", "maya"],
  "install": {
    "type": "git",
    "url": "https://github.com/user/my-extension",
    "ref": "main"
  }
}
```

Supported install types: `git`, `path`, `zip`.
