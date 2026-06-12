# Zero instances — CLI setup guide

Use this document only when:

- `dcc-mcp-cli health` (or `python scripts/dcc_gateway.py health`) succeeds,
- `dcc-mcp-cli list` (or `python scripts/dcc_gateway.py list`) returns `"total": 0`, and
- the user has explicitly approved setup guidance.

Until all three are true, do not run install commands, edit environment files,
launch GUI applications, or modify MCP host configuration.

---

## User consent

Before any setup step, confirm:

1. Which DCC product the user needs.
2. Whether they want commands suggested or executed.
3. That they will confirm after each DCC-side step so you can re-run
   `dcc-mcp-cli list`.

---

## Diagnose

| Check | Meaning | Next step |
|-------|---------|-----------|
| `dcc-mcp-cli health` fails | CLI auto-ensure could not start or reach the local gateway; remote `--base-url` cannot auto-start | Inspect structured CLI output and fix the binary/port/base URL before adapter setup |
| `dcc-mcp-cli health` succeeds and `list.total == 0` | Gateway is up, no DCC registered | Start a DCC adapter |

Gateway commands default to port `9765`. Local `dcc-mcp-cli health`, `list`,
`search`, `describe`, `call`, marketplace, skill, and update commands
auto-ensure a machine-wide gateway daemon before calling REST. Per-DCC
adapters then register with that daemon. The legacy first-wins election is
only for explicit `dcc-mcp-server auto --legacy-gateway-election` setups.

---

## Adapter discovery

With user approval, install adapter packages via the CLI:

```bash
dcc-mcp-cli install --dcc-type maya
dcc-mcp-cli install --dcc-type blender
```

The `install` command returns an auditable plan. Treat it as guidance unless the
user explicitly asks you to execute installation steps.

Alternatively, when the CLI binary is not yet available:

```bash
python scripts/dcc_gateway.py install --dcc-type maya
python scripts/dcc_gateway.py install --dcc-type blender
```

The Python fallback auto-downloads the CLI if needed (with user consent, pass
`--ensure-cli`), then delegates to `dcc-mcp-cli install`.

---

## Per-DCC checklist

### Maya

1. Install into Maya's Python: `mayapy -m pip install dcc-mcp-maya`
2. In Maya Script Editor:

   ```python
   import dcc_mcp_maya
   handle = dcc_mcp_maya.start_server(port=8765)
   print(handle.mcp_url())
   ```

3. Re-run `dcc-mcp-cli list`; expect `dcc_type: maya`.

### Blender

1. Install `dcc-mcp-blender` per its README.
2. Enable the add-on in Blender Preferences.
3. Re-run `dcc-mcp-cli list`; expect `dcc_type: blender`.

### Houdini / Photoshop / 3ds Max

Follow the adapter README for the target host, then re-run:

```bash
dcc-mcp-cli list
```

When `total >= 1`, resume the main flow: `search -> describe -> call`.

If Python is not available as `python` / `py`, install vx first:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```
