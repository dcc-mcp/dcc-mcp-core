# CLI cheatsheet — DCC-MCP gateway

Default profile: `local`. Remote gateways are selected with
`dcc-mcp-cli gateway set <name>` or one-off `--gateway <name>`.

Primary tool: `dcc-mcp-cli` — the CLI is the **default path for every
shell-capable AI agent**. Native MCP is the fallback for MCP-only clients or an
explicit user choice.

## CLI setup

If `dcc-mcp-cli` is missing, obtain user consent before installing the latest
official release:

```bash
# Linux/macOS
curl -fsSL https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.sh | sh

# Windows PowerShell
powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-core/main/scripts/install-cli.ps1 | iex"
```

Keep an official build current through the release manifest:

```bash
dcc-mcp-cli update check
dcc-mcp-cli update apply
```

`update apply` downloads and stages the latest CLI for the next launch. It does
not update a running `dcc-mcp-server`; update that server in its own environment.

For repository development only, the consent-gated bootstrap/fallback is:

```bash
vx python scripts/dcc_gateway.py --ensure-cli list
```

## Discovery and health

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli dcc-types` | List adapter-backed DCC identifiers from the bundled release catalog without starting a gateway |
| `dcc-mcp-cli dcc-types --catalog path/to/catalog.yml` | Inspect a studio or test catalog through the same typed contract |
| `dcc-mcp-cli list` | Ensure the local loopback gateway, then list local DCC instances from the FileRegistry |
| `dcc-mcp-cli doctor` | Report profile, registry, local inventory, direct-control readiness counts, gateway daemon status, and server binary diagnostics without launching services |
| `dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20` | Search local instances directly through MCP in the `local` profile; this form remains compatible with released CLI builds |
| `dcc-mcp-cli list --gateway pcA` | List DCC instances through a named remote gateway profile |
| `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) | Check gateway liveness; CLI auto-starts loopback gateway targets |
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
| `dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 20` | Find tools with a natural-language phrase |
| `dcc-mcp-cli describe <slug>` | Inspect schema |
| `dcc-mcp-cli call <slug> --json '{"radius":2}' --meta-json '{"agent_context":{"session_id":"task-42"}}'` | Invoke one tool with a stable task-scoped stats identifier |
| `dcc-mcp-cli call <slug> --json '{"radius":2}' --meta-json '{"lease_owner":"workflow-42","agent_context":{"session_id":"task-42"}}'` | Invoke a tool on an instance leased by this workflow |

`dcc-types` reports the release catalog, not running instances. Entries include
their canonical `dcc_type`, adapters, version/source data when available, and
`catalog_install_available`. Unknown/custom DCC identifiers remain valid at the
core boundary even when no catalog install plan exists.

## Post-task evidence

After acceptance, query only the task scope:

```bash
dcc-mcp-cli stats --range 24h --dcc-type maya --session-id task-42
```

Gateway stats are aggregate evidence and may not include direct local calls. A
`total_calls` value of `0` means there is no telemetry evidence. Feed the JSON
plus bounded task and validation summaries to the `review_skill_improvement`
prompt in `dcc-mcp-skills-creator`; do not include raw prompts, secrets, private
paths, or full tool payloads.

## Install and marketplace

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli install --dcc-type maya --version 2026` | Build an auditable adapter install plan with machine-readable `next_steps`, without changing local state |
| `dcc-mcp-cli install --dcc-type maya --version 2026 --python "<mayapy>" --execute` | Execute package install after consent; rolls back on failure and verifies pip/path outputs |
| `dcc-mcp-cli marketplace search --query "maya rigging" --limit 20` | Find installable Skill packages with released and current CLI builds |
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
Marketplace search and inspect do not require a live DCC instance. Always query
the CLI before recommending a marketplace Skill. If the first query is empty,
retry once with fewer capability words or without the DCC filter; never invent
a package name. Inspect the selected package before a consent-gated install or
update.
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
dcc-mcp-cli search --query "create sphere" --dcc-type maya --limit 10

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
| Gateway health fails | Run `dcc-mcp-cli doctor` and inspect the CLI JSON/stderr. Agent-control and endpoint/admin/update commands auto-ensure only loopback gateway targets. For remote profiles or `--base-url`, auto-start is not possible. Ask before installing adapters or launching GUI DCC apps |
| `total == 0` | Start a DCC adapter, then re-run `dcc-mcp-cli list` |
| Listed row is booting or `dispatch_status=unavailable` | Read `direct_control.recommended_next_action` and `direct_control.diagnostics`, then run `dcc-mcp-cli wait-ready --dcc-type <dcc> --instance-id <id>` or `dcc-mcp-cli doctor`; do not call tools until `direct_control.ready=true` |
| `unknown-slug` | Re-run `search`; the instance may have restarted |
| `invalid-params` | Fix the JSON object per `describe` output |
| `instance-leased` / `lease-owner-mismatch` | Pass the exact workflow owner with `--meta-json`, or select another instance; do not guess another owner's value |
