# ADR-016: Unify application automation under UI Control naming

## Status

Accepted naming; native Host design superseded by [ADR-020](./020-external-cua-runtime.md)

## Context

The product capability, CLI command, native host, and safety UI already use
**UI Control**, while the bundled skill, MCP tool prefix, environment variables,
Python contracts, and Rust contract crate still use the older App UI name. The
split vocabulary makes discovery, deployment, support, and audit output harder
to reason about.

These identifiers are public contracts. This change intentionally makes a hard
cut so every package has one vocabulary and one callable surface.

## Decision

Use one canonical vocabulary everywhere new code is written:

| Surface | Canonical identifier |
| --- | --- |
| Bundled skill | `ui-control` |
| MCP tools | `ui_control__*` |
| CLI | `dcc-mcp-cli ui-control` |
| Environment | `DCC_MCP_UI_CONTROL_*` |
| Python contracts | `UiControlPolicy`, `UiControlAuditRecord` |
| Gateway diagnostics | `diagnostics.ui_control` |
| Snapshot metadata | `metadata.ui_control` |
| Native runtime | `dcc-cua` |

Do not ship aliases, environment promotion, compatibility skills, facade
crates, gateway parsing, redirect pages, or VRS traces for the previous names.
Historical changelog entries and ADR-014 remain unchanged as records, not
runtime contracts.

ADR-020 supersedes the Core-owned native host. Core preserves the normalized
`ui_control__*` contract and delegates native Host, capture, input, and session
ownership to the independently released `dcc-cua` runtime.

## Consequences

### Positive

- one product name spans agent discovery, CLI, configuration, diagnostics, and
  implementation contracts;
- adapters and gateways expose a clean canonical surface without duplicate
  tools.

### Negative

- existing persisted skill names, saved tool slugs, imports, and environment
  variables must be updated before installing this release.

## Migration

1. Change dependencies and persisted skill names from `app-ui` to `ui-control`.
2. Rediscover tools and replace `app_ui__*` slugs with `ui_control__*`.
3. Rename environment variables to `DCC_MCP_UI_CONTROL_*`.
4. Change Python imports to `UiControlPolicy` and `UiControlAuditRecord`.
5. Change Rust dependencies/imports to `dcc-mcp-ui-control` /
   `dcc_mcp_ui_control`.
6. Restart adapters and the per-logon-session UI Control host after environment
   migration.

## References

- [ADR-014](./014-isolate-ui-control-host.md)
- [ADR-015](./015-bounded-ui-control-system-operations.md)
- [UI Control workflows](../guide/ui-control-workflows.md)
