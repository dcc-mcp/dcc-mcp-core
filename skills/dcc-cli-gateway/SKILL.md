---
name: dcc-cli-gateway
description: >-
  Default unified entry for agents and headless CLI hosts (OpenClaw, Hermes,
  Codex CLI, CI bots, custom agent runtimes) to control live DCC applications
  through dcc-mcp-cli local registry/direct MCP or remote gateway REST — not
  native MCP JSON-RPC. Agents use this skill plus shell; IDE users (Cursor,
  Claude Desktop, VS Code MCP) should configure the gateway MCP URL instead.
  Inventory DCC instances and search/describe/call tools via CLI. If
  dcc-mcp-cli is missing, ask for consent, download from GitHub Releases, and
  fall back to Python stdlib gateway REST only if download fails.
license: MIT-0
compatibility: Cross-platform Windows/macOS/Linux. Prefers dcc-mcp-cli on PATH; can download release asset from GitHub; local profile needs no gateway env. DCC_MCP_BASE_URL is optional for remote/legacy gateway REST fallback.
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "0.18.20"  # x-release-please-version
    search-hint: "cli gateway dcc-mcp-cli connect dcc instances search describe call clawhub"
    tags: "cli, gateway, infrastructure, clawhub, openclaw, instances"
  openclaw:
    emoji: "🖥️"
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/dcc-cli-gateway/SKILL.md
---

# DCC CLI Gateway — Agent Control Plane

> **Agents use `dcc-mcp-cli`; IDE users keep native MCP.** One skill for
> shell-capable agent hosts — no MCP connector required.

Use this skill when an **agent or headless CLI host** can run shell commands and
should control DCC-MCP through **`dcc-mcp-cli`**. The CLI uses local
FileRegistry + direct per-DCC MCP in the built-in `local` profile, and gateway
REST (`/v1/search`, `/v1/describe`, `/v1/call`) for named remote profiles.

The CLI returns JSON by default. The bundled Python fallback is gateway-REST
only and sends `Accept: application/json` because the gateway REST API itself
now defaults to compact TOON for agent-facing routes.

---

## Agent Path vs IDE Path

DCC-MCP supports two integration paths. Pick the one that matches how the user
works — do not force IDE users onto the CLI, and do not ask agents to configure
MCP when they can run shell.

| Dimension | **Agent path** (this skill) | **IDE path** (native MCP) |
|-----------|----------------------------|---------------------------|
| **Who** | OpenClaw, Hermes, Codex CLI, CI bots, custom agent runtimes, any host with shell | Cursor, Claude Desktop, VS Code MCP, other MCP-native clients |
| **Transport** | `dcc-mcp-cli` → local MCP or remote gateway REST | MCP Streamable HTTP → gateway `/mcp` |
| **Discovery surface** | `search` → `describe` → `call` via CLI or bundled Python helper | Gateway MCP tools: `search`, `describe`, `load_skill`, `call` |
| **Setup** | Install this skill; optional `dcc-mcp-cli` on `PATH` or `--ensure-cli` with consent | Add gateway URL to IDE MCP settings (see repo `docs/guide/*`) |
| **When to choose** | Host has no MCP connector, runs headless, or studio wants one forkable skill | User already works inside an IDE with MCP configured |
| **Resources / prompts** | Not covered here; use REST `/v1/context` or IDE MCP if needed | `resources/read`, `prompts/get`, SSE subscribe via MCP |

**Decision rules for agents loading this skill:**

1. **Use this skill (CLI path)** when the host can execute shell and the task is
   DCC control (`search` → `describe` → `call`). This is the **default for agents**.
2. **Do not use this skill** when the user is in Cursor / Claude Desktop / VS Code
   with gateway MCP already configured — point them to their IDE MCP workflow instead.
3. **Do not mix paths in one turn** — pick CLI+REST or MCP for the whole task, not both.
4. **Zero instances** — stop, explain, ask consent before bootstrap; see
   [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md).

Internal studios can fork this skill once and reuse the same CLI+REST workflow across
agents without maintaining per-host MCP server lists.

---

## Gateway Profiles And Local-First Inventory

`dcc-mcp-cli` has a built-in `local` profile. In local mode, `list` reads the
core default FileRegistry directly, and `search`, `describe`, `load-skill`,
`call`, `wait-ready`, and guarded `stop-instance` talk to the selected local
DCC instance's advertised MCP/readyz/safe-stop endpoints. Local control does
not require or auto-start a gateway. Remote machines are selected through named
gateway profiles:

