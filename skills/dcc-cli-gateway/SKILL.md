---
name: dcc-cli-gateway
description: >-
  Control live DCC hosts (Maya, Blender, Houdini, Photoshop, 3ds Max, and
  custom studio tools) through the dcc-mcp-cli command line. For ClawHub,
  OpenClaw, Cursor, Claude, and shell-capable agent hosts: verify gateway
  health, inventory registered DCC instances, search tools, inspect schemas,
  and invoke tools without speaking MCP directly. If dcc-mcp-cli is missing,
  ask for consent, download it from GitHub Releases, and fall back to Python
  stdlib REST only if download fails.
license: MIT-0
compatibility: Cross-platform Windows/macOS/Linux. Prefers dcc-mcp-cli on PATH; can download release asset from GitHub; Python 3.7+ stdlib REST fallback. DCC-MCP gateway reachable at DCC_MCP_BASE_URL (default http://127.0.0.1:9765)
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "0.17.44"  # x-release-please-version
    search-hint: "cli gateway dcc-mcp-cli connect dcc instances search describe call clawhub"
    tags: "cli, gateway, infrastructure, clawhub, openclaw, instances"
  openclaw:
    requires:
      env:
        - DCC_MCP_BASE_URL
    primaryEnv: DCC_MCP_BASE_URL
    emoji: "🖥️"
    homepage: https://github.com/loonghao/dcc-mcp-core/blob/main/skills/dcc-cli-gateway/SKILL.md
---

# DCC CLI Gateway — Agent Control Plane

Use this skill when an agent host can run shell commands and should connect to
DCC-MCP through **`dcc-mcp-cli`** instead of MCP JSON-RPC. The CLI wraps the
gateway REST API and returns JSON by default. The bundled Python fallback sends
`Accept: application/json` because the gateway REST API itself now defaults to
compact TOON for agent-facing routes.

Connection order:

1. Use `dcc-mcp-cli` when it is already on `PATH`.
2. If missing, ask user permission, then download `dcc-mcp-cli` from GitHub Releases.
3. If the download fails, use the bundled Python stdlib REST fallback.

Install via OpenClaw/ClawHub, or point your agent at this `SKILL.md` after cloning
[`dcc-mcp-core/skills/dcc-cli-gateway/`](https://github.com/loonghao/dcc-mcp-core/tree/main/skills/dcc-cli-gateway).

---

## Critical Rules

| Situation | You MUST |
|-----------|----------|
| Starting any DCC task | Run `python scripts/dcc_gateway.py health` and `python scripts/dcc_gateway.py list` first |
| `dcc-mcp-cli` missing | Ask permission before `--ensure-cli`; fallback Python REST is allowed if download fails |
| Inventory returns `total == 0` | Stop; do not run `search`, `describe`, or `call` |
| Gateway unreachable | Stop; explain; ask user permission before troubleshooting |
| User has not agreed to setup | Do not install packages, edit env files, launch GUI apps, or write configs |
| User approved setup | Follow [`references/ZERO_INSTANCES_CLI.md`](references/ZERO_INSTANCES_CLI.md) |
| After DCC crash/restart | Re-run `list` and `search`; old slugs may be invalid |

---

## Configuration

`dcc-mcp-cli` and the Python helper read the gateway URL from `DCC_MCP_BASE_URL`.

```bash
export DCC_MCP_BASE_URL="${DCC_MCP_BASE_URL:-http://127.0.0.1:9765}"
dcc-mcp-cli health
python scripts/dcc_gateway.py health
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

## Step 0 — Mandatory Instance Inventory

Run this every time you begin work or after the user starts/stops a DCC host:

```bash
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

## Step 1 — Search Tools

Only run this when inventory shows at least one non-stale target:

```bash
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 20
```

Copy the returned slug exactly. Gateway slugs look like:

```text
maya.a1b2c3d4.maya_primitives__create_sphere
```

Never hand-build slugs.

---

## Step 2 — Describe Schema

```bash
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

Read `tool.inputSchema` and safety annotations before calling.

---

## Step 3 — Call a Tool

```bash
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

Tool-specific fields (`code`, `file_path`, `radius`, and similar) belong inside
the `--json` object. Do not pass them as top-level CLI flags unless the CLI adds
an explicit first-class flag later.

See [`references/CLI_CHEATSHEET.md`](references/CLI_CHEATSHEET.md) for command
patterns and common errors.

---

## What This Skill Does Not Use

- MCP `tools/list`, `tools/call`, or `resources/read`
- Raw `curl` workflows except when debugging the gateway itself
- Direct Maya/Blender/Houdini scripting

The CLI is the preferred agent-facing control plane. The Python fallback uses
the same gateway REST endpoints only when the CLI is unavailable after a
download attempt fails.
