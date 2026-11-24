# Roblox Studio DCC-MCP bridge

This example is the first host-plugin implementation for the shared DCC
bridge protocol. It keeps MCP transport, request routing, and lifecycle state
in a local sidecar while the Roblox Studio plugin owns only typed Studio API
operations.

## Components

- `plugin/RobloxStudioDccMcpPlugin.luau` connects to the sidecar with the
  Studio-only `HttpService:CreateWebStreamClient` WebSocket API.
- `server.py` exposes a small stdio MCP server and reuses the core `DccBridge`
  for typed calls, lifecycle state, events, and instance routing.
- The Rust protocol tests cover the same hello, ready, heartbeat, event, and
  batch envelopes used by this example.

The initial tool surface is deliberately bounded to Studio state, hierarchy
inspection, script reads, and a change-history-backed multi-edit operation.
Arbitrary Luau execution and native UI input are not part of this example.

## Run the example

Install the optional example dependency:

```powershell
python -m pip install dcc-mcp-core websockets
```

Start the bridge:

```powershell
python examples/roblox-studio/server.py --ws-port 9001
```

Build the local plugin model from the canonical Luau source:

```powershell
python examples/roblox-studio/plugin/build_plugin.py
```

Copy `plugin/RobloxStudioDccMcpPlugin.rbxmx` to the Roblox Studio local
Plugins folder, then restart Studio. Roblox's local-plugin format preserves the
plugin script's legacy `RunContext` in the model file. The plugin uses
`HttpService:CreateWebStreamClient`, so enable HTTP requests in Studio before
running it. Then
open an MCP client against the stdio command:

```json
{
  "mcpServers": {
    "dcc-mcp-roblox-studio": {
      "command": "python",
      "args": ["examples/roblox-studio/server.py", "--ws-port", "9001"]
    }
  }
}
```

The plugin connects to `ws://127.0.0.1:9001`. Use a different local port when
multiple Studio sessions are active.

Call `roblox_list_studios` first when more than one Studio session is open. Pass
the returned `studio_id` to the other tools to target a specific session. The
tools also accept the core-wide `instance_id` spelling.

`roblox_multi_edit` validates every script path before changing the data model,
then wraps the edits in a `ChangeHistoryService` recording so Studio can commit
or cancel the complete operation as one undo unit.

## Official Roblox MCP compatibility

Current Roblox Studio also ships an official stdio MCP server. The generic
core translator can expose it through the DCC-MCP gateway without installing
this example:

```powershell
dcc-mcp-server translate `
  --stdio "`$env:LOCALAPPDATA\Roblox\mcp.bat" `
  --app-type roblox-studio `
  --expose-streamable-http true
```

The official route is useful for immediate adoption. This example is the
extension route when the studio plugin lifecycle, typed tool policy, event
stream, or multi-instance metadata needs to be owned by DCC-MCP.
