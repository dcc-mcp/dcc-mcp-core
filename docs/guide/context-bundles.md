# Context Bundles

A context bundle is the resolved runtime identity for a DCC MCP session. It
answers: which production domain is this, what kind of work is being done, which
project/shot/asset is active, and which skill packages should be visible?

```text
Rez context -> env vars -> DCC startup -> skill scan -> gateway metadata -> context-aware tools/list
```

`dcc-mcp-core` reads this resolved environment. It does not choose packages,
solve versions, or replace a studio package manager.

## Runtime Flow

1. Rez resolves packages for a project, department, task, asset type, and DCC.
2. The package commands set `DCC_MCP_*` context and path variables.
3. The DCC adapter starts `DccServerBase` or `McpHttpServer`.
4. Skills are discovered from `DCC_MCP_SKILL_PATHS` and
   `DCC_MCP_<DCC>_SKILL_PATHS`.
5. The gateway records context metadata in `FileRegistry`.
6. Clients call `list_dcc_instances`, `search_skills`, and `load_skill` against
   the selected context instead of exposing every studio tool at once.

## Metadata Keys

The gateway returns context metadata under each instance's `metadata` field.
The built-in `DccServerBase` populates these keys from environment variables:

| Metadata key | Environment variable |
|--------------|----------------------|
| `context_bundle` | `DCC_MCP_CONTEXT_BUNDLE` |
| `production_domain` | `DCC_MCP_PRODUCTION_DOMAIN` |
| `context_kind` | `DCC_MCP_CONTEXT_KIND` |
| `project` | `DCC_MCP_PROJECT` |
| `sequence` | `DCC_MCP_SEQUENCE` |
| `shot` | `DCC_MCP_SHOT` |
| `asset` | `DCC_MCP_ASSET` |
| `asset_type` | `DCC_MCP_ASSET_TYPE` |
| `task` | `DCC_MCP_TASK` |
| `toolset_profile` | `DCC_MCP_TOOLSET_PROFILE` |
| `package_provenance` | `DCC_MCP_PACKAGE_PROVENANCE` |
| `skill_paths` | `DCC_MCP_SKILL_PATHS` |
| `dcc_skill_paths` | `DCC_MCP_<DCC>_SKILL_PATHS` |

Adapters that do not use `DccServerBase` can set the same values explicitly via
`McpHttpConfig.instance_metadata`.

## Gateway Routing

The gateway is intentionally a selector over already-launched contexts. It
should not multiply every backend tool into one massive global surface. A client
should first inspect `list_dcc_instances`, choose a matching bundle or DCC
session, and then load/search skills in that selected context.

Example selection criteria:

- `production_domain=film`, `context_kind=shot`, `task=animation` -> Maya
  animation blocking tools.
- `production_domain=film`, `context_kind=shot`, `task=fx` -> Houdini cache and
  sim review tools.
- `production_domain=game`, `context_kind=level` -> level-layout tools.

## Examples

Copyable manifests live under `examples/context-bundles/`. Example Rez skill
packages live under `examples/rez-skills/`; each package has a `package.py`,
`SKILL.md`, `tools.yaml`, `README.md`, and a tiny script.

For package layout and provenance guidance, see
[rez-skill-packages.md](rez-skill-packages.md).
