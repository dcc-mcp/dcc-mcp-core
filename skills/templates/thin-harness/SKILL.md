---
name: <dcc>-scripting
description: >-
  Thin-harness skill — raw <DCC> Python script execution with recipes.
  Use when no domain skill covers the operation and the agent knows the
  <dcc> API. Not for pipeline-level intent — use domain skills for
  multi-step workflows, shot export, render farm, etc.
license: MIT
metadata:
  dcc-mcp:
    dcc: <dcc>
    layer: thin-harness
    tools: tools.yaml
    recipes: references/RECIPES.md
    introspection: references/INTROSPECTION.md
---

Execute arbitrary Python inside the live <DCC> session.

## When to use this skill

- The user wants to call a specific `<dcc>.*` function directly.
- No domain skill covers the operation.
- The user wants to inspect or iterate on raw DCC API calls.

## When NOT to use this skill

- Multi-step pipeline operations → use a domain skill
- Operations requiring explicit error recovery → use a domain skill
- Any operation where the user's intent maps to a named workflow → use a domain skill

## Checklist before calling execute_python

1. Check `references/RECIPES.md` for a working snippet.
2. If no recipe matches, use `dcc_introspect__search` or `dir()` to find the right symbol.
3. Submit the script. On error, read `_meta.dcc.raw_trace` for the failing call.
4. Correct the call and resubmit — no new wrapper tool needed.
