# DCC Integration Architectures

How to connect different DCC applications to dcc-mcp-core's MCP ecosystem.

dcc-mcp-core supports **three integration architectures** depending on whether
your DCC has embedded Python, uses a WebSocket bridge, or runs inside a WebView.

---

## Architecture Decision Tree

```
Does the DCC embed Python?
├─ YES → Architecture A: Embedded Python (DccServerBase)
│   Examples: Maya, Blender, Houdini, 3ds Max, Nuke, FreeCAD
│
└─ NO
    ├─ Does it expose a JS/WebSocket/HTTP API?
    │   └─ YES → Architecture B: WebSocket Bridge (DccBridge)
    │       Examples: Photoshop (UXP/CEP), ZBrush (GoZ HTTP), After Effects
    │
    └─ Is it a WebView/browser panel inside another DCC?
        └─ YES → Architecture C: WebView Host (WebViewAdapter)
            Examples: AuroraView, Electron tools, ImGui panels
```

---

## Architecture A: Embedded Python (`DccServerBase`)

**For:** DCCs that have built-in Python interpreters (Maya, Blender, Houdini, Nuke, 3ds Max).

**How it works:** The MCP server runs *inside* the DCC's Python process. Skills
execute their scripts via `subprocess` but communicate with the DCC through its
native Python API (e.g. `maya.cmds`, `bpy`, `hou`).

### Minimal adapter (~30 lines)

```python
# my_dcc_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class MyDccMcpServer(DccServerBase):
    def __init__(self, port: int = 8765, **kwargs):
        super().__init__(
            dcc_name="mydcc",                           # short identifier
            builtin_skills_dir=Path(__file__).parent / "skills",  # adapter-bundled skills
            port=port,
            **kwargs,
        )

    def _version_string(self) -> str:
        """Return the DCC application version."""
        import mydcc
        return mydcc.version()

# Zero-boilerplate start/stop pair (singleton + thread-safe)
start_server, stop_server = make_start_stop(
    MyDccMcpServer,
    hot_reload_env_var="DCC_MCP_MYDCC_HOT_RELOAD",
)
```

### What `DccServerBase` provides for free

- Skill search path collection (per-app env var + global env var + bundled)
- `McpHttpServer` + `SkillCatalog` wiring via `create_skill_server`
- All 7 skill query/management methods (find, list, load, unload, ...)
- Hot-reload integration (`DccSkillHotReloader`)
- Gateway election and failover (`DccGatewayElection`)
- Instance-bound diagnostics (screenshot, audit log — resolves DCC window by PID)
- Server lifecycle (start/stop/is_running/mcp_url)

### Real-world examples

| DCC | Adapter Repo | Key Pattern |
|-----|-------------|-------------|
| Maya | `dcc-mcp-maya` | `maya.cmds` + `cmds.evalDeferred` for main thread safety |
| Blender | `dcc-mcp-blender` | `bpy.app.timers` for main thread dispatch |
| Houdini | `dcc-mcp-houdini` | `hou.ui.addEventLoopCallback` for async operations |

### Maya example (complete)

```python
# maya_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class MayaMcpServer(DccServerBase):
    def __init__(self, port=8765, **kwargs):
        super().__init__(
            dcc_name="maya",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Autodesk Maya",    # for diagnostic screenshots
            **kwargs,
        )

    def _version_string(self):
        import maya.cmds as cmds
        return cmds.about(version=True)

start_server, stop_server = make_start_stop(
    MayaMcpServer,
    hot_reload_env_var="DCC_MCP_MAYA_HOT_RELOAD",
)

# --- In Maya's Script Editor or userSetup.py: ---
# from maya_adapter.server import start_server
# start_server(port=8765)
```

### Blender example

```python
# blender_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class BlenderMcpServer(DccServerBase):
    def __init__(self, port=8765, **kwargs):
        super().__init__(
            dcc_name="blender",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Blender",
            **kwargs,
        )

    def _version_string(self):
        import bpy
        return bpy.app.version_string

start_server, stop_server = make_start_stop(
    BlenderMcpServer,
    hot_reload_env_var="DCC_MCP_BLENDER_HOT_RELOAD",
)
```

### Houdini example

```python
# houdini_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class HoudiniMcpServer(DccServerBase):
    def __init__(self, port=8765, **kwargs):
        super().__init__(
            dcc_name="houdini",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Houdini",
            **kwargs,
        )

    def _version_string(self):
        import hou
        return hou.applicationVersionString()

start_server, stop_server = make_start_stop(
    HoudiniMcpServer,
    hot_reload_env_var="DCC_MCP_HOUDINI_HOT_RELOAD",
)
```

