# Discovery without automatic update maintenance

The candidate CLI adds the global `--no-update-maintenance` flag. It bypasses
all three automatic update paths: applying previously staged binary updates,
checking installed marketplace packages, and reading/refreshing the CLI update
cache (including the background check process). No environment variable or
stored user preference is changed. Without the flag, normal automatic update
behavior is preserved; explicitly requested update commands still execute.

Use it with `--no-auto-gateway` for discovery against an existing deployment:

```bash
dcc-mcp-cli --no-update-maintenance --no-auto-gateway --output json list
dcc-mcp-cli --no-update-maintenance --no-auto-gateway --gateway <approved-profile> search --query <intent>
dcc-mcp-cli --no-update-maintenance --no-auto-gateway describe <discovered-slug>
```

This flag controls automatic updates only. It does not make a requested `call`,
installation, update, profile change or gateway launch read-only. Discovery can
still contact the explicitly selected service and read local registry/config
files. `--no-auto-gateway` controls service startup separately. Marketplace
search or another explicitly requested catalog operation may perform its own
documented I/O; the opt-out only removes incidental update maintenance.

Verify artifact capability before invoking this candidate flag. It is not
available merely because a skill references it. Older binaries may reject it;
do not probe unknown older binaries, automatically upgrade them, or assume a
version number establishes the exact binary provenance.

Official v0.20.45 already routes executable help/version and update inspection
through the canonical read-only parser before the legacy startup path:

```bash
dcc-mcp-cli --help
dcc-mcp-cli --version
dcc-mcp-cli --output json update inspect --offline
dcc-mcp-cli --output json update discover --offline --provider official_install_catalog
```

Confirm the selected official artifact implements that gate. Inspect/discover
observe versions/catalog metadata; they do not prove a live DCC lifecycle route.
The new opt-out fills the separate ordinary inventory/search/describe gap.
`DCC_MCP_DISABLE_UPDATE_CHECK` only suppresses the cached refresh path and is
not a substitute for this three-path contract.

Regression tests reuse isolated pending sentinels and private update-cache and
registry directories. They assert ordinary inventory leaves pending bytes and
the cache unchanged, while the existing normal-startup test still quarantines
legacy unsigned staging. No test invokes an installed user CLI or updates it.
