# MCP + Skills System Integration Guide

## Overview

dcc-mcp-core combines two powerful concepts:

1. **Model Context Protocol (MCP)** — AI-native protocol for tool discovery and execution
2. **Skills System** — Zero-code tool registration via SKILL.md + scripts/

Together, they solve MCP's context explosion and enable **progressive discovery** — tools are revealed gradually based on session, DCC instance, scope, and product filter.

## Why MCP + Skills?

### The Problem with Generic MCP
- All tools returned in `tools/list` (context bloat with multiple DCC instances)
- No awareness of DCC state (active scene, document list, process health)
- No way to scope tools per instance or session
- Requires manual Python code to register each tool

### The Problem with CLI Tools
- Blind to DCC state (no viewport, no active objects)
- Multiple roundtrips for context gathering
- Fragile parsing of text outputs
- No visual feedback during execution
- Scales poorly with automation complexity

### Our Solution
**MCP + Skills = "Smart MCP"**

- ✅ Scope tools by DCC, instance, scene, scope level
- ✅ Session isolation (each AI talks to one DCC instance)
- ✅ Version-aware gateway (automatic handoff to newer DCC)
- ✅ Progressive discovery (reveal tools as needed)
- ✅ Zero-code tool registration (SKILL.md + scripts/)
- ✅ Structured results (success, message, context, next_steps)

## Key Concepts

### 1. Service Entry (Instance Metadata)

Each running DCC registers a `ServiceEntry`:

```python
{
    "dcc_type": "maya",
    "instance_id": "550e8400-e29b-41d4-a716-446655440000",
    "version": "2025",
    "scene": "project.ma",                    # Active document
    "documents": ["project.ma", "rig.ma"],    # All open files
    "display_name": "Maya-Production",        # User-friendly label
    "pid": 12345,                             # Process ID
    "status": "AVAILABLE",                    # AVAILABLE | BUSY | UNREACHABLE
}
```

The gateway uses this to:
- Route requests to correct instance
- Show available scenes/documents
- Decide which instance receives a new task

### 2. SkillScope (Trust Levels)

Skills are classified by trust level:

```
Repo (lowest)   < User < System < Admin (highest)
```

- **Repo**: `.codex/skills/` (project-local, lowest trust)
- **User**: `~/.skills/` (user-level)
- **System**: Bundled with dcc-mcp-core (shipped, verified)
- **Admin**: Enterprise-controlled (highest trust)

A Repo skill cannot override a System skill (by default).

### 3. SkillPolicy (Invocation Control)

Define how a skill can be called:

```yaml
policy:
  allow_implicit_invocation: false   # Require explicit load_skill
  products: ["maya", "houdini"]      # Only visible for these DCCs
```

- `allow_implicit_invocation: true` — Model can discover & call in same turn
- `allow_implicit_invocation: false` — Model must first call `load_skill` then use
- `products` — Limit to specific DCC applications

### 4. SkillDependencies (External Contracts)

Declare what a skill needs:

```yaml
external_deps:
  mcp:
    - name: "usd-tools"
      transport: "ipc"
  env_var:
    - name: "MAYA_PLUGINS_PATH"
  bin:
    - name: "ffmpeg"
```

The system validates before execution and reports missing deps.

## Session Isolation

Each AI session is pinned to one DCC instance:

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager(registry_dir="/tmp/dcc-mcp")

# AI starts a session for Maya instance
session_id = mgr.get_or_create_session(
    dcc_type="maya",
    instance_id=uuid_of_maya_instance
)

# tools/list is scoped to this session's instance
# Only Maya tools are visible; context stays lean
```

## Version-Aware Gateway Election

When multiple DCC instances run:

```
Maya 0.12.6 starts   → becomes gateway (port taken)
Maya 0.12.29 starts  → detects 0.12.6, sends POST /gateway/yield
Maya 0.12.6 sees     → version older, yields port
Maya 0.12.29        → becomes new gateway (version better)
```

No manual intervention; automatic best-version-wins.

## Getting Started

1. **Install**: `pip install dcc-mcp-core`
2. **Create skill**: Write `SKILL.md` + scripts/
3. **Register**: `scan_and_load(dcc_name="maya")`
4. **Deploy**: Use with MCP server (gateway handles routing)

See examples/skills/ for complete working examples.