---

## Architecture B: WebSocket Bridge (`DccBridge`)

**For:** DCCs that do NOT embed Python but expose a WebSocket, HTTP, or IPC API
(Photoshop via UXP/CEP, ZBrush via GoZ, After Effects via ExtendScript).

**How it works:** A standalone Python process runs the MCP server and the
`DccBridge` WebSocket server. The DCC connects to the bridge via a plugin
(UXP panel, CEP extension, GoZ script). Communication uses JSON-RPC 2.0 over
WebSocket.

```
┌─────────────┐   MCP/HTTP   ┌──────────────────┐  WebSocket   ┌──────────────┐
│  AI Agent   │ ──────────── │  Python Process   │ ──────────── │  Photoshop   │
│  (Claude)   │              │  DccServerBase +  │  JSON-RPC    │  UXP Plugin  │
│             │              │  DccBridge(:9001) │              │              │
└─────────────┘              └──────────────────┘              └──────────────┘
```

### Bridge protocol

The bridge uses a typed JSON-RPC 2.0 protocol:

```
DCC Plugin → Bridge:  {"type": "hello", "client": "photoshop", "version": "25.0"}
Bridge → DCC Plugin:  {"type": "hello_ack", "server": "dcc-mcp-server", "session_id": "..."}

Bridge → DCC Plugin:  {"type": "request", "jsonrpc": "2.0", "id": 1, "method": "ps.getDocumentInfo"}
DCC Plugin → Bridge:  {"type": "response", "jsonrpc": "2.0", "id": 1, "result": {...}}

DCC Plugin → Bridge:  {"type": "event", "event": "document.changed", "data": {...}}
DCC Plugin → Bridge:  {"type": "disconnect", "reason": "shutdown"}
```

### Photoshop example (complete)

```python
# photoshop_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.bridge import DccBridge
from dcc_mcp_core.factory import make_start_stop

class PhotoshopMcpServer(DccServerBase):
    def __init__(self, port=8765, bridge_port=9001, **kwargs):
        super().__init__(
            dcc_name="photoshop",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            # IMPORTANT: set dcc_pid to Photoshop's PID, not this process
            dcc_pid=kwargs.pop("dcc_pid", None),
            dcc_window_title="Adobe Photoshop",
            **kwargs,
        )
        self._bridge = DccBridge(port=bridge_port, server_name="photoshop-mcp")

    def start(self):
        # Start bridge first, then MCP server
        self._bridge.connect(wait_for_dcc=False)
        handle = super().start()

        # Register bridge-powered handlers
        self._server.register_handler("get_document_info", self._get_document_info)
        self._server.register_handler("list_layers", self._list_layers)
        self._server.register_handler("run_action", self._run_action)
        return handle

    def stop(self):
        self._bridge.disconnect()
        super().stop()

    # --- Bridge-powered tool handlers ---

    def _get_document_info(self, params):
        return self._bridge.call("ps.getDocumentInfo")

    def _list_layers(self, params):
        return self._bridge.call("ps.listLayers",
                                 include_hidden=params.get("include_hidden", False))

    def _run_action(self, params):
        return self._bridge.call("ps.runAction",
                                 action_name=params["action_name"],
                                 action_set=params.get("action_set", "Default Actions"))

start_server, stop_server = make_start_stop(PhotoshopMcpServer)
```

### DCC-side UXP plugin (Photoshop)

```javascript
// photoshop-uxp-plugin/main.js
const WebSocket = require("ws");
const photoshop = require("photoshop");
const app = photoshop.app;

const ws = new WebSocket("ws://localhost:9001");

ws.on("open", () => {
  // Hello handshake
  ws.send(JSON.stringify({
    type: "hello",
    client: "photoshop",
    version: app.version,
  }));
});

ws.on("message", async (data) => {
  const msg = JSON.parse(data);
  if (msg.type !== "request") return;

  try {
    let result;
    switch (msg.method) {
      case "ps.getDocumentInfo":
        const doc = app.activeDocument;
        result = { name: doc.name, width: doc.width, height: doc.height };
        break;
      case "ps.listLayers":
        result = app.activeDocument.layers.map(l => ({
          name: l.name, kind: l.kind, visible: l.visible,
        }));
        break;
      case "ps.runAction":
        await photoshop.action.batchPlay([{
          _obj: "play",
          _target: [{ _ref: "action", _name: msg.params.action_name }],
        }]);
        result = { success: true };
        break;
      default:
        ws.send(JSON.stringify({
          type: "response", jsonrpc: "2.0", id: msg.id,
          error: { code: -32601, message: `Unknown method: ${msg.method}` },
        }));
        return;
    }
    ws.send(JSON.stringify({
      type: "response", jsonrpc: "2.0", id: msg.id, result,
    }));
  } catch (err) {
    ws.send(JSON.stringify({
      type: "response", jsonrpc: "2.0", id: msg.id,
      error: { code: -32000, message: err.message },
    }));
  }
});
```

