---
name: photoshop-setup
description: Adobe Photoshop MCP setup — stage and load the adobepy UXP bridge, configure clients, and verify the full server
  connection
license: MIT
allowed-tools:
- Bash
- Read
- Write
metadata:
  dcc-mcp:
    dcc: photoshop
    version: 0.1.0
    layer: infrastructure
    tags:
    - photoshop
    - setup
    - install
    - configure
    - bridge
    - uxp
    - adobe
    search-hint: install setup configure bridge uxp plugin server verify photoshop mcp
    tools: tools.yaml
---
# photoshop-setup

Compatibility and diagnostics skill for dcc-mcp-photoshop. The canonical
installer is `dcc-mcp-photoshop install --json`; this skill must not create a
second staging, receipt, rollback, or uninstall implementation.

All lifecycle verbs use the same machine-readable contract and flags:
`install`, `status`, `verify`, `uninstall`, and `upgrade` with `--json`,
`--yes`, `--dry-run`, `--dcc-path`, and `--python` as applicable. The adapter
requires `adobepy==0.6.2` and `dcc-mcp-core>=0.20.14,<0.21.0`; the selected
interpreter is recorded and checked against those bounds before mutation.

## Distribution Channels

| Channel | Command / Artifact | Python Required |
|---------|-------------------|-----------------|
| **pip** | `pip install dcc-mcp-photoshop` | Yes |
| **Standalone binary** | GitHub Releases — platform-specific binary | No |
| **adobepy bridge** | Managed by `dcc-mcp-photoshop install --json` | No |

## Workflow

1. Run `dcc-mcp-cli doctor` to inspect the local CLI and gateway.
2. For an Internal deployment with an approved prebuilt bridge, use the
   approved studio deployment tooling to create the Adobe debug-plugin link.
   Skip `dcc-mcp-photoshop install --json --yes`; the current shared CLI does
   not accept undocumented bridge source or debug-root options. Restart
   Photoshop and wait for the bridge to load before continuing.
3. For a non-Internal deployment, review and execute the adapter-owned plan:
   `dcc-mcp-photoshop install --json --dry-run`, then
   `dcc-mcp-photoshop install --json --yes`.
4. Run `dcc-mcp-cli list`,
   `dcc-mcp-cli wait-ready --dcc-type photoshop`, then
   `dcc-mcp-photoshop verify --json`; only a real Photoshop RPC is usable.

A link on disk is not proof that Photoshop loaded the bridge. The shared CLI
does not currently provide the Internal bridge-link operation.

Do not treat copied bridge files, a green package install, a mock broker, or
release metadata as host verification. `verify.directly_usable` becomes true
only after the authenticated broker and a typed RPC prove the exact Photoshop
process, UXP bridge origin, and session identity. Failures remain fail-closed;
never print tokens or raw subprocess diagnostics into skill results.

## Tools

- `check_environment` — Python / pip / installed packages / Photoshop process / adobepy broker
- `install_package` — pip install dcc-mcp-photoshop
- `setup_uxp_plugin` — Legacy compatibility redirect to the canonical CLI
- `start_server` — Start the MCP adapter after the adobepy broker is ready
- `verify_connection` — End-to-end connection check
- `configure_mcp_client` — Write MCP client config for Claude Desktop / Cursor / VS Code
