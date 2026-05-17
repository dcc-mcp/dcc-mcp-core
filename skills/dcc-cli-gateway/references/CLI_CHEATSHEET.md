# CLI cheatsheet — DCC-MCP gateway

Base URL: `$DCC_MCP_BASE_URL` (default `http://127.0.0.1:9765`).

Preferred helper:

```bash
python scripts/dcc_gateway.py <command>
vx python scripts/dcc_gateway.py <command>
```

It uses `dcc-mcp-cli` when available. With user consent, add `--ensure-cli` to
download the CLI from GitHub Releases when missing. If download fails, it falls
back to Python stdlib REST.

If Python is not available as `python` / `py`, install vx and use
`vx python scripts/dcc_gateway.py ...`:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```

## Discovery and health

| Command | Purpose |
|---------|---------|
| `python scripts/dcc_gateway.py health` | Check gateway liveness |
| `python scripts/dcc_gateway.py list` | List registered DCC instances |
| `python scripts/dcc_gateway.py --pretty list` | Human-readable JSON |

## Capability workflow

| Command | Purpose |
|---------|---------|
| `python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 20` | Find tools |
| `python scripts/dcc_gateway.py describe <tool_slug>` | Inspect schema |
| `python scripts/dcc_gateway.py call <tool_slug> --json '{"radius":2}'` | Invoke one tool |

## Example: inventory

```bash
export DCC_MCP_BASE_URL="${DCC_MCP_BASE_URL:-http://127.0.0.1:9765}"
python scripts/dcc_gateway.py health
python scripts/dcc_gateway.py list
```

## Example: search

```bash
python scripts/dcc_gateway.py search --query sphere --dcc-type maya --limit 10
```

## Example: describe

```bash
python scripts/dcc_gateway.py describe maya.a1b2c3d4.maya_primitives__create_sphere
```

## Example: call

```bash
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
| CLI not found | Ask user permission, then run `vx python scripts/dcc_gateway.py --ensure-cli list`; fallback Python REST runs if download fails |
| Gateway health fails | Ask before setup; do not install or launch apps silently |
| `total == 0` | Start a DCC adapter, then re-run `python scripts/dcc_gateway.py list` |
| `unknown-slug` | Re-run `search`; the instance may have restarted |
| `invalid-params` | Fix the JSON object per `describe` output |