### ZBrush example (HTTP bridge)

ZBrush exposes GoZ which is file-based, but newer versions support HTTP.
The pattern is similar — a Python process bridges between MCP and ZBrush's API:

```python
# zbrush_adapter/server.py
import requests
from dcc_mcp_core.server_base import DccServerBase

class ZBrushMcpServer(DccServerBase):
    def __init__(self, port=8765, zbrush_url="http://localhost:6789", **kwargs):
        super().__init__(
            dcc_name="zbrush",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            **kwargs,
        )
        self._zbrush_url = zbrush_url

    def start(self):
        handle = super().start()
        self._server.register_handler("get_tool_info", self._get_tool_info)
        return handle

    def _get_tool_info(self, params):
        resp = requests.get(f"{self._zbrush_url}/api/tool/info")
        return resp.json()
```

### After Effects example (ExtendScript bridge)

```python
# ae_adapter/server.py
from dcc_mcp_core.bridge import DccBridge
from dcc_mcp_core.server_base import DccServerBase

class AfterEffectsMcpServer(DccServerBase):
    def __init__(self, port=8765, bridge_port=9002, **kwargs):
        super().__init__(
            dcc_name="aftereffects",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Adobe After Effects",
            **kwargs,
        )
        self._bridge = DccBridge(port=bridge_port, server_name="ae-mcp")

    def start(self):
        self._bridge.connect(wait_for_dcc=False)
        handle = super().start()
        self._server.register_handler("get_composition",
            lambda p: self._bridge.call("ae.getActiveComposition"))
        self._server.register_handler("add_layer",
            lambda p: self._bridge.call("ae.addLayer", **p))
        return handle
```

---

## Architecture C: WebView Host (`WebViewAdapter`)

**For:** Browser-based tool panels embedded inside a DCC or running standalone
(AuroraView, Electron apps, ImGui web panels, CEF tools).

**How it works:** The WebView is a thin host — it doesn't own a scene graph or
timeline. It advertises a **narrower capability surface** so the Gateway hides
tools that require capabilities the WebView doesn't support (e.g. `scene`,
`timeline`).

```
┌──────────────┐   MCP/HTTP   ┌──────────────────┐   CDP/JS    ┌──────────────┐
│  AI Agent    │ ──────────── │  Python Process   │ ──────────  │  WebView     │
│  (Claude)    │              │  DccServerBase +  │  Bridge     │  (AuroraView │
│              │              │  WebViewAdapter   │             │   / Electron) │
└──────────────┘              └──────────────────┘             └──────────────┘
```

### Capabilities model

WebView hosts declare which of the 5 core capabilities they support:

| Capability | Full DCC (Maya) | WebView (default) | Custom WebView |
|-----------|----------------|-------------------|----------------|
| `scene` | Yes | **No** | Override if app has scene graph |
| `timeline` | Yes | **No** | Override if app has timeline |
| `selection` | Yes | **No** | Override if app has selection model |
| `undo` | Yes | **No** | Override if app has undo stack |
| `render` | Yes | **No** | Override if app has render engine |

Tools registered with `required_capabilities=["scene"]` are hidden from
WebView sessions that don't support `scene`.

### AuroraView example

```python
# auroraview_adapter/adapter.py
from typing import ClassVar
from dcc_mcp_core.adapters import WebViewAdapter, WEBVIEW_DEFAULT_CAPABILITIES, WebViewContext

class AuroraViewAdapter(WebViewAdapter):
    dcc_name = "auroraview"

    # AuroraView supports undo (in-browser undo stack) but not scene/timeline
    capabilities: ClassVar[dict[str, bool]] = {
        **WEBVIEW_DEFAULT_CAPABILITIES,
        "undo": True,
    }

    def __init__(self, cdp_port: int = 9222, host_dcc: str | None = None):
        self._cdp_port = cdp_port
        self._host_dcc = host_dcc

    def get_context(self) -> WebViewContext:
        return WebViewContext(
            window_title="AuroraView",
            url=f"http://localhost:{self._cdp_port}",
            cdp_port=self._cdp_port,
            host_dcc=self._host_dcc,
        )

    def list_tools(self) -> list[dict]:
        return [
            {"name": "navigate_url", "description": "Navigate the WebView to a URL"},
            {"name": "take_screenshot", "description": "Capture the WebView content"},
            {"name": "execute_js", "description": "Execute JavaScript in the WebView"},
        ]

    def execute(self, tool: str, params=None) -> dict:
        params = params or {}
        # Dispatch to CDP or JS bridge
        if tool == "navigate_url":
            # ... CDP Page.navigate
            return {"success": True, "url": params.get("url")}
        elif tool == "take_screenshot":
            # ... CDP Page.captureScreenshot
            return {"success": True, "format": "png"}
        elif tool == "execute_js":
            # ... CDP Runtime.evaluate
            return {"success": True, "result": "..."}
        raise ValueError(f"Unknown tool: {tool}")
```

