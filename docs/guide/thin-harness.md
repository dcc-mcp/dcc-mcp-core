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
    skill-reference-docs: [references/INTROSPECTION.md]
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
      a working maya.cmds / bpy / hou snippet. How to use: pass a host-local
      'file_path' (or 'script_path' alias); on adapters that still require
      inline source, materialize it first and never resend unchanged code.
      Check references/RECIPES.md first.
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

from dcc_mcp_core import skill_entry, skill_success, skill_error_with_trace


@skill_entry
def execute_python(
    file_path: str,
    script_path: str | None = None,
    params: dict | None = None,
    timeout_secs: int = 30,
) -> dict:
    """Run a host-local Python script in the live DCC interpreter.

    Args:
        file_path: Host-local path of the materialized script.
        script_path: Backward-compatible alias for file_path.
        params: Values passed to the script's ``main(**params)`` entrypoint.
        timeout_secs: Execution timeout. Default 30 s.

    Returns:
        skill_success with 'output' key on success, skill_error on failure.
    """
    import ast
    import traceback

    source_path = script_path or file_path
    source = open(source_path, encoding="utf-8").read()
    tree = ast.parse(source, filename=source_path)
    has_main = any(
        isinstance(node, ast.FunctionDef) and node.name == "main"
        for node in tree.body
    )
    params = params or {}
    local_ns: dict = {}
    try:
        exec(compile(source, source_path, "exec"), {}, local_ns)  # noqa: S102
        if has_main:
            main = local_ns["main"]
            output = main(**params)
        else:
            output = local_ns.get("result", None)
        return skill_success("Script executed", output=output)
    except Exception as exc:  # noqa: BLE001
        return skill_error_with_trace(
            f"Script raised {type(exc).__name__}: {exc}",
            "script_execution_failed",
            underlying_call=source[:200],
            tb=traceback.format_exc(),
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

Create a named polygon cube at the origin with explicit dimensions.

\`\`\`python
def main(name: str, width: float, height: float, depth: float) -> dict:
    return {"cube": cmds.polyCube(name=name, w=width, h=height, d=depth)[0]}
\`\`\`

## set_world_translation

Set absolute world-space translation (not relative) using params.

\`\`\`python
def main(object_name: str, x: float, y: float, z: float) -> dict:
    cmds.xform(object_name, translation=(x, y, z), worldSpace=True)
    return {"object": object_name, "translation": (x, y, z)}
\`\`\`
```

Recipe anchor names are searchable via `recipes__get(skill=..., anchor=...)`. Each
recipe is a parameterized `main(...)` template: agents pass object names,
dimensions, and translation values through `params` instead of copying source.

---

### Materialize once, iterate by params

When the same snippet is executed more than once, stop resubmitting the
source. Materialize it once with `materialize_script(content=...,
reuse=True, reuse_key="STABLE_KEY")`, record `file_path`, `sha256`, and
`reused`, then pass the host-local `file_path` (or `script_path`) to the
execution tool. A byte-identical repeat with the same key returns the same
path with `reused=true`; never re-send unchanged code. Reject stale or
unexpected hashes instead of executing them.

Write generated scripts as typed `def main(**params)` functions and keep
varying values in the `params` object. The runner reads the host-local file
and passes `params` to `main(**params)`. There is no CLI flag and no params
file for this channel.

For the cube recipe above, the second run changes only `params`:

```json
{"file_path": "<host-local-recipe.py>", "params": {"name": "heroCube", "width": 2.0, "height": 2.0, "depth": 2.0}}
```

Author recipes as parameterized templates. A hardcoded literal recipe forces
the agent to copy and re-send source for every value; a parameterized recipe
changes only `params`.

After the same multi-step sequence succeeds twice, promote it with
`workflows_run` using stable `idempotency_key` values; recover an interrupted
run with `workflows_resume`. See
[`agents-reference.md`](agents-reference.md#iteration-playbook).

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
- [`dcc-mcp-skills-creator`](https://github.com/dcc-mcp/dcc-mcp-agent-plugins/tree/main/plugins/dcc-mcp/skills/dcc-mcp-skills-creator/) — scaffold and validate a new thin-harness skill
- [agents-reference.md](agents-reference.md) — skill layer definitions
- Issue #426 — `dcc_introspect__*` built-in tools
- Issue #427 — `_meta.dcc.raw_trace` error envelope
- Issue #428 — `metadata.dcc-mcp.recipes` formalization
