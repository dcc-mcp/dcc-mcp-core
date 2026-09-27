---
name: dcc-mcp-houdini-setup
description: |-
  Set up dcc-mcp-houdini for an agent or operator: install Houdini Python
  dependencies with hython, generate MCP host configuration, guide the user
  through the Houdini package / 123.py autostart load step, and run a first
  live-tool smoke prompt.
license: MIT
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: houdini
    layer: operator
    stage: bootstrap
    version: 1.0.0
    tags:
    - houdini
    - mcp
    - setup
    - hython
    - autostart
---
# dcc-mcp-houdini setup

Use this skill when a user wants an agent to prepare a machine so any MCP
host can use `dcc-mcp-houdini` with SideFX Houdini.

This is an operator skill, not a Houdini runtime skill. Do not load it through
the Houdini MCP server. Run it from the repository checkout or copy its steps
into another agent's instructions.

If the user says "帮我参考 `dcc-mcp/dcc-mcp-houdini/install.md` 去安装", read the
root `install.md` first, then follow this skill.

## Goal

End with:

- `dcc-mcp-houdini` and its pip dependencies installed into the target Houdini
  `hython` environment.
- An MCP host config snippet that points to the Houdini MCP server.
- The user guided to install the Houdini package so `scripts/123.py` autostarts
  the server (or to start it manually from `hython`).
- A live smoke prompt that proves the agent can discover and call Houdini tools.

## Fast Path

From the repository root, run:

```bash
python skills/dcc-mcp-houdini-setup/scripts/setup_dcc_mcp_houdini.py
```

The script:

1. Finds `hython` from `--hython`, `HYTHON`, `DCC_MCP_HOUDINI_HYTHON`, `PATH`,
   or common Side Effects Software install locations.
2. Installs this checkout into Houdini: `hython -m pip install -e .`
   (`dcc-mcp-houdini` defines no runtime extra; `dcc-mcp-core` is pulled in
   automatically).
3. Verifies `import dcc_mcp_houdini`.
4. Writes reusable MCP JSON snippets and a smoke prompt under
   `.dcc-mcp/agent-setup/`.

Use PyPI instead of the local checkout when setting up an end-user machine:

```bash
python skills/dcc-mcp-houdini-setup/scripts/setup_dcc_mcp_houdini.py --source pypi
```

If discovery fails, ask the user for the full `hython` path and re-run:

```bash
python skills/dcc-mcp-houdini-setup/scripts/setup_dcc_mcp_houdini.py --hython "C:\Program Files\Side Effects Software\Houdini 20.5.487\bin\hython.exe"
```

## MCP Configuration

The Houdini `123.py` autostart hook starts the embedded MCP server on the direct
port by default. Configure the MCP host with:

```json
{
  "mcpServers": {
    "houdini": {
      "url": "http://127.0.0.1:9765/mcp"
    }
  }
}
```

Use `http://127.0.0.1:9765/mcp` only when running multiple Houdini sessions
behind the auto-gateway (`DCC_MCP_GATEWAY_PORT=9765`).

When editing an existing MCP config, preserve unrelated servers. Merge only the
`houdini` server entry unless the user asks for a different server name.

## User Hand-Off: Load the Houdini Package / Autostart

After pip setup and MCP JSON generation, tell the user to make the server start
with Houdini. The recommended path is the bundled Houdini package:

1. Install the Houdini package (quickinstall ZIP `install.ps1` / `install.sh`,
   or a `dcc_mcp_houdini.json` package file pointing at the package root).
2. The package adds `scripts/` to `HOUDINI_PATH`; on startup `123.py` extracts
   bundled wheels into `vendor/` and starts the MCP server.
3. Start Houdini and watch the console / shell for the exact OS-assigned
   instance URL. Agents normally connect through the stable gateway on `9765`.

To run from a manual `hython` session instead of the package autostart:

```python
import dcc_mcp_houdini
server = dcc_mcp_houdini.start_server()
print(server.mcp_url)  # Exact direct endpoint selected by the OS
```

Set `DCC_MCP_HOUDINI_AUTOSTART=0` to disable package autostart.

## First Live Smoke Prompt

Ask the MCP host to run this prompt after Houdini is open and the server is
running:

```text
Use the Houdini MCP server. First call dcc_capability_manifest with loaded_only=false.
Then load the houdini-nodes skill, create a geo node named mcp_setup_smoke_geo
under /obj, list the obj-level nodes, and tell me the MCP URL and created node path.
Use typed tools where available and avoid execute_python unless no typed tool fits.
```

Expected behavior:

- The agent discovers capabilities without dumping every schema.
- The agent loads `houdini-nodes`.
- The agent calls `houdini_nodes__create_node` with `parent_path=/obj`,
  `node_type=geo`, `node_name=mcp_setup_smoke_geo`.
- The new node appears in the Houdini scene.
- `houdini_scene__list_obj_nodes` confirms it exists.

## Troubleshooting

- `hython` not found: ask for the exact Houdini version and the
  `bin/hython` path, then pass `--hython`.
- Pip bootstrap fails: run `hython -m ensurepip --upgrade`, then repeat install.
- MCP connection refused: Houdini is not running, autostart is disabled
  (`DCC_MCP_HOUDINI_AUTOSTART=0`), or the stable gateway on `9765` is not
  running. Use `dcc-mcp-cli list` to inspect direct URLs.
- Tool missing: call `dcc_capability_manifest` or `search_skills`, then
  `load_skill("<skill-name>")`.
- Autostart silent: check the Houdini console for a
  `dcc-mcp-houdini autostart failed` line and confirm the package JSON points at
  the correct package root.