Treat `list` as inventory plus diagnostics, not proof that a row is callable.
It intentionally keeps live `booting` / `dispatch_status=unavailable` sidecar
rows visible. Local `search`, `describe`, `load-skill`, `call`, and
`reload-skills` route only to rows ready for local CLI control. Per-DCC sidecar
rows become local MCP routes once they report `dispatch_status=ready`; before
that, they remain visible for diagnostics. Use `wait-ready` or `doctor` when a
listed instance is still booting.

```bash
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli gateway set pcA
dcc-mcp-cli gateway set local
dcc-mcp-cli list --gateway pcA
```

Use `--gateway <name>` to override the current profile for one command.
`--base-url` / `DCC_MCP_BASE_URL` remain direct endpoint overrides for legacy
scripts and smoke checks.

Endpoint-level commands such as `health`, `update`, and `smoke` without an
explicit `--url` still auto-ensure loopback HTTP gateway targets. Local
instance control commands, file-only commands, and explicit lifecycle commands
do not auto-start the gateway.
When startup state is unclear, run `dcc-mcp-cli doctor` before troubleshooting
adapters. It reports profile config/current selection, the registry directory
and local inventory, direct-control readiness counts, gateway daemon status, and
server binary path/source/version without launching or downloading anything.
When `list` shows local rows, prefer `direct_control.recommended_next_action`
over guessing from status text; sidecar rows are local tool-call routes only
after `direct_control.ready=true`. If `direct_control.ready=false`, inspect
`direct_control.diagnostics.failure_stage`, `failure_reason`, `host_rpc_*`, and
any `diagnostics.logs.*` paths before retrying. `doctor` summarizes the same
not-ready rows under `local.inventory.direct_control.not_ready_instances`.

### What auto-ensure does

1. **Probe** `GET /health` on the gateway port.
2. If healthy -> continue.
3. If unreachable -> acquire a launch lock, spawn the gateway daemon in the
   background, poll until healthy, then release the lock.
4. Run the original endpoint-level command.

### CLI usage

```bash
# Local inventory - no gateway required
dcc-mcp-cli list

# Startup diagnostics - no service launch or download
dcc-mcp-cli doctor

# Gateway health check - auto-starts loopback gateway if needed
dcc-mcp-cli health

# Disable auto-start for one command
dcc-mcp-cli --no-auto-gateway health

# Explicit daemon lifecycle
dcc-mcp-cli gateway daemon start
dcc-mcp-cli gateway daemon status
dcc-mcp-cli gateway daemon restart

# Set longer auto-start wait timeout
dcc-mcp-cli --auto-gateway-timeout-secs 30 health
```

`gateway daemon start` and `gateway daemon restart` are the durable operator
paths: their start phase passes `--gateway-idle-timeout-secs 0` by default so an
explicitly managed local daemon does not exit just because no DCC backend is
registered yet.

### Explicit ensure result format

```json
{
  "host": "127.0.0.1",
  "port": 9765,
  "already_running": true,
  "pid": 12345
}
```

- `already_running: true` -> gateway was already up; proceed to endpoint-level gateway commands.
- `already_running: false` -> gateway was just started by this call (includes `pid`).

### If auto-ensure fails

| Symptom | Likely cause | Action |
|---------|-------------|--------|
| Timeout after `--auto-gateway-timeout-secs` | Gateway binary missing or port conflict | Ask user to install dcc-mcp-core or check port availability |
| Lock contention | Concurrent launch race | Retry after a short delay |
| Port 0 rejected | Invalid config | Verify `DCC_MCP_GATEWAY_PORT` or `--port` is non-zero |
| Remote profile or `DCC_MCP_BASE_URL` unreachable | Auto-start only applies to local loopback HTTP URLs | Report the remote gateway as unreachable |

### Python fallback note

The Python fallback (`dcc_gateway.py`) does NOT include a `gateway ensure` command
because the ensure flow spawns a subprocess daemon - a capability specific to the
compiled CLI. When the CLI binary is unavailable, skip to `dcc_gateway.py health`
directly. If health fails, report the gateway as unreachable and ask the user to
start a gateway manually or install `dcc-mcp-cli`.

---

## Connection Order

1. Use `dcc-mcp-cli list` for local inventory, or `dcc-mcp-cli list --gateway <name>` for a remote profile.
2. Use `dcc-mcp-cli` for all subsequent commands when it is on `PATH`.
3. If missing, ask user permission, then download `dcc-mcp-cli` from GitHub Releases.
4. If the download fails, use the bundled Python stdlib REST fallback.

