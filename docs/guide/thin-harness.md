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
3. Materialize one typed `def main(**params)`, then execute its `file_path`.
   On error, read `_meta.dcc.raw_trace` for the failing call.
```

### tools.yaml

```yaml
tools:
  - name: execute_python
    description: >-
      Execute a reviewed inline or materialized Python script inside the live DCC interpreter.
      When to use: when no domain skill covers the operation and you have
      a working maya.cmds / bpy / hou snippet. Check references/RECIPES.md
      first; materialize once, then pass file_path plus params.
    input_schema:
      type: object
      properties:
        code: {type: string, description: "Source for the first materialization only."}
        file_path: {type: string, description: "Trusted host-local materialized script path."}
        script_path: {type: string, description: "Compatibility alias for file_path."}
        params: {type: object, description: "Values passed to a typed main(**params)."}
        sha256: {type: string, description: "Optional integrity assertion for the selected file."}
        timeout_secs: {type: integer, minimum: 1, default: 30}
      additionalProperties: false
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: false
      open_world_hint: false
    next-tools:
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

### scripts/execute.py

```python
from __future__ import annotations

import hashlib
from typing import Any, Dict, Optional

from dcc_mcp_core import (
    ToolValidator,
    derive_script_parameters_schema,
    json_dumps,
    skill_entry,
    skill_error_with_trace,
    skill_success,
    validate_script_file_path,
)


@skill_entry
def execute_python(
    code: Optional[str] = None,
    file_path: Optional[str] = None,
    script_path: Optional[str] = None,
    params: Optional[Dict[str, Any]] = None,
    sha256: Optional[str] = None,
    timeout_secs: int = 30,
) -> dict:
    """Execute one inline source or one trusted materialized file."""
    import traceback

    try:
        selected_path = file_path or script_path
        if bool(code) == bool(selected_path):
            raise ValueError("provide exactly one of code or file_path/script_path")
        source = code or validate_script_file_path(selected_path).read_text(encoding="utf-8")
        actual_sha256 = hashlib.sha256(source.encode("utf-8")).hexdigest()
        if sha256 is not None and sha256 != actual_sha256:
            raise ValueError("sha256 does not match the selected script")

        local_ns: dict = {}
        exec(  # noqa: S102
            compile(source, selected_path or "<execute_python>", "exec"),
            local_ns,
            local_ns,
        )
        if params is not None:
            schema = derive_script_parameters_schema(source)
            if schema is None or "main" not in local_ns:
                raise ValueError("params require a fully typed main(...) entry point")
            valid, errors = ToolValidator.from_schema_json(json_dumps(schema)).validate(json_dumps(params))
            if not valid:
                raise ValueError("params failed schema validation: " + "; ".join(errors))
            output = local_ns["main"](**params)
        else:
            output = local_ns.get("result")
        return skill_success("Script executed", output=output)
    except Exception as exc:  # noqa: BLE001
        return skill_error_with_trace(
            f"Script raised {type(exc).__name__}: {exc}",
            "script_execution_failed",
            underlying_call="<materialized file>" if selected_path else "<inline source redacted>",
            tb=traceback.format_exc(),
        )
```

---

## references/RECIPES.md Contract

A flat Markdown file with anchored `##` sections. Each section:
- One sentence describing when to use the recipe.
- A parameterized typed `main(...)` snippet (≤15 lines), with changing values
  outside the source.
- No boilerplate imports — assume `import maya.cmds as cmds` etc. are in scope.

```markdown
## create_polygon_cube

Create a named polygon cube at the origin with caller-supplied values.

\`\`\`python
def main(name: str = "myCube", size: float = 1.0):
    return cmds.polyCube(name=name, w=size, h=size, d=size)[0]
\`\`\`

## set_world_translation

Set caller-supplied absolute world-space translation (not relative).

\`\`\`python
def main(name: str, x: float, y: float, z: float):
    cmds.xform(name, translation=(x, y, z), worldSpace=True)
    return name
\`\`\`
```

Recipe anchors are searchable now through `recipes__list`, `recipes__search`,
and `recipes__get`; use `recipes__validate` before applying edited content and
`recipes__apply` only after the selected arguments are reviewed.

## Materialize Once, Iterate by Parameters

Use the recipe source to create one host-local file with `reuse=true` and a
stable `reuse_key`. Record its `file_path`, `sha256`, and
`parameters_schema`. Iteration one and iteration two both execute that exact
path; only the `params` object changes. A repeated materialization of unchanged
source must report `reused=true`. Do not use `--json-file` to resend the full
script on every correction.

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

## Use shipped dcc_introspect__* tools

Once the `dcc-introspect` built-in skill is loaded:
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

The agent reads the trace, corrects only `params`, and re-executes the same
reviewed `file_path`. Rematerialize only when the source itself is wrong; do
not ask for a new wrapper tool.

---

## Related

- [ADR 003](../adr/003-thin-harness-skill-pattern.md) — architectural decision
- [`dcc-mcp-skills-creator`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/skills/dcc-mcp-skills-creator/) — scaffold and validate a new thin-harness skill
- [agents-reference.md](agents-reference.md) — skill layer definitions
- `dcc_introspect__*` built-in tools (shipped from #426)
- Issue #427 — `_meta.dcc.raw_trace` error envelope
- `metadata.dcc-mcp.recipes` and `recipes__*` tools (shipped from #428/#616)
