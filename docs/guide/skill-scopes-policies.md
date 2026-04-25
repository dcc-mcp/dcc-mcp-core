# Skill Scopes & Policies

> **[中文版](../zh/guide/skill-scopes-policies)**

Skills have two new mechanisms for fine-grained control: **scopes** (trust levels) and **policies** (invocation rules).

## SkillScope: Trust Levels

Skills are classified by source trust:

```
Admin   (highest trust — enterprise managed)
  ↑
System  (bundled with dcc-mcp-core — verified)
  ↑
User    (~/.skills/ — personal workflows)
  ↑
Repo    (project-local skills/, lowest trust)
```

### How Scope Is Assigned

Scope is determined by **which path** the skill was discovered from:

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry

catalog = SkillCatalog(ToolRegistry())

# Skills discovered here get scope="repo"
catalog.discover(extra_paths=["skills"])

# Check scope in summaries
for skill in catalog.list_skills():
    print(f"{skill.name}: scope={skill.scope}")
```

### Scope in SkillSummary

After discovery, `list_skills()` returns `SkillSummary` objects with:

```python
summaries = catalog.list_skills()
for s in summaries:
    print(s.name)              # "maya-geometry"
    print(s.scope)             # "repo" | "user" | "system" | "admin"
    print(s.implicit_invocation)  # True | False
```

### Why Scopes Matter

- **Enterprise** — Admin skills always execute; can't be shadowed by project skills
- **Multi-project** — System skills available globally; repo skills stay project-local
- **Security** — Repo skills (untrusted code from a cloned repo) can't override System skills

## SkillPolicy: Invocation Control

Declare in `SKILL.md` frontmatter how AI agents may invoke a skill:

```yaml
---
name: maya-cleanup
dcc: maya
policy:
  allow_implicit_invocation: false   # Require explicit load_skill call
  products: ["maya", "houdini"]      # Only visible for these DCCs
---
```

### allow_implicit_invocation

Controls whether AI can call the skill immediately from `tools/list`.

| Value | Behavior |
|-------|----------|
| `true` (default) | Tool appears in `tools/list` and can be called directly |
| `false` | Tool is **hidden** until client calls `load_skill(name)` explicitly |

**Use `false` for:**
- Destructive operations (`delete_all_nodes`, `reset_scene`)
- High-cost tools (full render, simulation bake)
- Tools requiring user confirmation first

```python
from dcc_mcp_core import SkillMetadata
import json

md = SkillMetadata("secure-tool")
md.policy = json.dumps({"allow_implicit_invocation": False})

# Check:
md.is_implicit_invocation_allowed()  # → False
```

### products: Product Filter

Restrict skill visibility to specific DCC applications:

```yaml
policy:
  products: ["maya"]           # Only in Maya sessions
  # products: ["maya", "houdini"]  # Both Maya and Houdini
  # products: []               # All DCCs (default when policy is absent)
```

This prevents Maya MEL scripts from appearing in a Blender session.

```python
md = SkillMetadata("maya-mel-tool")
md.policy = json.dumps({"products": ["maya"]})

md.matches_product("maya")    # → True
md.matches_product("blender") # → False
md.matches_product("houdini") # → False
```

**Product matching is case-insensitive:**
```python
md.matches_product("MAYA")    # → True
md.matches_product("Maya")    # → True
```

## SkillDependencies: External Contracts

Declare what your skill requires before execution:

```yaml
---
name: usd-validator
external_deps:
  tools:
    - type: mcp
      value: "pixar-usd"
      description: "USD validation MCP server"
      transport: "ipc"
    - type: env_var
      value: "PYTHONPATH"
      description: "Must include USD site-packages"
    - type: bin
      value: "usdview"
      description: "USD inspection tool"
---
```

### Dependency Types

| Type | `value` field | Purpose |
|------|--------------|---------|
| `mcp` | MCP server name | Requires a running MCP service |
| `env_var` | Variable name | Requires an environment variable |
| `bin` | Binary name | Requires a system binary in PATH |

### Python API

```python
from dcc_mcp_core import SkillMetadata
import json

md = SkillMetadata("usd-validator")

# Set dependencies via JSON
deps = {
    "tools": [
        {"type": "env_var", "value": "PYTHONPATH"},
        {"type": "bin", "value": "usdview"},
    ]
}
md.external_deps = json.dumps(deps)

# Read back
print(md.external_deps)  # JSON string or None
```

## Complete Example

```yaml
---
name: maya-scene-publisher
version: "2.0.0"
description: "Production scene publishing with validation"
dcc: maya
scope: repo           # Project-local skill

policy:
  allow_implicit_invocation: false   # User must explicitly load
  products: ["maya"]                  # Maya only

external_deps:
  tools:
    - type: env_var
      value: "PIPELINE_ROOT"
      description: "Pipeline root directory"
    - type: mcp
      value: "asset-tracker"
      description: "Asset tracking MCP service"
    - type: bin
      value: "mayapy"
      description: "Maya Python interpreter"
---

# Maya Scene Publisher

Validates and publishes scenes to the production pipeline.
```

When this skill loads:

1. 🔒 **Scope**: `repo` — can be overridden by User/System skills
2. 🔐 **Policy**: `allow_implicit_invocation: false` — requires explicit `load_skill`
3. 🎯 **Product**: Only visible in Maya sessions; hidden in Blender/Houdini
4. 📋 **Deps**: Validates `PIPELINE_ROOT`, `asset-tracker`, `mayapy` before first call
