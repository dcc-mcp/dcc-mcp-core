# ADR 003 — Thin-Harness Skill Authoring Pattern

**Status**: Accepted
**Date**: 2026-04-25
**Relates to**: Issue #425, [The Bitter Lesson of Agent Harnesses](https://sotasync.com/reader/2026-04-24-bitter-lesson-agent-harnesses/)

---

## Context

DCC software APIs (`maya.cmds`, `bpy.ops`, `hou.*`, `pymxs.runtime`) are massively
represented in LLM training corpora. Downstream adapters (e.g. `dcc-mcp-maya`) have
historically shipped dozens of wrapper skills, each encoding 2–5 `cmds.*` calls.

These thin wrappers create a **new naming convention + schema** the model must learn,
instead of leveraging what it already knows. When a wrapper is missing, the agent
asks for it — rather than writing the native call itself.

Browser Use rewrote their agent harness from thousands of DOM helpers down to ~600
lines: a thin CDP wrapper and a SKILL.md telling the agent how to use it. The agent
now reads errors, greps references, and writes the function itself.

---

## Decision

Adopt a **thin-harness skill layer** alongside the existing `infrastructure` / `domain` /
`example` layers. A thin-harness skill:

1. **Exposes raw script execution** — one `execute_python` (or DCC-equivalent) tool,
   not a wrapper per API call.
2. **Ships a `references/RECIPES.md`** sibling file with ~20 copy-pasteable snippets
   for the most common operations. The agent reads the recipe, not the wrapper.
3. **Ships a `references/INTROSPECTION.md`** sibling file explaining how to query
   the live DCC namespace at runtime (`dcc_introspect__signature`, `dir()`, etc.).
4. **Is the primary fall-through** when no domain skill matches the user's intent.

---

## Rationale

| Approach | Pro | Con |
|----------|-----|-----|
| Wrapper per API call | Explicit schema validation | Model learns new names; N wrappers to maintain |
| Thin harness + recipes | Leverages training corpus; 1 tool to maintain | Less input validation; agent writes raw code |
| No skill at all | Zero maintenance | No guidance, no recipes, no tool listing |

The thin-harness pattern wins for **well-trained APIs** (anything with Python bindings
in the model's training set). Domain skills still win for **pipeline-level intent** —
multi-step operations where explicit state management and error handling matter more
than raw API flexibility.

---

## Consequences

- **New `thin-harness` layer value** for `metadata.dcc-mcp.layer` in SKILL.md.
- **New template** at `skills/templates/thin-harness/`.
- **New routing guidance** in `docs/guide/thin-harness.md`.
- **Downstream adapters** should elevate their existing `*-scripting` skills to
  thin-harness layer and add `references/RECIPES.md` + `references/INTROSPECTION.md`.
- **Domain skills** are unchanged — the thin-harness layer complements, not replaces.
- **AGENTS.md Do list** updated: "If no domain skill matches, load the thin-harness
  skill and check `references/RECIPES.md` before inventing a call."

---

## Related ADRs

- ADR 002 — DCC Main-Thread Affinity (the `DeferredExecutor` pattern that
  thin-harness skills must respect when making scene-mutating calls)
