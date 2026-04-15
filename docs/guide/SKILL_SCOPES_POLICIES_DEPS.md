# Skills: Scopes, Policies, Dependencies

## SkillScope: Trust Levels

Skills are classified by source and trust level:

```
Repo (lowest trust)
  ↓
User (medium trust)
  ↓
System (high trust)
  ↓
Admin (highest trust)
```

### Defining Scope in SKILL.md

```yaml
---
name: my-skill
scope: repo          # Default if omitted
---
```

### Scope Rules

| Scope | Location | Who Owns | Conflicts? |
|-------|----------|----------|-----------|
| **repo** | `.codex/skills/` | Project | Cannot override System skills |
| **user** | `~/.skills/` | User | Can override Repo skills |
| **system** | Bundled in wheel | dcc-mcp-core | Cannot be overridden (locked) |
| **admin** | Enterprise manager | Ops team | Overrides all others |

Example: If both Repo and System have `maya-cleanup`, System wins (it's locked).

### Benefits

1. **Enterprise Control** — Admin skills always execute (can't be shadowed)
2. **User Customization** — User skills personalize workflows without breaking core tools
3. **Project Isolation** — Repo skills stay local; don't affect other projects

## SkillPolicy: Invocation Control

Control **how** a skill is discovered and called:

```yaml
---
name: my-skill
policy:
  allow_implicit_invocation: false   # Requires load_skill first
  products: ["maya", "houdini"]      # Only show for these DCCs
---
```

### allow_implicit_invocation

**true** (default):
```python
# Model can discover and call in same turn:
# tools/list sees "maya_cleanup__cleanup"
# → model calls it directly
```

**false**:
```python
# Model must explicitly load first:
tools/list → sees "load_skill__my_skill"
→ model calls: load_skill(name="my_skill")
→ tools/list now shows "maya_cleanup__cleanup"
→ model calls: maya_cleanup__cleanup()
```

**Use Case**: Dangerous skills (delete_*, format_*, etc.) should require explicit acknowledgment.

### products Filter

Scope tools to specific DCCs:

```yaml
policy:
  products: ["maya", "houdini"]  # Not shown in Blender, Photoshop, etc.
```

When AI calls `tools/list` in a Blender session, this skill is hidden.

**Use Case**: Maya-MEL-specific tools shouldn't clutter Blender's tool list.

## SkillDependencies: External Contracts

Declare what a skill needs before execution:

```yaml
---
name: usd-validator
version: "1.0.0"
external_deps:
  mcp:
    - name: "pixar-usd"
      description: "USD library for validation"
      transport: "ipc"
      url: "https://github.com/PixarAnimationStudios/USD"
  env_var:
    - name: "PYTHONPATH"
      description: "Must include USD site-packages"
  bin:
    - name: "usdview"
      description: "USD inspection tool"
---
```

### Dependency Types

| Type | What | Check Timing |
|------|------|--------------|
| **mcp** | Other MCP tools | Before execute; report if unavailable |
| **env_var** | Environment variables | Before execute; warn if missing |
| **bin** | System binaries | Before execute; fail gracefully |

### Runtime Behavior

```python
result = dispatcher.dispatch("usd_validator__validate", params)

# If dependencies missing:
if not result["success"]:
    print(result["error"])
    # "Missing dependencies: pixar-usd (MCP), usdview (binary)"
    # Action blocked; AI gets clear feedback
```

**Benefits**:
- Self-documenting (what does this skill need?)
- Prevents cryptic errors (clear feedback when deps missing)
- Enables smart routing (skip skills with unmet deps)

## Complete Example

```yaml
---
name: maya-geometry-tools
version: "2.0.0"
description: "Advanced polygon and mesh utilities for Maya"
dcc: maya
scope: repo          # Project-local

tags: ["geometry", "modeling", "mesh"]

policy:
  allow_implicit_invocation: true   # Can call directly
  products: ["maya"]                # Only in Maya

external_deps:
  bin:
    - name: "python"
      description: "Python 3.7+ required"
  env_var:
    - name: "MAYA_PLUG_IN_PATH"
      description: "Must include geometry plugin path"
  mcp:
    - name: "mesh-validator"
      description: "For validating topology"
      transport: "ipc"
---

# Maya Geometry Tools

Advanced mesh utilities for polygon modeling.
```

When this skill loads:

1. ✅ **Scope**: Checked as Repo-level (can be shadowed by User/System)
2. ✅ **Policy**: If implicit_invocation=true, shows in tools/list immediately
3. ✅ **Product**: Only if running in Maya (hidden in Blender/Houdini)
4. ✅ **Deps**: Before first call, validate Python + MAYA_PLUG_IN_PATH + mesh-validator
5. ✅ **Result**: Structured `{success, message, context, next_steps}`

---

**See Also**:
- [MCP + Skills Integration](./MCP_SKILLS_INTEGRATION.md)
- [Gateway Election](./GATEWAY_ELECTION.md)
- [examples/skills/](../../examples/skills/) for 11 working examples