Install via OpenClaw/ClawHub, or point your agent at this `SKILL.md` after cloning
[`dcc-mcp-core/skills/dcc-cli-gateway/`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/skills/dcc-cli-gateway).

---

## Critical Rules

| Situation | You MUST |
|-----------|----------|
| **Starting any local DCC task** | Run `dcc-mcp-cli list`; it reads the local FileRegistry without requiring a gateway |
| **Startup state is ambiguous** | Run `dcc-mcp-cli doctor`; inspect selected profile, registry dir, local inventory, direct-control readiness counts, daemon status, and server binary diagnostics |
| **Starting any remote DCC task** | Select or override a profile with `dcc-mcp-cli gateway set <name>` or `dcc-mcp-cli list --gateway <name>` |
| `dcc-mcp-cli` missing | Ask permission before `--ensure-cli`; fallback Python REST is allowed if download fails |
| CLI auto-ensure fails | Stop; explain the result; do not run gateway endpoint commands such as `health`, `update`, or `smoke` |
| Inventory returns `total == 0` | Stop; do not run `search`, `describe`, or `call` |
| Remote gateway unreachable | Stop; explain; ask user permission before troubleshooting |
| User has not agreed to setup | Do not install packages, edit env files, launch GUI apps, or write configs |
| User approved setup | Follow [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md) |
| After DCC crash/restart | Re-run `list` and `search`; old slugs may be invalid |

---

## Configuration

`dcc-mcp-cli` stores remote gateway profiles under `~/.dcc-mcp/gateway-profiles.json`.
The Python helper still reads a gateway URL from `DCC_MCP_BASE_URL`.

```bash
dcc-mcp-cli list
dcc-mcp-cli gateway register https://workstation.example:19293 --name pcA
dcc-mcp-cli gateway list
dcc-mcp-cli list --gateway pcA
python scripts/dcc_gateway.py --base-url http://127.0.0.1:9765 health
```

For a one-off command:

```bash
python scripts/dcc_gateway.py --base-url http://127.0.0.1:9765 health
```

Quick probe helper:

```bash
python3 scripts/check_cli.py
py -3 scripts\check_cli.py
```

Flags: `--base-url URL`, `--cli dcc-mcp-cli`, `--ensure-cli`, `--install-dir DIR`, `--pretty`.

When the user approves downloading the CLI:

```bash
# Linux / macOS
python3 scripts/dcc_gateway.py --ensure-cli list
vx python scripts/dcc_gateway.py --ensure-cli list

# Windows
py -3 scripts\dcc_gateway.py --ensure-cli list
vx python scripts\dcc_gateway.py --ensure-cli list
```

Release assets are selected by platform:

| Platform | Asset |
|----------|-------|
| Windows x86_64 | `dcc-mcp-cli-windows-x86_64.exe` |
| Linux x86_64 | `dcc-mcp-cli-linux-x86_64` |
| macOS Intel/Apple Silicon | `dcc-mcp-cli-macos-universal2` |

If Python is not easy to locate, install vx first and run the helper through
`vx python`:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```

---

## Step 0 — Local Inventory First

Run this as the **very first step** every time you begin local work or after a
DCC adapter restarts:

```bash
# Local FileRegistry inventory
dcc-mcp-cli list

# No-launch startup diagnostics when state is unclear
dcc-mcp-cli doctor

# Optional gateway health check
dcc-mcp-cli health
```

Interpret the result:

- `list.total > 0` -> inspect status/dispatch metadata. Local `search`, `describe`, `load-skill`, `call`, and `reload-skills` only route to rows ready for local CLI control; use `wait-ready` or `doctor` for live-but-booting rows, including sidecars that have not reached `dispatch_status=ready`.
- `doctor.profile.selected.mode` / `doctor.local.registry_dir` -> confirms which local/remote mode and registry path the CLI is using before adapter setup.
- `health.status == "ok"` -> gateway is up when you need gateway endpoint/admin/update workflows.
- Error / timeout -> stop; explain the failure to the user. For remote
  profiles, the CLI cannot auto-start the gateway.

---

## Step 1 — Mandatory Instance Inventory

Run this every time the user starts/stops a DCC host:

```bash
# CLI (primary)
dcc-mcp-cli list
dcc-mcp-cli health

