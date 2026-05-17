# Zero instances — CLI setup guide

Use this document only when:

- `python scripts/dcc_gateway.py health` succeeds,
- `python scripts/dcc_gateway.py list` returns `"total": 0`, and
- the user has explicitly approved setup guidance.

Until all three are true, do not run install commands, edit environment files,
launch GUI applications, or modify MCP host configuration.

---

## User consent

Before any setup step, confirm:

1. Which DCC product the user needs.
2. Whether they want commands suggested or executed.
3. That they will confirm after each DCC-side step so you can re-run
   `python scripts/dcc_gateway.py list`.

---

## Diagnose

| Check | Meaning | Next step |
|-------|---------|-----------|
| `python scripts/dcc_gateway.py health` fails | Gateway is not reachable | Ask user to start a gateway-capable DCC adapter or `dcc-mcp-server` |
| `python scripts/dcc_gateway.py health` succeeds and `list.total == 0` | Gateway is up, no DCC registered | Start a DCC adapter |

Gateway election defaults to port `9765`. The first DCC-MCP process that binds
the gateway port becomes the gateway; other DCC adapters register with it.

---

## Adapter discovery

With user approval:

```bash
dcc-mcp-cli install --dcc-type maya
dcc-mcp-cli install --dcc-type blender
```

The `install` command returns an auditable plan. Treat it as guidance unless the
user explicitly asks you to execute installation steps.

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

3. Re-run `python scripts/dcc_gateway.py list`; expect `dcc_type: maya`.

### Blender

1. Install `dcc-mcp-blender` per its README.
2. Enable the add-on in Blender Preferences.
3. Re-run `python scripts/dcc_gateway.py list`; expect `dcc_type: blender`.

### Houdini / Photoshop / 3ds Max

Follow the adapter README for the target host, then re-run:

```bash
python scripts/dcc_gateway.py list
```

When `total >= 1`, resume the main flow: `search -> describe -> call`.

If Python is not available as `python` / `py`, install vx first:

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/loonghao/vx/main/install.sh | bash

# Windows PowerShell
powershell -c "irm https://raw.githubusercontent.com/loonghao/vx/main/install.ps1 | iex"
```
