# Gateway Election & Multi-Instance Support

## The Gateway Role

The **gateway** is a single Rust HTTP server that:

1. **Discovers** all running DCC instances (Maya, Houdini, Blender, etc.)
2. **Routes** AI requests to the right instance based on session
3. **Manages** tool discovery with scope + product filtering
4. **Handles** cancellation, notifications (SSE), and session lifecycle

Only **one gateway runs per machine** (`localhost:9999` by default).

## Version-Aware Election

### The Problem
When multiple DCC versions run simultaneously, which becomes the gateway?

**Old approach**: "First-come-first-served" (TCP port wins)
- v0.12.6 binds port → becomes gateway
- v0.12.29 arrives → can't bind, becomes subordinate client
- ❌ Old version keeps control; newer version ignored

**New approach**: **Version-aware election**
- v0.12.6 binds port → registers as gateway
- v0.12.29 arrives → reads `__gateway__` sentinel, sees v0.12.6 is running
- v0.12.29 → POST /gateway/yield (request v0.12.6 to step down)
- v0.12.6 → recognizes newer challenger, gracefully yields
- v0.12.29 → binds port, becomes new gateway
- ✅ Newest version automatically assumes control

### Implementation Details

**1. `__gateway__` Sentinel Entry**

FileRegistry stores a special entry when a gateway starts:

```json
{
  "dcc_type": "__gateway__",
  "instance_id": "sentinel",
  "version": "0.12.29",
  "status": "AVAILABLE"
}
```

New instances query this to know:
- Is a gateway running?
- What version is it?
- Should I try to take over?

**2. Version Comparison**

Semantic versioning (not string comparison):

```
v0.12.6  vs  v0.12.29
0.12.6       0.12.29
├─ compare─┘
     0 == 0 (major equal)
     12 == 12 (minor equal)
     6 < 29 (patch: 6 is older)
→ v0.12.6 is older → must yield
```

**3. Graceful Handoff**

Old gateway:
```
cleanup task every 15s:
  - reads FileRegistry
  - if challenger (v0.12.29) is running && v0.12.29 > my_version:
      yield_tx.send(true)  // trigger axum graceful shutdown
      release port
      become client
```

New gateway:
```
retry loop every 10s:
  - try to bind :9999
  - if success → register __gateway__ sentinel
  - if fail → wait and retry (max 120s)
```

## Instance Tracking

Each DCC registers metadata:

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager(registry_dir="/tmp/dcc-mcp")

# Maya plugin calls:
instance_id, listener = mgr.bind_and_register(
    dcc_type="maya",
    version="2025",
    pid=12345,
    display_name="Maya-Production",
    documents=["scene.ma", "rig.ma"]
)

# Now discoverable with full context:
entry = mgr.get_service("maya", instance_id)
print(entry.documents)      # ["scene.ma", "rig.ma"]
print(entry.display_name)   # "Maya-Production"
print(entry.pid)            # 12345
```

## Session Isolation

Sessions are **always pinned to one instance**:

```python
# Client (AI) perspective:
session = transport.get_or_create_session(
    dcc_type="maya",
    instance_id=uuid  # Explicitly pin to this instance
)

# tools/list is scoped to this instance's tools
# No cross-instance bleeding of context
```

## Smart Routing

The gateway intelligently selects instances:

```python
# Prefer documents.contains(hint) if provided:
session = transport.get_or_create_session_routed(
    dcc_type="maya",
    strategy=RoutingStrategy.MostRecent,  # Highest API version
    hint="project.ma"  # Prefer instance with this document
)
```

Priority order:
1. Explicit instance_id (if provided)
2. Document hint match (if available)
3. RoutingStrategy (AVAILABLE/BUSY/MostRecent/LeastBusy)
4. First available

## Backward Compatibility

- Older DCCs (v0.12.6) that don't support `yield` ignore POST /gateway/yield
- They continue as gateway until timeout/crash
- Newer challengers keep polling port every 10s
- System eventually becomes consistent once old DCC exits

No hard failures; graceful degradation.
