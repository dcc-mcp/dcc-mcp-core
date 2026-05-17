# Zero instances — user-consent setup guide

Use this document only when:

- `GET /v1/healthz` succeeds (gateway HTTP is up), **and**
- `GET /v1/instances` returns `"total": 0`, **and**
- The user has **explicitly approved** help with environment setup.

Until all three are true, do **not** run install commands or change the user's machine.

---

## User consent (mandatory)

Before any step below, confirm:

1. Which DCC product the user needs (Maya, Blender, Houdini, Photoshop, 3ds Max, …).
2. That they allow you to **suggest** commands (they run installs and launch apps themselves unless they ask you to run shell).
3. That they will tell you when a step is done so you can re-poll `GET /v1/instances`.

**Never** without approval:

- `pip install` / `npm install`
- Editing system or user environment variables permanently
- Launching GUI applications
- Modifying MCP host config files

---

## Diagnose why `total == 0`

| Check | Meaning | Next step |
|-------|---------|-----------|
| `healthz` fails | Nothing listening on `DCC_MCP_GATEWAY_URL` | Ask user to start any DCC with gateway election, or run `dcc-mcp-server` with gateway port 9765 |
| `healthz` OK, `readyz` 503 | Gateway booting | Wait; retry `readyz` |
| `healthz` OK, `readyz` 200, `total == 0` | Gateway up, no DCC registered | Start a DCC adapter (sections below) |

Gateway election: the first process binding `DCC_MCP_GATEWAY_PORT` (default **9765**) becomes the gateway; DCC adapters register in `FileRegistry`. See [gateway-election](https://github.com/loonghao/dcc-mcp-core/blob/main/docs/guide/gateway-election.md).

---

## Public catalog (static table)

REST-only agents cannot call MCP `gateway://catalog`. Use this table or, with user permission:

```bash
dcc-mcp-server catalog search --query maya
```

| Package | DCC | URL |
|---------|-----|-----|
| dcc-mcp-core | (library) | https://github.com/loonghao/dcc-mcp-core |
| dcc-mcp-maya-skills | maya | https://github.com/loonghao/dcc-mcp-maya-skills |
| dcc-mcp-blender-skills | blender | https://github.com/loonghao/dcc-mcp-blender-skills |
| dcc-mcp-houdini-skills | houdini | https://github.com/loonghao/dcc-mcp-houdini-skills |
| dcc-mcp-photoshop-skills | photoshop | https://github.com/loonghao/dcc-mcp-photoshop-skills |

**Adapters** (embed HTTP server inside the DCC host):

| DCC | Adapter repo | Typical start |
|-----|--------------|---------------|
| Maya | https://github.com/loonghao/dcc-mcp-maya | `pip install dcc-mcp-maya` → Script Editor: `dcc_mcp_maya.start_server(port=8765)` |
| Blender | https://github.com/loonghao/dcc-mcp-blender | Install addon / enable in Preferences → server auto-starts |
| Houdini | https://github.com/loonghao/dcc-mcp-houdini | Package install + `start_server` per repo README |
| Photoshop | https://github.com/loonghao/dcc-mcp-photoshop | Follow adapter install guide |
| 3ds Max | https://github.com/loonghao/dcc-mcp-3dsmax | Plugin / `start_server` per repo README |

Detailed steps:

- Maya: [getting-started](https://github.com/loonghao/dcc-mcp-maya/blob/main/docs/guide/getting-started.md)
- Blender: [README](https://github.com/loonghao/dcc-mcp-blender/blob/main/README.md)

---

## Per-DCC checklist (after user picks a product)

### Maya

1. Install into Maya's Python: `mayapy -m pip install dcc-mcp-maya`
2. In Maya Script Editor (Python):

   ```python
   import dcc_mcp_maya
   handle = dcc_mcp_maya.start_server(port=8765)
   print(handle.mcp_url())
   ```

3. Ensure `DCC_MCP_GATEWAY_PORT` is not `0` (default gateway election on 9765).
4. User confirms Maya is running → you run `GET /v1/instances` → expect `dcc_type: maya`.

### Blender

1. Install `dcc-mcp-blender` per [README](https://github.com/loonghao/dcc-mcp-blender/blob/main/README.md).
2. Enable the addon in Blender Preferences.
3. Confirm HTTP server is listening (default port 8765 in docs).
4. Poll `/v1/instances` for `dcc_type: blender`.

### Houdini

1. Clone/install [dcc-mcp-houdini](https://github.com/loonghao/dcc-mcp-houdini).
2. Follow repo `README` for package path and `start_server`.
3. Poll instances for `dcc_type: houdini`.

### Photoshop

1. Install [dcc-mcp-photoshop](https://github.com/loonghao/dcc-mcp-photoshop) per repo docs.
2. Start the Photoshop-side service as documented.
3. Poll instances for `dcc_type: photoshop`.

### 3ds Max

1. Install [dcc-mcp-3dsmax](https://github.com/loonghao/dcc-mcp-3dsmax) per repo docs.
2. Load plugin / run `start_server` example.
3. Poll instances for `dcc_type: 3dsmax` (or adapter's canonical `dcc_type`).

---

## Polling loop

After each user-confirmed step:

```bash
GATEWAY="${DCC_MCP_GATEWAY_URL:-http://127.0.0.1:9765}"
curl -s "$GATEWAY/v1/instances" | jq '{total, types: [.instances[].dcc_type]}'
```

When `total >= 1` and target rows have `stale: false`:

1. Report inventory to the user.
2. Resume the main skill flow: `search` → `describe` → `call`.

---

## After a crash

`instance_id` changes when the DCC process restarts. Always re-run inventory and `POST /v1/search` — do not reuse old `tool_slug` values.
