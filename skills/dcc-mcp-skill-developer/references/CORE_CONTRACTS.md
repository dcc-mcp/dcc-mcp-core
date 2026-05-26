# Core Contracts for New DCC Adapters

Use this reference before creating or modernizing a first-party adapter such as
`dcc-mcp-nuke`, `dcc-mcp-zbrush`, or `dcc-mcp-openusd`.

## Source of truth

Start from the current `main` branch of `dcc-mcp-core`:

- Compact LLM index: <https://github.com/loonghao/dcc-mcp-core/blob/main/llms.txt>
- Full LLM index: <https://github.com/loonghao/dcc-mcp-core/blob/main/llms-full.txt>
- Agent rules: <https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md>
- Dispatcher API: <https://github.com/loonghao/dcc-mcp-core/blob/main/docs/api/dispatcher.md>
- HTTP/Gateway API: <https://github.com/loonghao/dcc-mcp-core/blob/main/docs/api/http.md>
- Adapter context resources: <https://github.com/loonghao/dcc-mcp-core/blob/main/docs/api/adapter-context.md>
- Runtime contracts: <https://github.com/loonghao/dcc-mcp-core/blob/main/docs/guide/adapter-runtime-contracts.md>
- Skill maintenance: <https://github.com/loonghao/dcc-mcp-core/blob/main/docs/guide/skill-maintenance.md>

Sibling adapters are useful examples, but they are not the contract. If sibling
code disagrees with the current core docs, prefer the core docs and open a
migration issue for the adapter.

## New adapter checklist

1. Read `llms.txt` first, then open the focused API page for the part you are
   about to wire.
2. Build the server around `DccServerOptions.from_env(...)` and
   `DccServerBase`.
3. Keep `dcc_name`, `dcc_type`, environment variable prefixes, skill names, and
   docs examples parameterized. Do not bake in Maya defaults.
4. Register tools, resources, prompts, execution bridges, and diagnostics before
   `server.start()`.
5. Prefer `HostExecutionBridge` for in-process skill execution and dynamic host
   callables.
6. Use `HostUiDispatcherBase` for embedded interactive hosts that need a
   main-thread queue. The adapter should only supply host-specific pump glue.
7. Use `check_dcc_cancelled()` in long-running skill scripts that may run under
   a host dispatcher.
8. Use progressive loading: `search_skills` -> `load_skill` -> group activation.
   Do not expose every host command as a flat `tools/list`.
9. Add one low-level executable test plus one discovery/load/call or REST path
   when behavior crosses the MCP/gateway boundary.

## Host-family defaults

| Host family | Default design |
|-------------|----------------|
| Qt desktop hosts: Nuke, Houdini desktop, Maya, 3ds Max, Cinema 4D, Substance Painter, Mari | Use the shared core Qt dispatcher and `HostUiDispatcherBase` seams once available. Do not create a new adapter-local JSON-line TCP server. |
| Blender-style Python hosts | Use a lean `DccServerBase` scaffold plus a host timer pump around `bpy.app.timers` or equivalent. |
| Bridge/API hosts: ZBrush, Photoshop, Unity, Figma | Keep the native transport behind a typed bridge/client. Skills above the bridge should still be normal DCC-MCP tools with schemas, safety annotations, resources, and diagnostics. |
| OpenUSD / file-scene libraries | Treat as file/scene-library-first. Use `affinity: any` for pure USD file inspection, validation, conversion, and composition tools. Add UI dispatch only when driving an interactive viewer such as `usdview`. |
| Custom studio hosts | Preserve unknown host names and model capabilities explicitly. Add core abstractions only after at least two hosts need the same seam. |

## Do not copy these seams into new adapters

- Qt bridge servers.
- Sidecar action payload parsing and result envelopes.
- UI-thread queue, cancellation, timeout, and shutdown protocols.
- Gateway discovery, health, audit, or trace logic.
- Skill discovery/loading internals.
- JSON/YAML helpers already provided by core.

If the public core API is missing for one of these seams, create or update a
core issue first, then keep the adapter issue as a migration task.