### Registering with the Gateway (ServiceEntry.extras)

WebView hosts register with extra metadata so the Gateway can identify them:

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager(registry_dir="/tmp/dcc-mcp")
instance_id = mgr.register_service(
    "auroraview",
    "127.0.0.1",
    8765,
    extras={
        "cdp_port": 9222,
        "url": "http://localhost:3000",
        "window_title": "AuroraView Panel",
        "host_dcc": "maya",           # embedded inside Maya
    },
)

# AI agents discover it via the Gateway:
# tools/call list_dcc_instances → shows auroraview with all extras
```

---

## Unreal Engine Integration

Unreal Engine requires a **hybrid approach**: it embeds Python (via the
`PythonScriptPlugin`) but many operations must run on the game thread.

### Recommended architecture

```
┌──────────────┐   MCP/HTTP   ┌───────────────────┐  Python   ┌──────────────┐
│  AI Agent    │ ──────────── │  Unreal Python    │ ────────  │  Unreal      │
│  (Claude)    │              │  DccServerBase    │  unreal   │  Editor      │
│              │              │  (in PythonPlugin)│  module   │  (C++ core)  │
└──────────────┘              └───────────────────┘           └──────────────┘
```

### Example adapter

```python
# unreal_adapter/server.py
from pathlib import Path
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class UnrealMcpServer(DccServerBase):
    def __init__(self, port=8765, **kwargs):
        super().__init__(
            dcc_name="unreal",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Unreal Editor",
            **kwargs,
        )

    def _version_string(self):
        import unreal
        return unreal.SystemLibrary.get_engine_version()

start_server, stop_server = make_start_stop(
    UnrealMcpServer,
    hot_reload_env_var="DCC_MCP_UNREAL_HOT_RELOAD",
)

# --- In Unreal Editor Python console: ---
# import unreal_adapter.server
# unreal_adapter.server.start_server(port=8765)
```

### Unreal-specific considerations

- **Game thread safety**: Use `unreal.EditorAssetSubsystem` and tick-based
  deferred execution; never call `unreal.*` from skill subprocesses directly.
- **Blueprint integration**: Register skill handlers that call Blueprint
  functions via `unreal.call_function()`.
- **Remote Execution plugin**: Enable `Edit > Editor Preferences > Python >
  Enable Remote Execution` for external Python access.

---

## Unity Integration

Unity uses **C#** (no embedded Python). Integration requires a WebSocket bridge
similar to the Photoshop pattern.

### Recommended architecture

```
┌──────────────┐   MCP/HTTP   ┌──────────────────┐  WebSocket  ┌──────────────┐
│  AI Agent    │ ──────────── │  Python Process   │ ──────────  │  Unity       │
│  (Claude)    │              │  DccServerBase +  │  JSON-RPC   │  C# Plugin   │
│              │              │  DccBridge(:9003) │             │  (Editor)    │
└──────────────┘              └──────────────────┘             └──────────────┘
```

### Python side

```python
# unity_adapter/server.py
from pathlib import Path
from dcc_mcp_core.bridge import DccBridge
from dcc_mcp_core.server_base import DccServerBase
from dcc_mcp_core.factory import make_start_stop

class UnityMcpServer(DccServerBase):
    def __init__(self, port=8765, bridge_port=9003, **kwargs):
        super().__init__(
            dcc_name="unity",
            builtin_skills_dir=Path(__file__).parent / "skills",
            port=port,
            dcc_window_title="Unity",
            **kwargs,
        )
        self._bridge = DccBridge(port=bridge_port, server_name="unity-mcp")

    def start(self):
        self._bridge.connect(wait_for_dcc=False)
        handle = super().start()
        self._server.register_handler("get_scene_info",
            lambda p: self._bridge.call("unity.getSceneInfo"))
        self._server.register_handler("create_gameobject",
            lambda p: self._bridge.call("unity.createGameObject", **p))
        return handle

    def stop(self):
        self._bridge.disconnect()
        super().stop()

