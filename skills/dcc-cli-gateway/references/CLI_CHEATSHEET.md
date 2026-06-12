# CLI cheatsheet — DCC-MCP gateway

Default profile: `local`. Remote gateways are selected with
`dcc-mcp-cli gateway set <name>` or one-off `--gateway <name>`.

Primary tool: `dcc-mcp-cli` — the CLI is the **default path for AI agents**.
Fallback: `python scripts/dcc_gateway.py` when the CLI binary is unavailable.

## CLI setup

`dcc-mcp-cli` ships with the gateway. If missing, ask user permission then
ensure it:

```bash
vx python scripts/dcc_gateway.py --ensure-cli list
```

If the CLI binary is not yet on `$PATH`, the gateway downloads it from GitHub
Releases and places it under `~/.dcc-mcp/bin/`. Add this directory to `$PATH`
for direct `dcc-mcp-cli` access.

If download fails, the Python fallback runs automatically.

If Python is not available as `python` / `py`, install vx first:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```

## Discovery and health

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli list` | List local DCC instances from the FileRegistry; no gateway required |
| `dcc-mcp-cli doctor` | Report profile, registry, local inventory, direct-control readiness counts, gateway daemon status, and server binary diagnostics without launching services |
| `dcc-mcp-cli search --query sphere --dcc-type maya --limit 20` | Search local instances directly through MCP in the `local` profile |
| `dcc-mcp-cli list --gateway pcA` | List DCC instances through a named remote gateway profile |
| `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) | Check gateway liveness; CLI auto-starts only loopback gateway targets |
| `dcc-mcp-cli gateway register https://host:19293 --name pcA` | Persist a named remote gateway profile |
| `dcc-mcp-cli gateway list` | Inspect configured remote profiles and the active selection |
| `dcc-mcp-cli gateway set pcA` / `dcc-mcp-cli gateway set local` | Switch active gateway profile |
| `dcc-mcp-cli gateway daemon start` | Start the explicit local machine-wide daemon; default idle timeout is `0`, so it stays alive with no DCC backend |
| `dcc-mcp-cli gateway daemon restart` | Stop the pidfile-tracked daemon, then start it again with the same persistent default |
| `dcc-mcp-cli gateway daemon stop` | Stop the pidfile-tracked local daemon |
| `dcc-mcp-cli gateway daemon status` | Explicit local daemon lifecycle check with registry dir, PID file, health URL, and CLI version |
| `dcc-mcp-cli list --pretty` (or `python scripts/dcc_gateway.py --pretty list`) | Human-readable JSON |

## Capability workflow

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli search --query sphere --dcc-type maya --limit 20` | Find tools |
| `dcc-mcp-cli describe <slug>` | Inspect schema |
| `dcc-mcp-cli call <slug> --json '{"radius":2}'` | Invoke one tool |

## Install and marketplace

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli install --dcc-type maya --version 2026` | Build an auditable adapter install plan with machine-readable `next_steps`, without changing local state |
| `dcc-mcp-cli install --dcc-type maya --version 2026 --python "<mayapy>" --execute` | Execute package install after consent; rolls back on failure and verifies pip/path outputs |
| `dcc-mcp-cli marketplace search --query rigging --dcc maya --limit 20` | Find installable skill packages |
| `dcc-mcp-cli marketplace inspect <package_name>` | Inspect the selected skill package metadata before installing |
| `dcc-mcp-cli marketplace install <package_name> --dcc maya` | Install a skill package into the local marketplace root |
| `dcc-mcp-cli reload-skills --dcc-type maya` | Ask running Maya adapters to re-scan installed skill paths |
| `dcc-mcp-cli marketplace update <package_name> --dcc maya` | Update an installed skill package from the catalog |

After adapter package install, follow the plan's `next_steps`: read the
adapter-maintained `install.md` when `read-install-instructions` is present,
start or enable the DCC host plugin, run `doctor`, and confirm the sidecar
self-registered with `dcc-mcp-cli list`.
If `install_policy.auto_install_enabled=false`, stop and show
`install_policy.prompt`; the studio pipeline owns adapter deployment.
`list` keeps live diagnostic rows visible; `search`, `describe`, `load-skill`,
`call`, and `reload-skills` only route to rows ready for local CLI control. A
per-DCC sidecar row is routable once `direct_control.ready=true`; if a row is
booting or `dispatch_status=unavailable`, inspect
`direct_control.diagnostics.failure_stage`, `failure_reason`, `host_rpc_*`, and
any log paths, then run `wait-ready` or `doctor` before calling tools.
After marketplace skill search, inspect the selected package before installing.
After installing or updating marketplace skills, run `reload-skills`, then use
`load-skill` if the adapter has not auto-loaded the new skill.

## Example: inventory

```bash
# CLI (primary)
dcc-mcp-cli list
dcc-mcp-cli health

# Python fallback (when CLI is unavailable)
python scripts/dcc_gateway.py health
python scripts/dcc_gateway.py list
```

## Example: search

```bash
# CLI (primary)
dcc-mcp-cli search --query sphere --dcc-type maya --limit 10

# Python fallback
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 10
```

## Example: describe

```bash
# CLI (primary)
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

## Example: call

```bash
# CLI (primary)
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

## Slug rules

- Slugs are returned by `search`; local and remote modes use the same
  `dcc.instance.tool` shape.
- Do not invent slugs from DCC names or tool names.
- Re-run `list` and `search` after a DCC restart.

## Common errors

| Symptom | Action |
|---------|--------|
| CLI not found | Ask user permission, then run `vx python scripts/dcc_gateway.py --ensure-cli list` to download `dcc-mcp-cli`; Python fallback runs if download fails |
| Gateway health fails | Run `dcc-mcp-cli doctor` and inspect the CLI JSON/stderr. Local instance control does not require gateway; endpoint/admin/update commands auto-ensure only loopback gateway targets. For remote profiles or `--base-url`, auto-start is not possible. Ask before installing adapters or launching GUI DCC apps |
| `total == 0` | Start a DCC adapter, then re-run `dcc-mcp-cli list` |
| Listed row is booting or `dispatch_status=unavailable` | Read `direct_control.recommended_next_action` and `direct_control.diagnostics`, then run `dcc-mcp-cli wait-ready --dcc-type <dcc> --instance-id <id>` or `dcc-mcp-cli doctor`; do not call tools until `direct_control.ready=true` |
| `unknown-slug` | Re-run `search`; the instance may have restarted |
| `invalid-params` | Fix the JSON object per `describe` output |
