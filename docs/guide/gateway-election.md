# Gateway Election & Multi-Instance Support

> **[中文版](../zh/guide/gateway-election)**

## What is the Gateway?

The **gateway** is a single Rust HTTP server (running on `localhost:9999` by default) that:

- Discovers all running DCC instances (Maya, Blender, Houdini, Photoshop, etc.)
- Routes AI requests to the correct instance based on session
- Exposes a scoped `tools/list` per session (prevents context explosion)
- Handles cancellation and SSE notifications

**One gateway per machine**. It's started automatically when the first DCC instance registers.

## The Problem: First-Come-First-Served

Without version awareness, the oldest DCC wins the gateway role:

```
Maya v0.12.6 starts → binds port 9999 → becomes gateway
Maya v0.12.29 starts → port 9999 taken → becomes subordinate
❌ Old version controls routing; new features ignored
```

## Our Solution: Version-Aware Election

```
Maya v0.12.6 (gateway)           Maya v0.12.29 (new)
         │                                │
         │                   port 9999 taken
         │                                │
         │         read __gateway__ sentinel
         │         own version 0.12.29 > gw 0.12.6
         │                                │
         │ ←── POST /gateway/yield {"challenger_version": "0.12.29"}
         │
         │ (supports yield → graceful shutdown)
         │ yield_tx.send(true)
         │ release port 9999
         │
                          retry every 10s
                          port free → bind
                          register new sentinel
                          ✅ v0.12.29 is now gateway
```

### How It Works

**1. `__gateway__` Sentinel**

When a gateway starts, it writes a special entry to the FileRegistry:
```json
{"dcc_type": "__gateway__", "version": "0.12.29"}
```

New instances read this to know who the gateway is and what version it runs.

**2. Semantic Version Comparison**

Versions are compared numerically (not alphabetically):
```
0.12.6  vs  0.12.29
↓              ↓
[0, 12, 6]  [0, 12, 29]
                 29 > 6 → v0.12.29 is newer ✓
```

**3. Voluntary Yield**

The cleanup task (every 15s) checks for newer challengers. If found, it shuts down gracefully.

**4. Challenger Retry Loop**

New instances poll the port every 10s for up to 120s. As soon as the port is free, they take over.

## Multi-Instance Registration

Multiple DCC instances of the same type can coexist:

```python
from dcc_mcp_core import TransportManager
import os

mgr = TransportManager("/tmp/dcc-mcp")

# Maya #1: animation work
iid_anim = mgr.register_service(
    "maya", "127.0.0.1", 18812,
    pid=os.getpid(),
    display_name="Maya-Animation",
    scene="shot_001.ma",
    documents=["shot_001.ma", "shot_002.ma"],
    version="2025",
)

# Maya #2: rigging work
iid_rig = mgr.register_service(
    "maya", "127.0.0.1", 18813,
    pid=12345,
    display_name="Maya-Rigging",
    scene="character_rig.ma",
    documents=["character_rig.ma"],
    version="2025",
)

# Find all Maya instances
instances = mgr.list_instances("maya")
# → [Maya-Animation, Maya-Rigging]

# Find best instance (AVAILABLE > BUSY; IPC > TCP)
best = mgr.find_best_service("maya")

# Rank all instances by preference
ranked = mgr.rank_services("maya")
```

## Document Tracking

For multi-document DCCs (Photoshop, After Effects), track all open files:

```python
# Photoshop registers with initial documents
iid = mgr.register_service(
    "photoshop", "127.0.0.1", 18820,
    pid=55001,
    display_name="PS-Marketing",
    scene="logo.psd",
    documents=["logo.psd", "banner.psd"],
)

# User opens a new document
mgr.update_documents(
    "photoshop", iid,
    active_document="icon.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

# User switches active document
mgr.update_documents(
    "photoshop", iid,
    active_document="banner.psd",
    documents=["logo.psd", "banner.psd", "icon.psd"],
)

entry = mgr.get_service("photoshop", iid)
print(entry.scene)      # "banner.psd" (active)
print(entry.documents)  # ["logo.psd", "banner.psd", "icon.psd"]
```

## Session Isolation

Each AI session is **pinned to one instance**:

```python
# AI Agent A talks only to Maya-Animation
session_a = mgr.get_or_create_session("maya", iid_anim)

# AI Agent B talks only to Maya-Rigging
session_b = mgr.get_or_create_session("maya", iid_rig)

# Sessions are different — no context bleeding
assert session_a != session_b

# tools/list scoped to each session's instance
# Agent A sees: maya_anim__set_keyframe, ...
# Agent B sees: maya_rig__mirror_joints, ...
```

## Instance Health

```python
# Keep instance alive with heartbeats
mgr.heartbeat("maya", iid)  # → True if alive, False if not found

# Update instance status
from dcc_mcp_core import ServiceStatus
mgr.update_service_status("maya", iid, ServiceStatus.BUSY)

# Cleanup when DCC exits
mgr.deregister_service("maya", iid)
```

## Backward Compatibility

Older DCC versions that don't support `/gateway/yield` return 404 — that's OK. The challenger enters a polling retry loop and waits until the port is naturally freed (when old DCC exits or crashes). No hard failures; graceful degradation.
