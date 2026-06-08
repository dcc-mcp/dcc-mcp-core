# CLI cheatsheet — DCC-MCP gateway

Base URL: `$DCC_MCP_BASE_URL` (default `http://127.0.0.1:9765`).

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
| `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) | Check gateway liveness |
| `dcc-mcp-cli list` (or `python scripts/dcc_gateway.py list`) | List registered DCC instances |
| `dcc-mcp-cli list --pretty` (or `python scripts/dcc_gateway.py --pretty list`) | Human-readable JSON |

## Capability workflow

| Command | Purpose |
|---------|---------|
| `dcc-mcp-cli search --query sphere --dcc-type maya --limit 20` | Find tools |
| `dcc-mcp-cli describe --tool-slug <slug>` | Inspect schema |
| `dcc-mcp-cli call --tool-slug <slug> --arguments '{"radius":2}'` | Invoke one tool |

## Example: inventory

```bash
export DCC_MCP_BASE_URL="${DCC_MCP_BASE_URL:-http://127.0.0.1:9765}"

# CLI (primary)
dcc-mcp-cli health
dcc-mcp-cli list

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
dcc-mcp-cli describe --tool-slug maya.a1b2c3d4.maya_primitives__create_sphere

# Python fallback
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

## Example: call

```bash
# CLI (primary)
dcc-mcp-cli call --tool-slug maya.a1b2c3d4.maya_primitives__create_sphere \
  --arguments '{"radius":2.0}'

# Python fallback
python scripts/dcc_gateway.py call maya.a1b2c3d4.maya_primitives__create_sphere \
  --json '{"radius":2.0}'
```

## Slug rules

- Gateway slugs are returned by `search`.
- Do not invent slugs from DCC names or tool names.
- Re-run `list` and `search` after a DCC restart.

## Common errors

| Symptom | Action |
|---------|--------|
| CLI not found | Ask user permission, then run `vx python scripts/dcc_gateway.py --ensure-cli list` to download `dcc-mcp-cli`; Python fallback runs if download fails |
| Gateway health fails | Ask before setup; do not install or launch apps silently |
| `total == 0` | Start a DCC adapter, then re-run `dcc-mcp-cli list` |
| `unknown-slug` | Re-run `search`; the instance may have restarted |
| `invalid-params` | Fix the JSON object per `describe` output |
