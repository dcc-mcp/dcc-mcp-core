# MCP + Skills Integration

> **[中文版](../zh/guide/mcp-skills-integration)**

## Why We Combine MCP and Skills

### The Problem with Stock MCP

Standard MCP tools/list returns every tool the server knows about — all at once.

With 3 DCC instances × 50 skills × 5 scripts = **750 tools in context**. The AI's context window fills instantly, costs skyrocket, and reasoning quality degrades.

```
Stock MCP tools/list:
┌──────────────────────────────────────────────┐
│  maya_geo__create_sphere                      │
│  maya_geo__bevel                              │
│  maya_anim__set_keyframe  ... (x 250)         │
│  blender_sculpt__smooth                       │
│  blender_sculpt__grab     ... (x 250)         │
│  houdini_vex__compile     ... (x 250)         │
└──────────────────────────────────────────────┘
750 tools in context every time → expensive, slow
```

### Our Solution: Session-Scoped Progressive Discovery

Each AI session is **pinned to one DCC instance**. tools/list is scoped to that instance.

```
Session A (Maya instance #1):
  tools/list → 100 Maya tools + 50 shared tools = 150 tools

Session B (Houdini instance #1):
  tools/list → 100 Houdini tools + 50 shared tools = 150 tools

71% reduction in context size, zero information loss
```

### The Problem with CLI Tools

CLI tools are **blind to DCC state**:
- Can't see the active scene, selected objects, or viewport
- Require multiple roundtrips to gather context
- Return raw text requiring fragile parsing
- Have no visual feedback during execution

dcc-mcp-core **lives inside the DCC** and can access its full state directly.

## Architecture

```
AI Agent (Claude, GPT, etc.)
    │
    │ tools/call {"name": "maya_geo__create_sphere", "radius": 2.0}
    │
    ▼
Gateway Server (dcc-mcp-server)          ← one per machine
    │
    │ Session A → Maya instance #1 (IPC)
    │ Session B → Houdini instance #1 (IPC)
    │
    ▼
DCC Bridge Plugin
    │
    │ Executes script, captures result
    │
    ▼
{success: true, message: "Sphere created", context: {name: "pSphere1"}}
```

## Key Concepts

### ServiceEntry — What Each DCC Registers

```python
from dcc_mcp_core import create_skill_server, McpHttpConfig

# Gateway auto-registers the DCC instance
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
```

The gateway reads this to know:
- Which instance is available
- What files are open
- Which instance to route the AI's request to

### Session Isolation

```python
# Session pinning is handled automatically by the gateway
# based on dcc_type, dcc_version, and scene metadata
# set on McpHttpConfig
```

### Skills System

```
SKILL.md  (metadata)          scripts/
──────────────────────────────────────────────
name: maya-geometry           create_sphere.py
dcc: maya                     bevel.py
scope: repo                   export_fbx.bat
policy:                       ──────────────
  products: ["maya"]          ↓ registered as
  allow_implicit_invocation:  maya_geometry__create_sphere
    false                     maya_geometry__bevel
                              maya_geometry__export_fbx
```

Zero Python glue code. metadata + scripts = MCP tools.

### Progressive Discovery

Tools are revealed gradually based on context:

| Filter | Criteria | Result |
|--------|----------|--------|
| **DCC type** | Session is pinned to Maya | Only Maya tools visible |
| **Product** | `policy.products: ["maya"]` | Houdini tools hidden |
| **Scope** | `scope: system` | Can't be overridden by repo skills |
| **Implicit** | `allow_implicit_invocation: false` | Requires explicit `load_skill` first |

## Quick Start

### 1. Install

```bash
pip install dcc-mcp-core
```

### 2. Create a skill

```
my-tools/
├── SKILL.md
└── scripts/
    └── create_sphere.py
```

**SKILL.md:**
```yaml
---
name: my-tools
dcc: maya
scope: repo
policy:
  allow_implicit_invocation: true
  products: ["maya"]
---
# My Maya Tools
Custom geometry tools.
```

### 3. Start the server

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my-tools"

server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"MCP server: {handle.mcp_url()}")
# AI clients connect to http://127.0.0.1:8765/mcp
```

### 4. Connect Claude Desktop

```json
{
  "mcpServers": {
    "maya": {
      "url": "http://127.0.0.1:8765/mcp"
    }
  }
}
```

## What Makes This Different

| Feature | dcc-mcp-core | Generic MCP | CLI tools |
|---------|-------------|------------|-----------|
| DCC state awareness | ✅ Scene, docs, objects | ❌ None | ❌ None |
| Context scoping | ✅ Session-isolated | ❌ Global | ❌ N/A |
| Zero-code tools | ✅ SKILL.md | ❌ Full Python | ✅ Scripts only |
| Multi-instance | ✅ Gateway election | ❌ Single endpoint | ❌ No |
| Structured results | ✅ Always | ⚠️ Manual | ❌ Text parsing |
