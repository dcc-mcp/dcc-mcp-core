# Thin-Harness Skill Authoring Pattern

> **TL;DR** — When no domain skill covers the user's intent, a thin-harness skill
> hands the agent a raw script executor plus a recipe book. The agent reads the
> recipe, writes the native DCC call, and submits it — no wrapper needed.
> See [ADR 003](../adr/003-thin-harness-skill-pattern.md) for the architectural rationale.

---

## When to Write a Wrapper vs. a Thin Harness

| Signal | Use this |
|--------|----------|
| Operation is 2–5 native API calls, well-documented in training data | **Thin harness** — ship `execute_python` + recipes |
| Operation requires multi-step pipeline logic (render farm, shot export) | **Domain skill** — explicit schema + error handling |
| Operation needs security validation before execution | **Domain skill** — `ToolValidator` + `SandboxPolicy` |
| You're wrapping `maya.cmds`, `bpy.ops`, or `hou.*` one-to-one | **Thin harness** — agent already knows these APIs |
| You need `next-tools` chaining across multiple DCC state changes | **Domain skill** — declare the chain explicitly |

**Rule of thumb**: If the LLM training corpus contains 10,000+ examples of the native
call, write a thin harness. If the operation is proprietary pipeline logic, write a
domain skill.

---

## Skill Layer Values

```yaml
# SKILL.md metadata
metadata:
  dcc-mcp:
    layer: thin-harness   # ← new value alongside infrastructure / domain / example
```

Routing: agents load thin-harness skills as the **fall-through** after searching
domain skills. If `search_skills(query)` returns no domain match, the agent loads
the DCC's thin-harness skill and checks `references/RECIPES.md`.

---

## Thin-Harness Skill Structure

```
my-dcc-scripting/
├── SKILL.md                      # short, layer: thin-harness
├── tools.yaml                    # execute_python + optional group
├── scripts/
│   └── execute.py                # raw script runner
└── references/
    ├── RECIPES.md                # ~20 copy-pasteable snippets
    └── INTROSPECTION.md          # how to query the live DCC namespace
```

### SKILL.md

```yaml
---
name: maya-scripting
description: >-
  Thin-harness skill — raw Maya Python script execution with recipes.
  Use when no domain skill covers the operation and the agent knows the
  maya.cmds / OpenMaya API. Not for pipeline-level intent — use
  maya-pipeline domain skills for shot export, render farm, etc.
license: MIT
metadata:
  dcc-mcp:
    dcc: maya
    layer: thin-harness
    tools: tools.yaml
    recipes: references/RECIPES.md
    introspection: references/INTROSPECTION.md
---

Execute arbitrary Python inside the live Maya session.

## When to use this skill

- The user wants to call a specific `maya.cmds.*` function directly.
- No domain skill covers the operation.
- The user wants to inspect or iterate on raw DCC API calls.

## When NOT to use this skill

- Shot export → use `maya-pipeline__export_shot`
- Render farm submission → use `maya-render__submit`
- Any operation with multi-step error recovery → use a domain skill

## Checklist before calling execute_python

1. Check `references/RECIPES.md` for a working snippet.
2. If no recipe matches, call `dcc_introspect__search` to find the right symbol.
3. Submit the script. On error, read `_meta.dcc.raw_trace` for the failing call.
```

### tools.yaml

```yaml
tools:
  - name: execute_python
    description: >-
      Execute a Python script string inside the live DCC interpreter.
      When to use: when no domain skill covers the operation and you have
      a working maya.cmds / bpy / hou snippet. How to use: pass the full
      script as 'code'; check references/RECIPES.md first.
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: false
    next-tools:
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

### scripts/execute.py

```python
from __future__ import annotations

from dcc_mcp_core import skill_entry, skill_success, skill_error


@skill_entry
def execute_python(code: str, timeout_secs: int = 30) -> dict:
    """Execute a Python script string in the live DCC interpreter.

    Args:
        code: Python source to execute.
        timeout_secs: Execution timeout. Default 30 s.

    Returns:
        skill_success with 'output' key on success, skill_error on failure.
    """
    import traceback

    local_ns: dict = {}
    try:
        exec(compile(code, "<execute_python>", "exec"), {}, local_ns)  # noqa: S102
        output = local_ns.get("result", None)
        return skill_success("Script executed", output=output)
    except Exception as exc:  # noqa: BLE001
        return skill_error(
            f"Script raised {type(exc).__name__}: {exc}",
            underlying_call=code[:200],
            traceback=traceback.format_exc(),
        )
```

---

## references/RECIPES.md Contract

A flat Markdown file with anchored `##` sections. Each section:
- One sentence describing when to use the recipe.
- A ready-to-run Python snippet (≤15 lines).
- No boilerplate imports — assume `import maya.cmds as cmds` etc. are in scope.

```markdown
## create_polygon_cube

Create a named polygon cube at the origin.

\`\`\`python
cube = cmds.polyCube(name="myCube", w=1, h=1, d=1)[0]
\`\`\`

## set_world_translation

Set absolute world-space translation (not relative).

\`\`\`python
cmds.xform("myCube", translation=(1, 2, 3), worldSpace=True)
\`\`\`
```

Recipe anchor names become searchable via `recipes__get(skill=..., anchor=...)` once
issue #428 lands.

---

## references/INTROSPECTION.md Contract

Explains how the agent can discover the live DCC namespace without reading vendor docs.

```markdown
## List a module's public names

\`\`\`python
import maya.cmds as cmds
result = [n for n in dir(cmds) if not n.startswith("_")]
\`\`\`

## Get a command's flags

\`\`\`python
help(cmds.polyCube)
\`\`\`

## Use dcc_introspect__* tools (issue #426)

Once the dcc-introspect built-in skill is loaded:
- ``dcc_introspect__list_module(module="maya.cmds")``
- ``dcc_introspect__signature(qualname="maya.cmds.polyCube")``
- ``dcc_introspect__search(pattern="poly.*", module="maya.cmds")``
```

---

## Routing in AGENTS.md

Add to `AGENTS.md` Do list (see also ADR 003):

> **If no domain skill matches the user's intent**, load the DCC's `*-scripting`
> (thin-harness) skill and read `references/RECIPES.md` before inventing a call.
> Only fall back to raw `execute_python` if no recipe matches.

---

## Error Envelope Integration (issue #427)

When a thin-harness `execute_python` call raises, the `_meta.dcc.raw_trace` block
(when `McpHttpConfig.enable_error_raw_trace = True`) gives the agent:

```jsonc
{
  "_meta": {
    "dcc.raw_trace": {
      "underlying_call": "cmds.polySphere(name='mySphere', radius=-1.0)",
      "traceback": "...",
      "recipe_hint": "references/RECIPES.md#create_sphere",
      "introspect_hint": "dcc_introspect__signature(qualname='maya.cmds.polySphere')"
    }
  }
}
```

The agent reads the trace, corrects the call, and resubmits — without asking for
a new wrapper tool.

---

## Related

- [ADR 003](../adr/003-thin-harness-skill-pattern.md) — architectural decision
- [skills/templates/thin-harness/](../../skills/templates/thin-harness/) — starter template
- [skills/README.md#skill-layering](../../skills/README.md) — layer definitions
- Issue #426 — `dcc_introspect__*` built-in tools
- Issue #427 — `_meta.dcc.raw_trace` error envelope
- Issue #428 — `metadata.dcc-mcp.recipes` formalization