start_server, stop_server = make_start_stop(UnityMcpServer)
```

### Unity C# side (Editor script)

```csharp
// Assets/Editor/DccMcpBridge.cs
using UnityEngine;
using UnityEditor;
using WebSocketSharp;
using Newtonsoft.Json.Linq;

[InitializeOnLoad]
public class DccMcpBridge
{
    static WebSocket ws;

    static DccMcpBridge()
    {
        ws = new WebSocket("ws://localhost:9003");
        ws.OnOpen += (s, e) => {
            ws.Send(JsonUtility.ToJson(new {
                type = "hello", client = "unity", version = Application.unityVersion
            }));
        };
        ws.OnMessage += (s, e) => HandleMessage(e.Data);
        ws.Connect();
    }

    static void HandleMessage(string data)
    {
        var msg = JObject.Parse(data);
        if (msg["type"]?.ToString() != "request") return;

        var id = msg["id"];
        var method = msg["method"]?.ToString();
        JToken result = null;
        string error = null;

        switch (method)
        {
            case "unity.getSceneInfo":
                var scene = UnityEngine.SceneManagement.SceneManager.GetActiveScene();
                result = JObject.FromObject(new { name = scene.name, path = scene.path });
                break;
            case "unity.createGameObject":
                var name = msg["params"]?["name"]?.ToString() ?? "New Object";
                var go = new GameObject(name);
                result = JObject.FromObject(new { name = go.name, id = go.GetInstanceID() });
                break;
            default:
                error = $"Unknown method: {method}";
                break;
        }

        // Send response back
        ws.Send(new JObject {
            ["type"] = "response", ["jsonrpc"] = "2.0", ["id"] = id,
            [error != null ? "error" : "result"] = error != null
                ? new JObject { ["code"] = -32601, ["message"] = error }
                : result
        }.ToString());
    }
}
```

---

## Architecture Comparison

| | Embedded Python (A) | WebSocket Bridge (B) | WebView Host (C) |
|---|---|---|---|
| **Adapter LOC** | ~30 lines | ~80 lines | ~50 lines |
| **DCC plugin needed?** | No (Python built-in) | Yes (JS/C# plugin) | Yes (JS bridge) |
| **Latency** | Lowest (in-process) | Medium (WS round-trip) | Medium (CDP) |
| **DCC examples** | Maya, Blender, Houdini | Photoshop, ZBrush, Unity | AuroraView, Electron |
| **Capabilities** | Full (scene, timeline, ...) | Full (via bridge calls) | Narrow (configurable) |
| **Base class** | `DccServerBase` | `DccServerBase` + `DccBridge` | `WebViewAdapter` |
| **Main thread safety** | DCC-specific (deferred exec) | Always safe (separate process) | N/A |
| **Gateway registration** | Automatic (via `McpHttpConfig`) | Automatic | Manual (`TransportManager`) |

---

## Common Patterns

### Registering custom tool handlers

Regardless of architecture, register handlers **before** calling `server.start()`:

```python
# DCC-specific tools that go beyond skill scripts
server._server.register_handler("get_viewport_camera",
    lambda p: {"position": [0, 5, 10], "rotation": [0, 0, 0]})

# Then start
handle = server.start()
```

### Environment variables

| Variable | Purpose |
|----------|---------|
| `DCC_MCP_SKILL_PATHS` | Global skill search paths |
| `DCC_MCP_{DCC}_SKILL_PATHS` | Per-DCC skill search paths (e.g. `DCC_MCP_MAYA_SKILL_PATHS`) |
| `DCC_MCP_GATEWAY_PORT` | Gateway election port (default 9765) |
| `DCC_MCP_REGISTRY_DIR` | Shared FileRegistry directory |
| `DCC_MCP_{DCC}_HOT_RELOAD=1` | Enable skill hot-reload |

### Multiple DCC instances

The Gateway automatically discovers and aggregates tools from all running
DCC instances. Each instance registers with a unique `instance_id` and its
tools are namespaced as `<8char_prefix>__<tool_name>` in the Gateway's
`tools/list` response.

```python
# AI agent sees:
# a1b2c3d4__create_sphere  (Maya instance 1)
# e5f6g7h8__create_sphere  (Maya instance 2)
# i9j0k1l2__add_material   (Blender instance 1)
```
