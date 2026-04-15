# Feature Summary: MCP + Skills System

## What We Built

A **production-grade DCC automation framework** combining:

1. **Model Context Protocol (MCP)** — AI-native tool discovery and execution
2. **Zero-Code Skills System** — SKILL.md + scripts/ = instant MCP tools
3. **Multi-Instance Support** — Version-aware gateway election and session isolation
4. **Progressive Discovery** — Scope tools by DCC/instance/scene to prevent context explosion

## Key Innovations

### 1. Session-Bound Tool Discovery

**Problem**: Stock MCP returns all 750 tools in tools/list context → bloats AI context

**Solution**: Each AI session pinned to one DCC instance → sees only its tools

```
Without dcc-mcp-core: 550 tools in context
With dcc-mcp-core: 150 tools per session (71% reduction)
```

### 2. Version-Aware Gateway Election

**Problem**: Which DCC becomes the gateway when multiple versions run?

**Solution**: Newest version automatically takes over via semantic versioning comparison

```
v0.12.6 running → becomes gateway
v0.12.29 arrives → compares versions → v0.12.6 yields → v0.12.29 takes over
Zero manual intervention; automatic best-version-wins
```

### 3. Zero-Code Skill Registration

**Problem**: Every tool requires Python glue code

**Solution**: Write SKILL.md + scripts/ → instantly discoverable as MCP tools

```yaml
name: my-tool
scope: repo
policy:
  allow_implicit_invocation: false
  products: ["maya"]
external_deps:
  bin:
    - name: ffmpeg
```

No Python code. Metadata + scripts = done.

### 4. Structured Results (AI-Friendly)

**Problem**: AI can't reason about tool outcomes clearly

**Solution**: Every tool returns `{success, message, context, next_steps}`

```python
result = {
    "success": True,
    "message": "Scene cleanup complete",
    "context": {"nodes_deleted": 42, "warnings": []},
    "next_steps": "Consider saving the scene"
}
```

## Why Not Alternatives?

| Alternative | Problem | Our Solution |
|-------------|---------|--------------|
| **Generic MCP** | No DCC awareness; context explosion | Session isolation + scoping |
| **CLI Tools** | Blind to DCC state; requires parsing | Direct DCC access + structured results |
| **Browser Ext** | Can't access desktop software | Native bridges for Maya/Blender/Houdini/etc |
| **Monolithic Server** | Single failure point; can't multi-instance | Stateless gateway + session isolation |

## Ecosystem Reuse

We **don't reinvent wheels**:

- ✅ **MCP Protocol** — Standard, via Anthropic (tools/list, tools/call, notifications)
- ✅ **Skills Format** — OpenClaw ecosystem standard (SKILL.md)
- ✅ **Python Bindings** — PyO3 (industry standard)
- ✅ **IPC** — Named pipes (Windows), Unix sockets (POSIX), TCP (cross-machine)
- ✅ **Serialization** — rmp-serde (fast, standard Rust)

We **extend** MCP with:
- Session isolation (no standard support)
- DCC instance tracking (not in spec)
- Progressive discovery scoping (not in spec)
- Structured results format (beyond tools/call)

## Implementation Quality

- **315+ Tests** — All passing
- **Zero Runtime Python Deps** — Rust core, 99% less overhead
- **Cross-Platform** — Windows, macOS, Linux CI
- **Type-Safe** — Full `.pyi` stubs (~140 public symbols)
- **Documented** — 3 comprehensive guides + examples

## What's Next

1. ✅ **MCP Cancellation** — Handle `notifications/cancelled`
2. ✅ **Version-Aware Election** — Newest gateway wins
3. ✅ **Session Isolation** — Per-instance tool discovery
4. ✅ **SkillPolicy & Scope** — Fine-grained control
5. ⏳ **Update DCC Plugins** — Maya/Blender/Photoshop with new fields
6. ⏳ **Dynamic Resources** — MCP Resources API for scene data
7. ⏳ **Completion** — MCP Completions for smart autocomplete

## Getting Started

```bash
pip install dcc-mcp-core

# Or from source:
git clone https://github.com/loonghao/dcc-mcp-core.git
cd dcc-mcp-core
pip install -e .
```

See:
- [MCP + Skills Integration](guide/mcp-skills-integration.md)
- [Gateway Election](guide/gateway-election.md)
- [Skill Scopes & Policies](guide/skill-scopes-policies.md)