# Python fallback (when CLI is unavailable)
python scripts/dcc_gateway.py health
python scripts/dcc_gateway.py list
```

Interpret `dcc-mcp-cli list`:

```json
{
  "total": 1,
  "instances": [
    {
      "instance_id": "full-uuid",
      "instance_short": "a1b2c3d4",
      "dcc_type": "maya",
      "status": "available",
      "stale": false,
      "mcp_url": "http://127.0.0.1:8765/mcp"
    }
  ]
}
```

Report to the user:

1. `total`
2. Count by `dcc_type`
3. Any `stale: true` rows
4. The target `instance_id` or `instance_short` you will use

If `total == 0`, stop and ask whether the user wants setup guidance for the
target DCC. Continue only after explicit approval.

---

## Step 2 — Search Tools

Only run this when inventory shows at least one non-stale target:

```bash
# CLI (primary)
dcc-mcp-cli search --query sphere --dcc-type maya --limit 20

# Python fallback
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 20
```

Copy the returned slug exactly. Local and gateway slugs use the same
agent-facing shape:

```text
maya.a1b2c3d4.maya_primitives__create_sphere
```

Never hand-build slugs.

---

## Step 3 — Describe Schema

```bash
# CLI (primary)
dcc-mcp-cli describe maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

Read `tool.inputSchema` and safety annotations before calling.

---

## Step 4 — Call a Tool

```bash
# CLI (primary)
dcc-mcp-cli call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

Tool-specific fields (`code`, `file_path`, `radius`, and similar) belong inside
the `--json` object. Do not pass them as top-level CLI flags unless the CLI adds
an explicit first-class flag later.

See [`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md) for command
patterns and common errors.

---

## Updates and Marketplace Maintenance

Use the gateway update manifest for binary checks:

```bash
# Check whether the local CLI has an update.
dcc-mcp-cli update check

# Check a server/instance version shown in the admin panel.
dcc-mcp-cli update check --binary dcc-mcp-server --current-version 0.18.16

# Stage a CLI binary update for the next CLI launch.
dcc-mcp-cli update apply
```

`dcc-mcp-cli update apply` only stages the CLI binary. To update a running
server binary, run the server-side command in that server environment:

```bash
dcc-mcp-server update check
dcc-mcp-server update apply
```

Use marketplace commands for skills:

```bash
dcc-mcp-cli marketplace search --query rigging --dcc maya --limit 20
dcc-mcp-cli marketplace inspect <package_name>
dcc-mcp-cli marketplace install <package_name> --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
dcc-mcp-cli marketplace outdated --dcc maya
dcc-mcp-cli marketplace update <package_name> --dcc maya
dcc-mcp-cli reload-skills --dcc-type maya
```

After installing or updating skills, first run
`dcc-mcp-cli reload-skills --dcc-type <dcc>` so running adapters re-scan the
marketplace skill path. Then use `dcc-mcp-cli load-skill` for a live instance
when the adapter has not auto-loaded the skill yet.

Use `install` for adapter plans, not marketplace skills:

```bash
dcc-mcp-cli install --dcc-type maya --version 2026
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe"
dcc-mcp-cli install --dcc-type maya --version 2026 --python "C:/Program Files/Autodesk/Maya2026/bin/mayapy.exe" --execute
```

Agents must ask before using `--execute`. The executor prompts for consent,
rolls back completed steps if a later step fails, verifies pip packages with
`pip show`, and verifies git/zip/path installs by checking their target path.
Package install is not online registration: the DCC plugin or sidecar must
start and remain alive before `dcc-mcp-cli list` shows an instance. Treat the
install JSON `next_steps` array as the authoritative machine-readable follow-up
sequence. If it includes `read-install-instructions`, read that adapter
repository's raw `install.md` first; it owns host-specific setup. Then
start/enable the host plugin, run `doctor`, confirm `list`, wait for readiness,
search/call tools, and use marketplace `search`, `inspect`, `install`, then
`reload-skills` for optional community skill packages.

If `install_policy.auto_install_enabled` is `false`, do not retry with
`--execute`. Show the returned `install_policy.prompt` to the user and hand off
to the named Pipeline TD / studio deployment path. Studios set this through
`DCC_MCP_INSTALL_DISABLED=1` and `DCC_MCP_INSTALL_DISABLED_PROMPT`.

---

## What This Skill Does Not Use

- Native MCP `tools/list`, `tools/call`, or `resources/read` on the agent host
  (IDE users should use MCP instead of this skill)
- Raw `curl` workflows except when debugging the gateway itself
- Direct Maya/Blender/Houdini scripting

The CLI is the **default agent-facing control plane**. The Python fallback uses
the same gateway REST endpoints only when the CLI is unavailable after a
download attempt fails. The gateway still serves MCP for IDE clients in parallel;
choosing this skill does not replace or disable the IDE MCP path.
